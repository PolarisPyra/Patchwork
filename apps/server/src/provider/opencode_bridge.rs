//! Runs one OpenCode 2.x turn through a private `opencode serve`, so the turn can ask for
//! approvals, ask questions, and take steering messages. Patchwork starts this bridge as the
//! turn's process: the first stdin line is the turn, later lines are replies and steering
//! messages, and stdout carries the session's server events as JSON lines, plus `bridge.*`
//! lines of its own. The bridge and its server end with the turn.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};

/// The argument that starts Patchwork's own binary as the bridge.
pub const BRIDGE_ARG: &str = "--opencode-bridge";

/// How long the server may take to print the address it listens on.
const START_TIMEOUT: Duration = Duration::from_secs(30);

/// The turn, as the first stdin line.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// The `opencode` binary.
    pub bin: String,
    /// The project or chat folder the turn works in.
    pub root: String,
    pub session_id: Option<String>,
    /// `provider/model`.
    pub model: Option<String>,
    /// The model's variant, OpenCode's reasoning effort.
    pub effort: Option<String>,
    /// `build`, or `plan` in plan mode.
    pub agent: String,
    /// The session's permission rules; empty keeps OpenCode's own.
    pub permissions: Value,
    pub text: String,
    /// Images sent with the message.
    pub files: Vec<String>,
}

/// Runs the bridge; the process exits with what this returns.
pub fn run() -> i32 {
    let (tx, rx) = mpsc::channel();
    let mut first = String::new();
    let mut stdin = BufReader::new(std::io::stdin());
    if stdin.read_line(&mut first).unwrap_or(0) == 0 {
        emit(&json!({"type": "bridge.error", "message": "The OpenCode bridge got no turn"}));
        return 1;
    }
    let request: Request = match serde_json::from_str(&first) {
        Ok(request) => request,
        Err(e) => {
            emit(&json!({"type": "bridge.error", "message": format!("Bad turn for OpenCode: {e}")}));
            return 1;
        }
    };
    let input = tx.clone();
    std::thread::spawn(move || {
        for line in stdin.lines() {
            let Ok(line) = line else { break };
            if input.send(Msg::Input(line)).is_err() {
                break;
            }
        }
    });
    match turn(&request, &tx, &rx) {
        Ok(()) => 0,
        Err(message) => {
            emit(&json!({"type": "bridge.error", "message": message}));
            1
        }
    }
}

enum Msg {
    /// A line Patchwork wrote to the bridge's stdin.
    Input(String),
    /// The server took a queued message into the turn.
    Delivered(String),
    /// The session's run ended.
    Ended,
    /// The event stream closed.
    StreamClosed,
}

fn turn(request: &Request, tx: &Sender<Msg>, rx: &Receiver<Msg>) -> Result<(), String> {
    let server = Server::start(request)?;
    let api = Api::new(&server.url, &server.password);
    let session_id = match &request.session_id {
        Some(id) if api.get(&format!("/api/session/{id}")).is_ok() => {
            if let Some(model) = model_ref(request) {
                api.post(&format!("/api/session/{id}/model"), &json!({ "model": model }))?;
            }
            api.post(&format!("/api/session/{id}/agent"), &json!({ "agent": request.agent }))?;
            id.clone()
        }
        // The session is gone, so the turn runs again in a new one with the conversation.
        Some(_) => {
            emit(&json!({"type": "bridge.session_lost"}));
            return Ok(());
        }
        None => {
            let mut body = json!({
                "agent": request.agent,
                "location": { "directory": request.root },
            });
            if let Some(model) = model_ref(request) {
                body["model"] = model;
            }
            let created = api.post("/api/session", &body)?;
            let id = created["data"]["id"]
                .as_str()
                .ok_or("OpenCode did not return a session")?
                .to_string();
            emit(&json!({"type": "bridge.session", "sessionID": id}));
            id
        }
    };
    api.patch(
        &format!("/api/session/{session_id}"),
        &json!({ "permissions": request.permissions }),
    )?;
    // Subscribed before the prompt, so no event of the turn is missed.
    events(&api, &session_id, tx.clone())?;
    api.post(
        &format!("/api/session/{session_id}/prompt"),
        &prompt_body(&request.text, &request.files),
    )?;

    // Steering messages sent but not yet taken into the turn; the turn ends once none wait.
    let mut steers = HashSet::new();
    for msg in rx {
        match msg {
            Msg::Input(line) => command(&api, &session_id, &line, &mut steers),
            Msg::Delivered(id) => {
                if steers.remove(&id) {
                    emit(&json!({"type": "bridge.steered"}));
                }
            }
            Msg::Ended if steers.is_empty() => return Ok(()),
            Msg::Ended => {}
            Msg::StreamClosed => return Err("OpenCode's event stream closed".into()),
        }
    }
    Ok(())
}

/// Acts on one line from Patchwork: an approval or question reply, a steering message, or a stop.
fn command(api: &Api, session_id: &str, line: &str, steers: &mut HashSet<String>) {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return };
    let request = v["requestID"].as_str().unwrap_or_default();
    let result = match v["type"].as_str() {
        Some("permission") => api
            .post(
                &format!("/api/session/{session_id}/permission/{request}/reply"),
                &json!({ "decision": v["decision"] }),
            )
            .map(drop),
        Some("question") => api
            .post(
                &format!("/api/session/{session_id}/question/{request}/reply"),
                &json!({ "answers": v["answers"] }),
            )
            .map(drop),
        Some("question_reject") => api
            .post(&format!("/api/session/{session_id}/question/{request}/reject"), &json!({}))
            .map(drop),
        Some("steer") => {
            let files: Vec<String> = v["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|f| f.as_str().map(String::from))
                .collect();
            let text = v["text"].as_str().unwrap_or_default();
            match api.post(
                &format!("/api/session/{session_id}/prompt"),
                &prompt_body(text, &files),
            ) {
                Ok(sent) => {
                    if let Some(id) = sent["data"]["id"].as_str() {
                        steers.insert(id.to_string());
                    }
                    Ok(())
                }
                Err(message) => {
                    emit(&json!({"type": "bridge.steer_rejected", "message": message}));
                    Ok(())
                }
            }
        }
        Some("interrupt") => api
            .post(&format!("/api/session/{session_id}/interrupt"), &json!({}))
            .map(drop),
        _ => Ok(()),
    };
    if let Err(message) = result {
        log_error(&message);
    }
}

/// `{text, files, delivery}`: steering delivers a message into the running turn.
fn prompt_body(text: &str, files: &[String]) -> Value {
    let files: Vec<Value> = files
        .iter()
        .map(|path| {
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            json!({ "uri": format!("file://{path}"), "name": name })
        })
        .collect();
    json!({ "text": text, "files": files, "delivery": "steer" })
}

/// `provider/model` as OpenCode's model reference, with the effort as its variant.
fn model_ref(request: &Request) -> Option<Value> {
    let (provider, model) = request.model.as_deref()?.split_once('/')?;
    let mut model = json!({ "providerID": provider, "id": model });
    if let Some(effort) = &request.effort {
        model["variant"] = json!(effort);
    }
    Some(model)
}

/// Streams the server's events in a thread: the session's own go to stdout, with each tool
/// event carrying the tool's name and input, which OpenCode sends only once.
fn events(api: &Api, session_id: &str, tx: Sender<Msg>) -> Result<(), String> {
    let response = api.stream("/api/event")?;
    let session_id = session_id.to_string();
    std::thread::spawn(move || {
        let mut tools: HashMap<String, (String, Value)> = HashMap::new();
        for line in BufReader::new(response).lines() {
            let Ok(line) = line else { break };
            let Some(data) = line.strip_prefix("data:") else { continue };
            let Ok(mut event) = serde_json::from_str::<Value>(data.trim()) else { continue };
            if event["data"]["sessionID"].as_str() != Some(&session_id) {
                continue;
            }
            let kind = event["type"].as_str().unwrap_or_default().to_string();
            let call = event["data"]["id"].as_str().unwrap_or_default().to_string();
            match kind.as_str() {
                "session.tool.input.started" => {
                    let name = event["data"]["name"].as_str().unwrap_or_default();
                    tools.insert(call, (name.into(), Value::Null));
                    continue;
                }
                "session.tool.called" => {
                    let entry = tools.entry(call).or_default();
                    entry.1 = event["data"]["input"].clone();
                    event["data"]["name"] = json!(entry.0);
                }
                "session.tool.success" | "session.tool.failed" => {
                    if let Some((name, input)) = tools.remove(&call) {
                        event["data"]["name"] = json!(name);
                        event["data"]["input"] = input;
                    }
                }
                "session.inbox.delivered" => {
                    if let Some(id) = event["data"]["inboxID"].as_str() {
                        let _ = tx.send(Msg::Delivered(id.to_string()));
                    }
                }
                _ => {}
            }
            emit(&event);
            if kind.starts_with("session.execution.") && kind != "session.execution.started" {
                let _ = tx.send(Msg::Ended);
            }
        }
        let _ = tx.send(Msg::StreamClosed);
    });
    Ok(())
}

fn emit(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn log_error(message: &str) {
    let _ = writeln!(std::io::stderr(), "opencode bridge: {message}");
}

/// The private server, stopped when the turn ends.
struct Server {
    child: Child,
    url: String,
    password: String,
}

impl Server {
    fn start(request: &Request) -> Result<Self, String> {
        let password = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let mut child = Command::new(&request.bin)
            .args(["serve", "--hostname", "127.0.0.1", "--port", "0"])
            .current_dir(&request.root)
            // 2.x works in the folder `PWD` names, and takes its password from OPENCODE_PASSWORD.
            .env("PWD", &request.root)
            .env("OPENCODE_PASSWORD", &password)
            .env_remove("OPENCODE_SERVER_PASSWORD")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start OpenCode: {e}"))?;
        let stdout = child.stdout.take().ok_or("OpenCode's output is unavailable")?;
        let stderr = child.stderr.take().ok_or("OpenCode's output is unavailable")?;
        // The server's own messages go to the bridge's stderr, where Patchwork keeps the tail.
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log_error(&line);
            }
        });
        let (found, url) = mpsc::channel();
        std::thread::spawn(move || {
            let mut found = Some(found);
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(url) = line.split("server listening on").nth(1)
                    && let Some(found) = found.take()
                {
                    let _ = found.send(url.trim().to_string());
                }
            }
        });
        let mut server = Server { child, url: String::new(), password };
        server.url = url
            .recv_timeout(START_TIMEOUT)
            .map_err(|_| "OpenCode's server did not start".to_string())?;
        Ok(server)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Requests to the server, with its Basic credentials.
struct Api {
    agent: ureq::Agent,
    stream_agent: ureq::Agent,
    base: String,
    auth: String,
}

impl Api {
    fn new(base: &str, password: &str) -> Self {
        let config = |timeout| {
            ureq::Agent::config_builder()
                .timeout_global(timeout)
                .http_status_as_error(false)
                .build()
                .into()
        };
        let credentials = base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"));
        Api {
            agent: config(Some(Duration::from_secs(30))),
            stream_agent: config(None),
            base: base.trim_end_matches('/').to_string(),
            auth: format!("Basic {credentials}"),
        }
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        let response = self
            .agent
            .get(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .call();
        read(path, response)
    }

    fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let response = self
            .agent
            .post(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .send_json(body);
        read(path, response)
    }

    fn patch(&self, path: &str, body: &Value) -> Result<Value, String> {
        let response = self
            .agent
            .patch(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .send_json(body);
        read(path, response)
    }

    fn stream(&self, path: &str) -> Result<impl Read + Send + 'static, String> {
        let response = self
            .stream_agent
            .get(format!("{}{path}", self.base))
            .header("Authorization", &self.auth)
            .call()
            .map_err(|e| format!("OpenCode's event stream failed: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("OpenCode's event stream answered {}", response.status()));
        }
        Ok(response.into_body().into_reader())
    }
}

/// The JSON body of a reply, or the server's message when it refused the request.
fn read(
    path: &str,
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<Value, String> {
    let mut response = response.map_err(|e| format!("OpenCode request {path} failed: {e}"))?;
    let status = response.status();
    let text = response.body_mut().read_to_string().unwrap_or_default();
    let body = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(body);
    }
    let message = body["message"].as_str().map_or(text.clone(), String::from);
    Err(format!("OpenCode refused {path} ({status}): {message}"))
}
