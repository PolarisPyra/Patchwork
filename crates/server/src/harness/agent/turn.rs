//! One turn: what Patchwork sends to start it, the loop that runs it, and the lines it writes.

use super::prompt::system_prompt;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, SystemTime};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::plugins::AgentHook;
use crate::provider::AgentKind;
use crate::threads::AccessMode;

/// The argument that starts Patchwork's own binary as the API agent.
pub const BRIDGE_ARG: &str = "--api-bridge";

/// Times a turn goes on with a reply the output limit cut off.
const MAX_CONTINUES: u32 = 3;

const CONTINUE: &str = "(Your reply reached the output limit and was cut off. Continue exactly where it stopped, without repeating what you already wrote.)";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Request {
    pub(super) base_url: String,
    pub(super) api_key: Option<String>,
    #[serde(default)]
    pub(super) headers: HashMap<String, String>,
    pub(super) model: String,
    /// The provider's name, for messages.
    pub(super) provider: String,
    pub(super) root: String,
    pub(super) sessions_dir: String,
    pub(super) session_id: Option<String>,
    pub(super) text: String,
    #[serde(default)]
    pub(super) images: Vec<ImageIn>,
    /// Whether `model` reads images. When it does not, the images in the conversation are left
    /// out of the request, and the conversation keeps them for a turn that can read them.
    pub(super) images_ok: bool,
    /// The model that reads images for `model`: it describes each image the user sends, and
    /// `model` gets the description in its place.
    #[serde(default)]
    pub(super) vision: Option<Vision>,
    pub(super) mode: AccessMode,
    pub(super) plan: bool,
    pub(super) effort: Option<String>,
    /// Summarizes the conversation before a request once the last one's context reached this; 0
    /// leaves it until the provider refuses the conversation as too long.
    #[serde(default)]
    pub(super) compact_window: u64,
    /// Patchwork's instructions about the user's shell.
    #[serde(default)]
    pub(super) instructions: Option<String>,
    /// The system prompt the user gave the provider in Settings.
    #[serde(default)]
    pub(super) system_prompt: String,
    /// `compact` summarizes the conversation instead of sending a message.
    #[serde(default)]
    pub(super) command: Option<String>,
    /// Tools of the MCP servers Patchwork runs, which it calls for the agent.
    #[serde(default)]
    pub(super) mcp_tools: Vec<McpTool>,
    /// The plugins' tool hooks, which the agent runs around each call.
    #[serde(default)]
    pub(super) hooks: Vec<AgentHook>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct McpTool {
    pub(super) name: String,
    pub(super) server: String,
    pub(super) tool: String,
    pub(super) description: String,
    pub(super) parameters: Value,
    pub(super) read_only: bool,
}

/// The vision model: a CLI the user is signed in to, which the agent runs for each message with
/// images, or an API provider's endpoint.
#[derive(Deserialize)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub(super) enum Vision {
    Cli {
        agent: AgentKind,
        model: String,
        /// Where the user located the CLI, when they did.
        path: Option<String>,
    },
    Endpoint {
        base_url: String,
        api_key: Option<String>,
        #[serde(default)]
        headers: HashMap<String, String>,
        model: String,
        provider: String,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ImageIn {
    pub(super) media_type: String,
    pub(super) data: String,
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

pub(super) struct Agent {
    pub(super) request: Request,
    pub(super) root: PathBuf,
    pub(super) session_id: String,
    pub(super) session_file: PathBuf,
    pub(super) messages: Vec<Value>,
    /// Tokens of the last request and its reply, which the next request starts from; saved with
    /// the conversation, so a turn knows where the last one left off.
    pub(super) context: u64,
    /// Replies stream to Patchwork unless set, as while summarizing on the agent's own.
    pub(super) quiet: bool,
    /// The model's context window as the endpoint reports it; None until asked, once a turn.
    pub(super) model_window: Option<Option<u64>>,
    pub(super) input: Receiver<Value>,
    /// Steering messages, and `/compact` requests, that came in while the agent was busy.
    pub(super) steers: Vec<Value>,
    /// Images `read_file` opened this step. A tool result is text only, so they follow the
    /// step's results as a message of their own.
    pub(super) read_images: Vec<ImageIn>,
    /// Stdin closed: Patchwork is ending the turn.
    pub(super) closed: bool,
    /// The last tool call, as its name and arguments, and how many times in a row it was made.
    pub(super) last_call: (String, u32),
    /// The step's reply stopped at the output limit, which is why a call's arguments can be cut off.
    pub(super) cut_off: bool,
    /// Each file the model read or wrote this turn, as its modified time and length then, so a
    /// write over a file that changed since is refused.
    pub(super) seen: HashMap<PathBuf, (SystemTime, u64)>,
    /// Results of the step's reads, run together ahead of the step's calls, by call id.
    pub(super) prefetched: HashMap<String, Result<String, String>>,
    pub(super) http: ureq::Agent,
    pub(super) tools: Value,
    /// Built once per turn, so every request starts the same and the provider's prompt cache
    /// keeps matching, even if the project's instructions file changes during the turn.
    pub(super) system: String,
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
        let mut tools = crate::harness::tools::definitions(request.plan);
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
            last_call: (String::new(), 0),
            cut_off: false,
            seen: HashMap::new(),
            prefetched: HashMap::new(),
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

        let mut continued = 0;
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
                    .map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": history_arguments(&c.arguments)}}))
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
            self.cut_off = reply.cut_off;
            self.prefetch(&reply.calls);
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
                if steer["type"] == "compact" {
                    emit(&json!({"type": "api.steered"}));
                    emit(&json!({"type": "api.compacting"}));
                    self.summarize(true)?;
                    continue;
                }
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
            // A reply the output limit stopped mid-text goes on from where it stopped, a few times
            // a turn, so a model that always runs long still ends.
            if reply.cut_off && reply.calls.is_empty() && continued < MAX_CONTINUES {
                continued += 1;
                self.messages
                    .push(json!({"role": "user", "content": CONTINUE}));
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
                Ok(v) if v["type"] == "steer" || v["type"] == "compact" => self.steers.push(v),
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

    /// Where tool output too long to send whole is saved: a folder beside the session's file,
    /// deleted with it.
    pub(super) fn saved_output_dir(&self) -> PathBuf {
        Path::new(&self.request.sessions_dir).join(&self.session_id)
    }

    pub(super) fn save(&self) -> Result<(), String> {
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
    let mut messages = v["messages"].as_array().cloned()?;
    // Repairs a conversation saved with a cut-off call, which every request would be refused for.
    for call in messages
        .iter_mut()
        .filter_map(|m| m["tool_calls"].as_array_mut())
        .flatten()
    {
        if let Some(arguments) = call["function"]["arguments"].as_str() {
            call["function"]["arguments"] = json!(history_arguments(arguments));
        }
    }
    Some((messages, v["context"].as_u64().unwrap_or(0)))
}

/// A call's arguments as the conversation keeps them: `{}` in place of arguments that are not a
/// JSON object, such as ones cut off at the output limit. Endpoints parse the history's calls and
/// refuse the whole request over one that does not parse; the call's result already tells the
/// model what went wrong.
fn history_arguments(arguments: &str) -> String {
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(_)) => arguments.to_string(),
        _ => "{}".to_string(),
    }
}

pub(super) fn emit(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}
