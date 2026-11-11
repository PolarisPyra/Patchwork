//! One turn through the bridge: the request it is given, the messages it trades with the server, and the events it turns them into.

// Rust guideline compliant 2026-10-09

use super::server::Api;
use super::server::Server;
use crate::Error;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use serde::Deserialize;
use serde_json::{Value, json};

/// The argument that starts Patchwork's own binary as the bridge.
pub const BRIDGE_ARG: &str = "--opencode-bridge";

/// How long the server may take to print the address it listens on.
pub(super) const START_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the server may take to load the folder's commands.
pub(super) const COMMANDS_TIMEOUT: Duration = Duration::from_secs(3);

/// The turn, as the first stdin line.
#[derive(Debug, Deserialize)]
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
    /// A message that is only `/name` and its arguments.
    #[serde(default)]
    pub command: Option<SlashCommand>,
    /// Compacts the session once a step's context reaches this; 0 leaves it to OpenCode.
    #[serde(default)]
    pub compact_window: u64,
    /// The system prompt the user gave OpenCode in Settings.
    #[serde(default)]
    pub system_prompt: Option<String>,
}

/// The session's instruction entry that holds the user's system prompt.
pub(super) const PROMPT_ENTRY: &str = "patchwork-system-prompt";

#[derive(Debug, Deserialize)]
pub struct SlashCommand {
    pub name: String,
    /// What follows the name, OpenCode's `$ARGUMENTS`.
    pub text: String,
}

/// Runs the bridge; the process exits with what this returns.
#[must_use]
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
            emit(
                &json!({"type": "bridge.error", "message": format!("Bad turn for OpenCode: {e}")}),
            );
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

pub(super) enum Msg {
    /// A line Patchwork wrote to the bridge's stdin.
    Input(String),
    /// The server took a queued message into the turn.
    Delivered(String),
    /// The session's run ended.
    Ended,
    /// A compaction the bridge asked for started (true), or ended or failed (false).
    Compaction(bool),
    /// The event stream closed.
    StreamClosed,
}

pub(super) fn turn(request: &Request, tx: &Sender<Msg>, rx: &Receiver<Msg>) -> Result<(), Error> {
    let server = Server::start(request)?;
    let api = Api::new(&server.url, &server.password);
    let session_id = match &request.session_id {
        Some(id) if api.get(&format!("/api/session/{id}")).is_ok() => {
            if let Some(model) = model_ref(request) {
                api.post(
                    &format!("/api/session/{id}/model"),
                    &json!({ "model": model }),
                )?;
            }
            api.post(
                &format!("/api/session/{id}/agent"),
                &json!({ "agent": request.agent }),
            )?;
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
    // The system prompt rides on the session as an instruction entry, which OpenCode adds to the
    // model's instructions; a cleared prompt is taken off a resumed session. The API is marked
    // experimental, so a server without it runs the turn without the prompt, and says so on
    // stderr, which goes to the agents' log.
    let entry =
        format!("/api/experimental/session/{session_id}/instructions/entries/{PROMPT_ENTRY}");
    match &request.system_prompt {
        Some(text) => {
            if let Err(e) = api.put(&entry, &json!({ "value": text })) {
                let _ = writeln!(
                    std::io::stderr(),
                    "OpenCode did not take the system prompt: {e}"
                );
            }
        }
        None => {
            let _ = api.delete(&entry);
        }
    }
    // Subscribed before the prompt, so no event of the turn is missed.
    events(&api, &session_id, request.compact_window, tx.clone())?;
    send(&api, &session_id, request)?;

    // Steering messages sent but not yet taken into the turn; the turn ends once none wait, and
    // once a compaction it asked for is done, since the server stops with the turn.
    let mut steers = HashSet::new();
    let (mut compacting, mut ended) = (false, false);
    for msg in rx {
        match msg {
            Msg::Input(line) => command(&api, &session_id, &line, &mut steers),
            Msg::Delivered(id) => {
                if steers.remove(&id) {
                    // The run goes on with it, so an end seen before is over.
                    ended = false;
                    emit(&json!({"type": "bridge.steered"}));
                }
            }
            Msg::Compaction(on) => {
                compacting = on;
                if !on && ended && steers.is_empty() {
                    return Ok(());
                }
            }
            Msg::Ended if steers.is_empty() && !compacting => return Ok(()),
            Msg::Ended => ended = true,
            Msg::StreamClosed => return Err("OpenCode's event stream closed".into()),
        }
    }
    Ok(())
}

/// Starts the turn: `/compact` compacts the session, a command the folder has runs as that
/// command, and any other message is a prompt.
pub(super) fn send(api: &Api, session_id: &str, request: &Request) -> Result<(), Error> {
    // Only a command waits for the list; any other turn lists the commands alongside.
    let known = match &request.command {
        Some(command) if command.name != "compact" => commands(api),
        _ => {
            let api = api.clone();
            std::thread::spawn(move || commands(&api));
            Vec::new()
        }
    };
    let (route, body) = match &request.command {
        Some(command) if command.name == "compact" => ("compact", json!({})),
        Some(command) if known.contains(&command.name) => (
            "command",
            json!({ "name": command.name, "text": command.text }),
        ),
        _ => ("prompt", prompt_body(&request.text, &request.files)),
    };
    api.post(&format!("/api/session/{session_id}/{route}"), &body)
        .map(drop)
}

/// Lists the folder's commands for Patchwork's slash menu, and returns their names.
pub(super) fn commands(api: &Api) -> Vec<String> {
    let started = std::time::Instant::now();
    // A fresh server lists none until it has loaded the folder.
    let list = loop {
        let Ok(listed) = api.get("/api/command") else {
            return Vec::new();
        };
        let list = listed["data"].as_array().cloned().unwrap_or_default();
        if !list.is_empty() || started.elapsed() > COMMANDS_TIMEOUT {
            break list;
        }
        // OpenCode fills its command list soon after it starts; poll often until it does.
        std::thread::sleep(Duration::from_millis(100));
    };
    emit(&json!({"type": "bridge.commands", "commands": list}));
    list.iter()
        .filter_map(|c| c["name"].as_str().map(String::from))
        .collect()
}

/// Acts on one line from Patchwork: an approval or question reply, a steering message, or a stop.
pub(super) fn command(api: &Api, session_id: &str, line: &str, steers: &mut HashSet<String>) {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return;
    };
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
            .post(
                &format!("/api/session/{session_id}/question/{request}/reject"),
                &json!({}),
            )
            .map(drop),
        Some("form") => api
            .post(
                &format!("/api/session/{session_id}/form/{request}/reply"),
                &json!({ "answer": v["answer"] }),
            )
            .map(drop),
        Some("form_cancel") => api
            .delete(&format!("/api/session/{session_id}/form/{request}"))
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
        log_error(&message.to_string());
    }
}

/// `{text, files, delivery}`: steering delivers a message into the running turn.
pub(super) fn prompt_body(text: &str, files: &[String]) -> Value {
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
pub(super) fn model_ref(request: &Request) -> Option<Value> {
    let (provider, model) = request.model.as_deref()?.split_once('/')?;
    let mut model = json!({ "providerID": provider, "id": model });
    if let Some(effort) = &request.effort {
        model["variant"] = json!(effort);
    }
    Some(model)
}

/// Streams the server's events in a thread: the session's own go to stdout, with each tool
/// event carrying the tool's name and input, which OpenCode sends only once. A step whose
/// context reached `compact_window` has the session compacted at its next safe point.
pub(super) fn events(
    api: &Api,
    session_id: &str,
    compact_window: u64,
    tx: Sender<Msg>,
) -> Result<(), Error> {
    let response = api.stream("/api/event")?;
    let session_id = session_id.to_string();
    let api = api.clone();
    std::thread::spawn(move || {
        let mut tools: HashMap<String, (String, Value)> = HashMap::new();
        let mut compacting = false;
        for line in BufReader::new(response).lines() {
            let Ok(line) = line else { break };
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let Ok(mut event) = serde_json::from_str::<Value>(data.trim()) else {
                continue;
            };
            // A form names its session inside the form.
            let owner = event["data"]["sessionID"]
                .as_str()
                .or_else(|| event["data"]["form"]["sessionID"].as_str());
            if owner != Some(&session_id) {
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
                "session.step.ended" if compact_window > 0 && !compacting => {
                    let tokens = &event["data"]["tokens"];
                    let count = |v: &Value| v.as_u64().unwrap_or(0);
                    let context = count(&tokens["input"])
                        + count(&tokens["cache"]["read"])
                        + count(&tokens["cache"]["write"])
                        + count(&tokens["output"])
                        + count(&tokens["reasoning"]);
                    if context >= compact_window {
                        match api.post(&format!("/api/session/{session_id}/compact"), &json!({})) {
                            Ok(_) => {
                                compacting = true;
                                let _ = tx.send(Msg::Compaction(true));
                            }
                            Err(message) => log_error(&message.to_string()),
                        }
                    }
                }
                "session.compaction.ended" | "session.compaction.failed" if compacting => {
                    compacting = false;
                    let _ = tx.send(Msg::Compaction(false));
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

pub(super) fn emit(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

pub(super) fn log_error(message: &str) {
    let _ = writeln!(std::io::stderr(), "opencode bridge: {message}");
}
