//! Patchwork's own agent, for API providers: it runs one turn against an OpenAI-compatible Chat
//! Completions endpoint, calling the tools in `tools` until the model answers. Patchwork
//! starts it as the turn's process: the first stdin line is the turn (with the key, which so
//! stays out of the arguments and environment), later lines answer approvals and questions or
//! steer the turn, and stdout carries `api.*` events as JSON lines. Each session's
//! conversation is a JSON file in the sessions folder.

mod calls;
mod context;
mod prompt;
mod request;
mod vision;

use context::CONTEXT_PHRASES;
use prompt::system_prompt;
use request::{Failure, Reply, Call};

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::plugins::AgentHook;
use crate::threads::AccessMode;

/// The argument that starts Patchwork's own binary as the API agent.
pub const BRIDGE_ARG: &str = "--api-bridge";

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
    /// The model that reads images for `model`: it describes each image the user sends, and
    /// `model` gets the description in its place.
    #[serde(default)]
    vision: Option<Vision>,
    mode: AccessMode,
    plan: bool,
    effort: Option<String>,
    /// Summarizes the conversation before a request once the last one's context reached this; 0
    /// leaves it until the provider refuses the conversation as too long.
    #[serde(default)]
    compact_window: u64,
    /// Patchwork's instructions about the user's shell.
    #[serde(default)]
    instructions: Option<String>,
    /// The system prompt the user gave the provider in Settings.
    #[serde(default)]
    system_prompt: String,
    /// `compact` summarizes the conversation instead of sending a message.
    #[serde(default)]
    command: Option<String>,
    /// Tools of the MCP servers Patchwork runs, which it calls for the agent.
    #[serde(default)]
    mcp_tools: Vec<McpTool>,
    /// The plugins' tool hooks, which the agent runs around each call.
    #[serde(default)]
    hooks: Vec<AgentHook>,
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

/// The connection to the vision model.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Vision {
    base_url: String,
    api_key: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    model: String,
    provider: String,
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
    /// Tokens of the last request and its reply, which the next request starts from; saved with
    /// the conversation, so a turn knows where the last one left off.
    context: u64,
    /// Replies stream to Patchwork unless set, as while summarizing on the agent's own.
    quiet: bool,
    /// The model's context window as the endpoint reports it; None until asked, once a turn.
    model_window: Option<Option<u64>>,
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

impl Agent {
    fn new(request: Request, input: Receiver<Value>) -> Self {
        let root = PathBuf::from(&request.root);
        let (session_id, existing) = match &request.session_id {
            Some(id) => (id.clone(), true),
            None => (uuid::Uuid::new_v4().to_string(), false),
        };
        let session_file = Path::new(&request.sessions_dir).join(format!("{session_id}.json"));
        let (messages, context) = if existing {
            load(&session_file).unwrap_or_default()
        } else {
            (Vec::new(), 0)
        };
        let http = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            // A reasoning model can think for minutes before its first token. The body has no
            // overall limit; `stream` stops a reply whose lines stop coming.
            .timeout_recv_response(Some(Duration::from_secs(600)))
            .http_status_as_error(false)
            .build()
            .into();
        let mut tools = crate::provider::api::tools::definitions(request.plan);
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
            context,
            quiet: false,
            model_window: None,
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

        loop {
            self.compact_if_full()?;
            let reply = match self.complete(true) {
                Ok(reply) => reply,
                Err(failure) => self.recover(failure)?,
            };
            if reply.context > 0 {
                self.context = reply.context;
            }
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
    }

    /// A user message's content: plain text, or text and images. With a vision model, the
    /// images are its description of them. A model that cannot read the images refuses them,
    /// and `recover` sends the message again without them.
    fn user_content(&self, text: &str, images: &[ImageIn]) -> Value {
        if images.is_empty() {
            return json!(text);
        }
        if let Some(vision) = &self.request.vision {
            return json!(self.described(vision, text, images));
        }
        let mut parts = vec![json!({"type": "text", "text": text})];
        parts.extend(images.iter().map(|i| {
            json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", i.media_type, i.data)}})
        }));
        Value::Array(parts)
    }

    /// Takes the steering messages that have arrived, without waiting.
    fn take_input(&mut self) {
        loop {
            match self.input.try_recv() {
                Ok(v) if v["type"] == "steer" => self.steers.push(v),
                Ok(v) if v["type"] == "compactWindow" => {
                    self.request.compact_window = v["window"].as_u64().unwrap_or(0);
                }
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
            "context": self.context,
        }))
        .map_err(|e| e.to_string())?;
        crate::system::fs::write_private(&self.session_file, &text)
    }
}

/// A saved conversation and its context; 0 for one saved before the context was.
fn load(path: &Path) -> Option<(Vec<Value>, u64)> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let messages = v["messages"].as_array().cloned()?;
    Some((messages, v["context"].as_u64().unwrap_or(0)))
}

fn emit(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}
