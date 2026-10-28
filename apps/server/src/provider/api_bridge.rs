//! Patchwork's own agent, for API providers: it runs one turn against an OpenAI-compatible Chat
//! Completions endpoint, calling the tools in `api_tools` until the model answers. Patchwork
//! starts it as the turn's process: the first stdin line is the turn (with the key, which so
//! stays out of the arguments and environment), later lines answer approvals and questions or
//! steer the turn, and stdout carries `api.*` events as JSON lines. Each session's
//! conversation is a JSON file in the sessions folder.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::api_tools;
use crate::permissions::Target;
use crate::types::{AccessMode, PermissionKind};

/// The argument that starts Patchwork's own binary as the API agent.
pub const BRIDGE_ARG: &str = "--api-bridge";

/// Model requests a turn may make, so a model that never stops calling tools ends.
const MAX_STEPS: usize = 400;
/// Attempts at a request that failed for a reason worth retrying.
const MAX_ATTEMPTS: u32 = 4;
/// Longest project instructions file read into the system prompt.
const MAX_INSTRUCTIONS: usize = 20_000;
/// The longest gap between two lines of a streamed reply before it counts as stalled. Endpoints
/// that queue or think send keep-alive comments, which count as lines.
const STREAM_IDLE: Duration = Duration::from_secs(180);
/// Tool results kept whole each time a conversation is cut to fit: older ones become a note.
const KEEP_RESULTS: [usize; 3] = [8, 2, 0];
const LEFT_OUT: &str = "(Earlier messages were left out to fit the context window.)";
/// Phrases of the errors endpoints answer a too-long request with: OpenAI, DeepSeek, vLLM, and
/// xAI ("maximum context/prompt length"), Groq (`context_length_exceeded`, "reduce the length"),
/// Mistral and llama.cpp ("context size", "maximum context"), Fireworks ("prompt is too long"),
/// LM Studio ("context the overflows"), TGI-based hosts ("`max_new_tokens` must be"), and
/// OpenRouter, which can wrap them in `metadata.raw`.
const CONTEXT_PHRASES: [&str; 9] = [
    "max_new_tokens",
    "context length",
    "context_length",
    "context size",
    "context window",
    "maximum context",
    "prompt length",
    "reduce the length",
    "too long",
];
const DENIED: &str = "The user denied this. Do not retry it; ask what they would like instead, or carry on another way.";
const REMOVED_RESULT: &str = "(This output was removed to fit the context window. Run the tool again if it is still needed.)";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    base_url: String,
    api_key: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    model: String,
    /// The provider's name, for messages.
    provider: String,
    root: String,
    sessions_dir: String,
    session_id: Option<String>,
    text: String,
    #[serde(default)]
    images: Vec<ImageIn>,
    /// Whether `model` reads images. When it does not, the images in the conversation are left
    /// out of the request, and the conversation keeps them for a turn that can read them.
    images_ok: bool,
    mode: AccessMode,
    plan: bool,
    effort: Option<String>,
    /// Patchwork's instructions about the user's shell.
    #[serde(default)]
    instructions: Option<String>,
    /// `compact` summarizes the conversation instead of sending a message.
    #[serde(default)]
    command: Option<String>,
    /// Tools of the MCP servers Patchwork runs, which it calls for the agent.
    #[serde(default)]
    mcp_tools: Vec<McpTool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpTool {
    name: String,
    server: String,
    tool: String,
    description: String,
    parameters: Value,
    read_only: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImageIn {
    media_type: String,
    data: String,
}

/// Runs the agent; the process exits with what this returns.
pub fn run() -> i32 {
    let mut first = String::new();
    let mut stdin = BufReader::new(std::io::stdin());
    if stdin.read_line(&mut first).unwrap_or(0) == 0 {
        emit(&json!({"type": "api.error", "message": "The API agent got no turn"}));
        return 1;
    }
    // Patchwork sends only an error when the turn cannot run, e.g. its provider is gone.
    if let Ok(v) = serde_json::from_str::<Value>(&first)
        && let Some(message) = v["error"].as_str()
    {
        emit(&json!({"type": "api.error", "message": message}));
        return 1;
    }
    let request: Request = match serde_json::from_str(&first) {
        Ok(request) => request,
        Err(e) => {
            emit(
                &json!({"type": "api.error", "message": format!("Bad turn for the API agent: {e}")}),
            );
            return 1;
        }
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in stdin.lines() {
            let Ok(line) = line else { break };
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if tx.send(v).is_err() {
                break;
            }
        }
    });
    let mut agent = Agent::new(request, rx);
    match agent.turn() {
        Ok(()) => 0,
        Err(message) => {
            emit(&json!({"type": "api.error", "message": message}));
            1
        }
    }
}

struct Agent {
    request: Request,
    root: PathBuf,
    session_id: String,
    session_file: PathBuf,
    messages: Vec<Value>,
    input: Receiver<Value>,
    /// Steering messages that came in while the agent was busy.
    steers: Vec<Value>,
    /// Images `read_file` opened this step. A tool result is text only, so they follow the
    /// step's results as a message of their own.
    read_images: Vec<ImageIn>,
    /// Stdin closed: Patchwork is ending the turn.
    closed: bool,
    http: ureq::Agent,
    tools: Value,
    /// Built once per turn, so every request starts the same and the provider's prompt cache
    /// keeps matching, even if the project's instructions file changes during the turn.
    system: String,
}

/// Why a request failed: some failures are fixed by making the conversation smaller.
enum Failure {
    /// The conversation is longer than the model's context window.
    Context(String),
    /// The model or endpoint does not accept images.
    Images(String),
    Other(String),
}

impl From<Failure> for String {
    fn from(failure: Failure) -> String {
        match failure {
            Failure::Context(m) | Failure::Images(m) | Failure::Other(m) => m,
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure::Other(message)
    }
}

/// The request's messages: the system prompt, then the conversation, written straight from the
/// history without copying it. Reasoning goes back only for the current turn, as endpoints that
/// return `reasoning_content` ask; earlier turns drop it.
struct Messages<'a> {
    system: &'a str,
    history: &'a [Value],
    last_user: usize,
}

impl Serialize for Messages<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.history.len() + 1))?;
        seq.serialize_element(&json!({"role": "system", "content": self.system}))?;
        for (i, m) in self.history.iter().enumerate() {
            if i < self.last_user && m.get("reasoning_content").is_some() {
                let mut m = m.clone();
                if let Some(obj) = m.as_object_mut() {
                    obj.remove("reasoning_content");
                }
                seq.serialize_element(&m)?;
            } else {
                seq.serialize_element(m)?;
            }
        }
        seq.end()
    }
}

#[derive(Serialize)]
struct Body<'a> {
    model: &'a str,
    messages: Messages<'a>,
    stream: bool,
    stream_options: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
}

/// One reply of the model.
#[derive(Default)]
struct Reply {
    content: String,
    reasoning: String,
    /// The endpoint sent its reasoning as `reasoning_content`, which it wants back.
    reasoning_content: bool,
    calls: Vec<Call>,
}

#[derive(Default, Clone)]
struct Call {
    id: String,
    name: String,
    arguments: String,
}

impl Agent {
    fn new(request: Request, input: Receiver<Value>) -> Self {
        let root = PathBuf::from(&request.root);
        let (session_id, existing) = match &request.session_id {
            Some(id) => (id.clone(), true),
            None => (uuid::Uuid::new_v4().to_string(), false),
        };
        let session_file = Path::new(&request.sessions_dir).join(format!("{session_id}.json"));
        let messages = if existing {
            load(&session_file).unwrap_or_default()
        } else {
            Vec::new()
        };
        let http = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            // A reasoning model can think for minutes before its first token. The body has no
            // overall limit; `stream` stops a reply whose lines stop coming.
            .timeout_recv_response(Some(Duration::from_secs(600)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut tools = api_tools::definitions(request.plan);
        if let Some(list) = tools.as_array_mut() {
            list.extend(request.mcp_tools.iter().map(|t| {
                json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}})
            }));
        }
        let system = system_prompt(&request, &root);
        Agent {
            request,
            root,
            session_id,
            session_file,
            messages,
            input,
            steers: Vec::new(),
            read_images: Vec::new(),
            closed: false,
            http,
            tools,
            system,
        }
    }

    fn turn(&mut self) -> Result<(), String> {
        if self.request.session_id.is_some() && !self.session_file.is_file() {
            emit(&json!({"type": "api.session_lost"}));
            return Ok(());
        }
        if self.request.session_id.is_none() {
            emit(&json!({"type": "api.session", "id": self.session_id}));
        }
        emit(&json!({"type": "api.commands", "commands": [
            {"name": "compact", "description": "Summarize the conversation to free up context"}
        ]}));
        if self.request.command.as_deref() == Some("compact") {
            return self.compact();
        }
        let content = self.user_content(&self.request.text, &self.request.images);
        self.messages
            .push(json!({"role": "user", "content": content}));
        self.save()?;

        for _ in 0..MAX_STEPS {
            let reply = match self.complete(true) {
                Ok(reply) => reply,
                Err(failure) => self.recover(failure)?,
            };
            let mut message = json!({"role": "assistant", "content": reply.content});
            if reply.reasoning_content && !reply.reasoning.is_empty() {
                message["reasoning_content"] = json!(reply.reasoning);
            }
            if !reply.calls.is_empty() {
                let calls: Vec<Value> = reply
                    .calls
                    .iter()
                    .map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.arguments}}))
                    .collect();
                message["tool_calls"] = json!(calls);
            }
            self.messages.push(message);
            if reply.calls.is_empty() {
                self.save()?;
            }

            // Saved once the step's calls are done: a turn stopped midway leaves the file at
            // the previous step, which the model can continue from.
            let mut proposed = false;
            for call in &reply.calls {
                let (result, stop) = self.call(call);
                proposed |= stop;
                self.messages
                    .push(json!({"role": "tool", "tool_call_id": call.id, "content": result}));
            }
            if !self.read_images.is_empty() {
                let images = std::mem::take(&mut self.read_images);
                let content = self.user_content("The images read_file opened:", &images);
                self.messages
                    .push(json!({"role": "user", "content": content}));
            }
            if !reply.calls.is_empty() {
                self.save()?;
            }
            self.take_input();
            if self.closed {
                return Ok(());
            }
            let steered = !self.steers.is_empty();
            for steer in std::mem::take(&mut self.steers) {
                let images: Vec<ImageIn> = steer["images"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|i| {
                        Some(ImageIn {
                            media_type: i["mediaType"].as_str()?.into(),
                            data: i["data"].as_str()?.into(),
                        })
                    })
                    .collect();
                let text = steer["text"].as_str().unwrap_or_default();
                let content = self.user_content(text, &images);
                self.messages
                    .push(json!({"role": "user", "content": content}));
                emit(&json!({"type": "api.steered"}));
            }
            if steered {
                self.save()?;
                continue;
            }
            if proposed || reply.calls.is_empty() {
                return Ok(());
            }
        }
        Err(format!(
            "Stopped after {MAX_STEPS} model requests in one turn"
        ))
    }

    /// Summarizes the conversation and keeps only the summary.
    fn compact(&mut self) -> Result<(), String> {
        if self.messages.is_empty() {
            emit(&json!({"type": "api.text", "delta": "There is nothing to compact yet."}));
            return Ok(());
        }
        self.summarize(false)?;
        self.save()
    }

    /// Replaces the conversation with the model's summary of it. When even the request for the
    /// summary does not fit, the oldest messages are left out of it until it does. With `resume`,
    /// the user's latest message follows the summary word for word, as the request to answer.
    fn summarize(&mut self, resume: bool) -> Result<(), String> {
        let latest = resume
            .then(|| self.messages.iter().rev().find(|m| m["role"] == "user"))
            .flatten()
            .and_then(|m| m["content"].as_str())
            .map(str::to_string);
        let reply = loop {
            self.messages
                .push(json!({"role": "user", "content": COMPACT_PROMPT}));
            let reply = self.complete(false);
            self.messages.pop();
            match reply {
                Err(Failure::Context(_)) if self.drop_oldest() => continue,
                reply => break reply?,
            }
        };
        if reply.content.trim().is_empty() {
            return Err("The model returned an empty summary, so the conversation was kept".into());
        }
        let mut summary = format!(
            "This conversation was compacted. A summary of it so far:\n\n{}",
            reply.content.trim()
        );
        if let Some(latest) = latest {
            summary.push_str(&format!(
                "\n\nThe user's latest message, which the work above was answering:\n\n{latest}"
            ));
        }
        self.messages = vec![json!({"role": "user", "content": summary})];
        Ok(())
    }

    /// Retries a request the provider refused for the conversation's size or its images, after
    /// making it fit: images out, then old tool output cut, then the whole thing summarized.
    fn recover(&mut self, mut failure: Failure) -> Result<Reply, String> {
        let (mut imageless, mut stage, mut summarized) = (false, 0, false);
        loop {
            match failure {
                Failure::Images(_) if !imageless && self.drop_images() => imageless = true,
                Failure::Context(_) if stage < KEEP_RESULTS.len() => {
                    // A stage that cut nothing is skipped without a request.
                    let cut = self.trim_results(KEEP_RESULTS[stage]);
                    stage += 1;
                    if !cut {
                        continue;
                    }
                }
                Failure::Context(_) if !summarized => {
                    emit(
                        &json!({"type": "api.retry", "reason": "The conversation is too long for the model, so it is being summarized", "attempt": 1, "max": 1}),
                    );
                    self.summarize(true)?;
                    summarized = true;
                }
                failure => return Err(failure.into()),
            }
            self.save()?;
            match self.complete(true) {
                Ok(reply) => return Ok(reply),
                Err(next) => failure = next,
            }
        }
    }

    /// Cuts every tool result but the last `keep` to a note; returns whether any was cut.
    fn trim_results(&mut self, keep: usize) -> bool {
        let results: Vec<usize> = (0..self.messages.len())
            .filter(|&i| self.messages[i]["role"] == "tool")
            .collect();
        let mut cut = false;
        for &i in results.iter().rev().skip(keep) {
            let content = &mut self.messages[i]["content"];
            if content
                .as_str()
                .is_some_and(|c| c.len() > REMOVED_RESULT.len())
            {
                *content = json!(REMOVED_RESULT);
                cut = true;
            }
        }
        cut
    }

    /// Leaves out the older half of the conversation, starting it again at a user message where
    /// one follows, and never at a tool result whose call was left out; returns whether any was.
    fn drop_oldest(&mut self) -> bool {
        let len = self.messages.len();
        if len < 2 {
            return false;
        }
        let half = len / 2;
        let at = |role: &str| (half..len).find(|&i| self.messages[i]["role"] == role);
        let start = at("user")
            .or_else(|| (half..len).find(|&i| self.messages[i]["role"] != "tool"))
            .unwrap_or(len - 1);
        self.messages.drain(..start);
        if self.messages.first().is_some_and(|m| m["role"] != "user") {
            self.messages
                .insert(0, json!({"role": "user", "content": LEFT_OUT}));
        }
        true
    }

    /// Takes the images out of the conversation, noting where they were; returns whether any
    /// were there.
    fn drop_images(&mut self) -> bool {
        let mut dropped = false;
        let provider = &self.request.provider;
        for m in &mut self.messages {
            dropped |= strip_images(m, provider) > 0;
        }
        dropped
    }

    /// The conversation as a model that cannot read images gets it: the images left out.
    fn without_images(&self) -> Vec<Value> {
        let mut messages = self.messages.clone();
        for m in &mut messages {
            strip_images(m, &self.request.provider);
        }
        messages
    }

    /// A user message's content: plain text, or text and images. A model that cannot read the
    /// images refuses them, and `recover` sends the message again without them.
    fn user_content(&self, text: &str, images: &[ImageIn]) -> Value {
        if images.is_empty() {
            return json!(text);
        }
        let mut parts = vec![json!({"type": "text", "text": text})];
        parts.extend(images.iter().map(|i| {
            json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", i.media_type, i.data)}})
        }));
        Value::Array(parts)
    }

    /// Runs one tool call and returns its result for the model, and whether the turn ends here.
    fn call(&mut self, call: &Call) -> (String, bool) {
        let args: Value = match serde_json::from_str(if call.arguments.trim().is_empty() {
            "{}"
        } else {
            &call.arguments
        }) {
            Ok(args) => args,
            Err(e) => {
                let message =
                    format!("The arguments were not valid JSON ({e}); call the tool again");
                emit(&json!({"type": "api.tool", "id": call.id, "name": call.name, "summary": ""}));
                emit(
                    &json!({"type": "api.tool_done", "id": call.id, "error": true, "output": message}),
                );
                return (message, false);
            }
        };
        let summary = api_tools::summary(&call.name, &args, &self.root);
        emit(&json!({"type": "api.tool", "id": call.id, "name": call.name, "summary": summary}));
        let mcp = self
            .request
            .mcp_tools
            .iter()
            .position(|t| t.name == call.name);
        let (result, error, stop) = if let Some(index) = mcp {
            let result = self.call_mcp(index, &call.id, &args);
            let error = result.is_err();
            (result, error, false)
        } else {
            match call.name.as_str() {
                "update_plan" => {
                    emit(&json!({"type": "api.todos", "steps": args["steps"]}));
                    (
                        Ok("The plan is shown to the user.".to_string()),
                        false,
                        false,
                    )
                }
                "propose_plan" => {
                    let plan = args["plan"].as_str().unwrap_or_default();
                    emit(&json!({"type": "api.plan", "text": plan}));
                    (
                        Ok("The plan was shown to the user, who will reply.".to_string()),
                        false,
                        true,
                    )
                }
                "ask_user" => match self.ask(&call.id, &args) {
                    Ok(answer) => (Ok(answer), false, false),
                    Err(e) => (Err(e), true, false),
                },
                name => {
                    let allowed =
                        match api_tools::approval(name, &args, self.request.mode, &self.root) {
                            Some((title, detail, target, asks)) => {
                                self.approve(&call.id, &title, &detail, target, asks)
                            }
                            None => true,
                        };
                    if allowed
                        && name == "read_file"
                        && let Some(image) = self.read_image(&args)
                    {
                        let error = image.is_err();
                        (image, error, false)
                    } else if allowed {
                        let result = api_tools::run(name, &args, &self.root);
                        let error = result.is_err();
                        (result, error, false)
                    } else {
                        (Err(DENIED.into()), true, false)
                    }
                }
            }
        };
        let text = match &result {
            Ok(text) | Err(text) => text.clone(),
        };
        let mut done =
            json!({"type": "api.tool_done", "id": call.id, "error": error, "output": text});
        if !error
            && api_tools::writes(&call.name)
            && let Some(path) = args["path"].as_str()
        {
            done["path"] = json!(api_tools::shown(
                &api_tools::resolve(&self.root, path),
                &self.root
            ));
        }
        emit(&done);
        (text, stop)
    }

    /// Opens the image `read_file` was pointed at for the model to see, or None when the path is
    /// not an image.
    fn read_image(&mut self, args: &Value) -> Option<Result<String, String>> {
        let path = api_tools::resolve(&self.root, args["path"].as_str()?);
        let path = path.to_str()?;
        crate::types::image_media_type(path)?;
        let image = crate::types::Attachment::from_path(path)
            .and_then(|a| super::load_images(&[a]))
            .map(|mut images| images.remove(0));
        Some(image.map(|image| {
            self.read_images.push(ImageIn {
                media_type: image.media_type.into(),
                data: image.data,
            });
            format!("{path} is an image; it follows the tool results.")
        }))
    }

    /// Has Patchwork call an MCP tool, once the user approves where the access mode asks and the
    /// permission rules allow it.
    fn call_mcp(&mut self, index: usize, id: &str, args: &Value) -> Result<String, String> {
        let tool = &self.request.mcp_tools[index];
        let asks = match self.request.mode {
            AccessMode::FullAccess => false,
            AccessMode::Supervised => true,
            _ => !tool.read_only,
        };
        let (server, name) = (tool.server.clone(), tool.tool.clone());
        if !self.approve(
            id,
            &format!("Use {name} from {server}"),
            &args.to_string(),
            Target::new(PermissionKind::Mcp, [format!("{server}/{name}")]),
            asks,
        ) {
            return Err(DENIED.into());
        }
        emit(
            &json!({"type": "api.mcp_call", "id": id, "server": server, "tool": name, "arguments": args}),
        );
        let reply = self
            .wait(id)
            .ok_or("The turn ended before the tool finished")?;
        let output = api_tools::cap(reply["output"].as_str().unwrap_or_default().to_string());
        if reply["error"] == true {
            Err(output)
        } else {
            Ok(output)
        }
    }

    /// Asks Patchwork for an approval, or with `asks` false for the permission rules' answer to a
    /// call the access mode runs without asking, and waits for the answer.
    fn approve(&mut self, id: &str, title: &str, detail: &str, target: Target, asks: bool) -> bool {
        let Target { kind, resources } = target;
        emit(&json!({
            "type": if asks { "api.approval" } else { "api.check" }, "id": id, "title": title, "detail": detail,
            "kind": kind, "resources": resources,
        }));
        self.wait(id).is_some_and(|reply| reply["allow"] == true)
    }

    fn ask(&mut self, id: &str, args: &Value) -> Result<String, String> {
        let questions: Vec<Value> = args["questions"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, q)| {
                json!({
                    "id": format!("q{i}"),
                    "header": q["header"].as_str().unwrap_or_default(),
                    "question": q["question"].as_str().unwrap_or_default(),
                    "options": q["options"],
                    "multiSelect": q["multi_select"] == true,
                })
            })
            .collect();
        if questions.is_empty() {
            return Err("Give at least one question".into());
        }
        emit(&json!({"type": "api.question", "id": id, "questions": questions}));
        let reply = self
            .wait(id)
            .ok_or("The turn ended before the user answered")?;
        if reply["allow"] != true {
            return Err(
                "The user dismissed the questions. Carry on with your best judgment.".into(),
            );
        }
        let lines: Vec<String> = questions
            .iter()
            .map(|q| {
                let qid = q["id"].as_str().unwrap_or_default();
                let answer: Vec<&str> = reply["answers"][qid]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let answer = if answer.is_empty() {
                    "(no answer)".into()
                } else {
                    answer.join(", ")
                };
                format!("{}: {answer}", q["question"].as_str().unwrap_or_default())
            })
            .collect();
        Ok(format!("The user answered:\n{}", lines.join("\n")))
    }

    /// Waits for the reply to the request `id`, keeping steering messages that come meanwhile.
    fn wait(&mut self, id: &str) -> Option<Value> {
        while let Ok(v) = self.input.recv() {
            match v["type"].as_str() {
                Some("reply") if v["id"] == id => return Some(v),
                Some("steer") => self.steers.push(v),
                _ => {}
            }
        }
        self.closed = true;
        None
    }

    /// Takes the steering messages that have arrived, without waiting.
    fn take_input(&mut self) {
        loop {
            match self.input.try_recv() {
                Ok(v) if v["type"] == "steer" => self.steers.push(v),
                Ok(_) => {}
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.closed = true;
                    break;
                }
            }
        }
    }

    fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string(&json!({
            "model": self.request.model,
            "messages": self.messages,
        }))
        .map_err(|e| e.to_string())?;
        super::api_config::write_private(&self.session_file, &text)
    }

    /// Sends the conversation and streams the reply, retrying a request that failed before
    /// any of it arrived.
    fn complete(&self, with_tools: bool) -> Result<Reply, Failure> {
        let history: Cow<[Value]> = if self.request.images_ok {
            Cow::Borrowed(&self.messages[..])
        } else {
            Cow::Owned(self.without_images())
        };
        let body = Body {
            model: &self.request.model,
            messages: Messages {
                system: &self.system,
                history: &history,
                last_user: self
                    .messages
                    .iter()
                    .rposition(|m| m["role"] == "user")
                    .unwrap_or(0),
            },
            stream: true,
            stream_options: json!({"include_usage": true}),
            tools: with_tools.then_some(&self.tools),
            reasoning_effort: self.request.effort.as_deref(),
        };
        let body = serde_json::to_string(&body).map_err(|e| e.to_string())?;
        let has_images = self.messages.iter().any(|m| {
            m["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|p| p["type"] == "image_url"))
        });
        let url = format!(
            "{}/chat/completions",
            self.request.base_url.trim_end_matches('/')
        );
        let mut attempt = 1;
        loop {
            let mut request = self
                .http
                .post(&url)
                .header("Content-Type", "application/json");
            if let Some(key) = &self.request.api_key {
                request = request.header("Authorization", &format!("Bearer {key}"));
            }
            for (name, value) in &self.request.headers {
                request = request.header(name, value);
            }
            let failure = match request.send(body.as_str()) {
                Ok(response) if response.status().is_success() => {
                    return self
                        .stream(response.into_body().into_reader())
                        .map_err(Failure::Other);
                }
                Ok(mut response) => {
                    let status = response.status().as_u16();
                    let text = response.body_mut().read_to_string().unwrap_or_default();
                    let parsed = serde_json::from_str(&text).unwrap_or(Value::Null);
                    let message = super::api::error_text(status, &parsed, &text);
                    // The whole body, so codes and errors wrapped by a router count too.
                    let lower = text.to_lowercase();
                    let refused = matches!(status, 400 | 413 | 415 | 422);
                    if status == 413
                        || (refused && CONTEXT_PHRASES.iter().any(|p| lower.contains(p)))
                    {
                        return Err(Failure::Context(message));
                    }
                    let about_images = lower.contains("image") || lower.contains("multimodal");
                    if refused && has_images && about_images {
                        return Err(Failure::Images(message));
                    }
                    let retry =
                        matches!(status, 408 | 409 | 425 | 429 | 500 | 502 | 503 | 504 | 529);
                    (message, retry)
                }
                Err(e) => (
                    format!("Could not reach {}: {e}", self.request.provider),
                    true,
                ),
            };
            let (message, retry) = failure;
            if !retry || attempt >= MAX_ATTEMPTS {
                return Err(Failure::Other(message));
            }
            emit(
                &json!({"type": "api.retry", "reason": message, "attempt": attempt, "max": MAX_ATTEMPTS - 1}),
            );
            std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
            attempt += 1;
        }
    }

    /// Reads a server-sent event stream of completion chunks.
    fn stream(&self, body: impl Read + Send + 'static) -> Result<Reply, String> {
        let mut reply = Reply::default();
        let mut calls: Vec<Call> = Vec::new();
        let mut usage = Value::Null;
        // Lines come through a thread so a stalled reply can be given up on.
        let (tx, lines) = mpsc::sync_channel(256);
        std::thread::spawn(move || {
            for line in BufReader::new(body).lines() {
                let end = line.is_err();
                if tx.send(line).is_err() || end {
                    break;
                }
            }
        });
        loop {
            let line = match lines.recv_timeout(STREAM_IDLE) {
                Ok(line) => line,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "{} stopped sending its reply for {} s, so the turn was stopped",
                        self.request.provider,
                        STREAM_IDLE.as_secs()
                    ));
                }
            };
            let line = line
                .map_err(|e| format!("The reply from {} broke off: {e}", self.request.provider))?;
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let Ok(chunk) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if !chunk["error"].is_null() {
                return Err(super::api::error_text(0, &chunk, data));
            }
            if chunk["usage"].is_object() {
                usage = chunk["usage"].clone();
            }
            let Some(choice) = chunk["choices"].get(0) else {
                continue;
            };
            let delta = &choice["delta"];
            if let Some(text) = delta["reasoning_content"]
                .as_str()
                .filter(|t| !t.is_empty())
            {
                reply.reasoning_content = true;
                reply.reasoning.push_str(text);
                emit(&json!({"type": "api.reasoning", "delta": text}));
            } else if let Some(text) = delta["reasoning"].as_str().filter(|t| !t.is_empty()) {
                reply.reasoning.push_str(text);
                emit(&json!({"type": "api.reasoning", "delta": text}));
            }
            if let Some(text) = delta["content"].as_str().filter(|t| !t.is_empty()) {
                reply.content.push_str(text);
                emit(&json!({"type": "api.text", "delta": text}));
            }
            for part in delta["tool_calls"].as_array().into_iter().flatten() {
                // Calls carry an index; without one, a new id starts a new call.
                let index = match part["index"].as_u64() {
                    Some(n) => n as usize,
                    None => match part["id"].as_str() {
                        Some(id) if calls.iter().all(|c| c.id != id) => calls.len(),
                        _ => calls.len().saturating_sub(1),
                    },
                };
                if calls.len() <= index {
                    calls.resize(index + 1, Call::default());
                }
                let call = &mut calls[index];
                if let Some(id) = part["id"].as_str().filter(|id| !id.is_empty()) {
                    call.id = id.into();
                }
                if let Some(name) = part["function"]["name"].as_str() {
                    call.name.push_str(name);
                }
                if let Some(arguments) = part["function"]["arguments"].as_str() {
                    call.arguments.push_str(arguments);
                }
            }
        }
        if !usage.is_null() {
            emit_usage(&usage);
        }
        reply.calls = calls
            .into_iter()
            .filter(|c| !c.name.is_empty())
            .map(|mut c| {
                if c.id.is_empty() {
                    c.id = format!("call_{}", uuid::Uuid::new_v4().simple());
                }
                c
            })
            .collect();
        Ok(reply)
    }
}

/// Replaces the images in a message with a note that they were left out; returns how many.
fn strip_images(message: &mut Value, provider: &str) -> usize {
    let Some(parts) = message["content"].as_array() else {
        return 0;
    };
    let images = parts.iter().filter(|p| p["type"] == "image_url").count();
    if images > 0 {
        let text = parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        message["content"] = json!(format!("{text}\n\n{}", images_note(provider, images)));
    }
    images
}

/// Stands in for images a provider cannot read, so the model can tell the user.
fn images_note(provider: &str, count: usize) -> String {
    let what = if count == 1 {
        "an image, which"
    } else {
        "images, which"
    };
    let it = if count == 1 { "it was" } else { "they were" };
    format!("(The user attached {what} {provider} cannot read, so {it} left out. Tell the user.)")
}

fn system_prompt(request: &Request, root_dir: &Path) -> String {
    let root = &request.root;
    let git = root_dir.join(".git").exists();
    let today = crate::time::format_rfc3339_ms(crate::types::now_ms());
    let today = today.get(..10).unwrap_or(&today);
    let mut prompt = format!(
        "You are a coding agent working in Patchwork, a desktop workspace, on the user's own machine.\n\
         You are working in {root}{}. Today is {today}. The platform is {}.\n\n{GUIDANCE}",
        if git { ", a git repository" } else { "" },
        std::env::consts::OS,
    );
    if let Some(instructions) = &request.instructions {
        prompt.push_str(&format!("\n\n{instructions}"));
    }
    if request.plan {
        prompt.push_str(&format!("\n\n{PLAN_MODE}"));
    } else if crate::computer::enabled() {
        prompt.push_str(&format!("\n\n{COMPUTER_USE}"));
    }
    for name in ["AGENTS.md", "CLAUDE.md"] {
        if let Ok(text) = std::fs::read_to_string(root_dir.join(name)) {
            let text: String = text.chars().take(MAX_INSTRUCTIONS).collect();
            prompt.push_str(&format!(
                "\n\nThe project's instructions, from {name}:\n\n{text}"
            ));
            break;
        }
    }
    prompt
}

const GUIDANCE: &str = "\
Help with software engineering tasks: fixing bugs, adding features, refactoring, explaining code.
- Use the tools to look before you act: read files before you edit them, and search the code instead of guessing.
- Make the smallest change that does what was asked, in the style of the code around it.
- Prefer edit_file for changes to existing files; write_file is for new files or full rewrites.
- Run the project's own build, lint, or test commands with bash when they help you check your work.
- For a task of several steps, keep a checklist with update_plan.
- Ask with ask_user only when a decision is genuinely the user's; otherwise make the sensible choice.
- When a tool is denied, do not retry it; find another way or ask.
- Be brief. Write in Markdown, and say what you changed and anything left undone when you finish.";

const COMPUTER_USE: &str = "\
Computer use is on. computer_screenshot writes a PNG of the screen and returns its path; read it \
with read_file to see the screen. The picture is scaled down to 1440 pixels wide, so multiply a \
position you see by screenWidth divided by the picture's width before clicking. Take a screenshot \
before acting, and again after, because the screen only changes when you act. Prefer a tool over \
the screen whenever one exists. Stop and report when a step needs the user's password.";

const PLAN_MODE: &str = "\
Plan mode is on: do not change anything. Research with the read-only tools, then call propose_plan \
once with a concrete, step-by-step implementation plan in Markdown, and stop.";

const COMPACT_PROMPT: &str = "\
Summarize this conversation so that work can continue from the summary alone. Include the user's \
goals and instructions, decisions made, files read or changed with what was learned or done, \
commands run and their results, and what is left to do. Reply with the summary only.";

fn load(path: &Path) -> Option<Vec<Value>> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v["messages"].as_array().cloned()
}

/// Reports a request's tokens: OpenAI's cached-token details, or DeepSeek's cache hit count.
fn emit_usage(usage: &Value) {
    let count = |v: &Value| v.as_u64().unwrap_or(0);
    let prompt = count(&usage["prompt_tokens"]);
    let cached = usage["prompt_tokens_details"]["cached_tokens"]
        .as_u64()
        .or_else(|| usage["prompt_cache_hit_tokens"].as_u64())
        .unwrap_or(0);
    emit(&json!({
        "type": "api.usage",
        "id": uuid::Uuid::new_v4().to_string(),
        "input": prompt.saturating_sub(cached),
        "cacheRead": cached,
        "output": count(&usage["completion_tokens"]),
        "context": prompt,
    }));
}

fn emit(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}
