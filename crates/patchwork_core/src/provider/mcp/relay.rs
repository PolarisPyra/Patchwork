//! The relay a CLI's turn reaches Patchwork's servers through: `patchwork --mcp-proxy` in the CLI's config, forwarding to the app.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use crate::Error;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Value, json};

use super::config::TurnTool;
pub use patchwork_harness::mcp_proxy::{OWN_SERVER, PROXY_ARG, run_proxy};
use super::{CALL_TIMEOUT, REQUEST_TIMEOUT, pool};

const RELAY_FILE: &str = "mcp-relay.json";
static RELAY_READY: AtomicBool = AtomicBool::new(false);
/// The id the next CLI's notification listener is kept by.
static LISTENERS: AtomicU64 = AtomicU64::new(0);
/// This copy's own relay, as it bound it. Kept in memory rather than read back from `RELAY_FILE`,
/// which every copy of the app on the machine writes to the same state folder.
static ENDPOINT: OnceLock<Value> = OnceLock::new();

/// Runs one of Patchwork's own tools for a chat's agent: the chat, the tool, its arguments,
/// and whether to stop waiting.
type ToolHandler =
    Box<dyn Fn(&str, &str, &Value, &dyn Fn() -> bool) -> Result<String, Error> + Send + Sync>;

/// Answers a permission hook's request.
type HookHandler = Box<dyn Fn(&Value) -> Value + Send + Sync>;

/// Lists a chat's own tools as `tools/list` entries.
type ToolLister = Box<dyn Fn(&str) -> Vec<Value> + Send + Sync>;

/// What answers the requests that reach the relay; the core installs them once as it opens.
pub(crate) struct Handlers {
    /// Answers a permission hook's request (see `crate::permissions::hook`).
    pub hook: HookHandler,
    /// Runs Patchwork's own tools.
    pub tool: ToolHandler,
    /// The tools of Patchwork's own that a chat's agent has, as `tools/list` entries.
    pub list_tools: ToolLister,
    /// Shows the app's window, for a second launch of the app (see
    /// `crate::system::processes::instance`).
    pub raise: Box<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for Handlers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handlers").finish_non_exhaustive()
    }
}

static HANDLERS: OnceLock<Handlers> = OnceLock::new();

/// Sets what answers the relay's requests; a second call is ignored.
pub(crate) fn install(handlers: Handlers) {
    let _ = HANDLERS.set(handlers);
}

/// The tools of Patchwork's own that `chat_id`'s agent has; none while Patchwork starts.
fn list_own(chat_id: &str) -> Vec<Value> {
    HANDLERS
        .get()
        .map(|h| (h.list_tools)(chat_id))
        .unwrap_or_default()
}

/// The tools of Patchwork's own that a chat's agent has beside the one every agent has, as an
/// API turn offers them to the model.
pub(crate) fn own_turn_tools(chat_id: &str) -> Vec<TurnTool> {
    let listed = list_own(chat_id);
    listed
        .into_iter()
        .filter_map(|tool| {
            let name = tool["name"].as_str()?.to_string();
            // An API turn asks for a secret through its own harness.
            if name == crate::vault::requests::TOOL {
                return None;
            }
            Some(TurnTool {
                tool: name.clone(),
                name,
                server: OWN_SERVER.into(),
                description: tool["description"].as_str().unwrap_or_default().into(),
                parameters: tool["inputSchema"].clone(),
                read_only: false,
            })
        })
        .collect()
}

/// Runs Patchwork's own tool `tool` for the agent of `chat_id`, until `stopped`.
pub(crate) fn call_own(
    chat_id: &str,
    tool: &str,
    args: &Value,
    stopped: &dyn Fn() -> bool,
) -> Result<String, Error> {
    HANDLERS
        .get()
        .ok_or_else(|| "Patchwork is still starting".into())
        .and_then(|h| (h.tool)(chat_id, tool, args, stopped))
}

/// Answers a request a CLI sends Patchwork's own server, for the agent of `chat_id`.
fn own_request(
    chat_id: &str,
    method: &str,
    params: &Value,
    stopped: &dyn Fn() -> bool,
) -> Result<Value, Error> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or(super::PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": OWN_SERVER, "version": crate::system::VERSION},
        })),
        "ping" => Ok(json!({})),
        "tools/list" => {
            let tools = list_own(chat_id);
            Ok(json!({"tools": tools}))
        }
        "tools/call" => {
            let tool = params["name"].as_str().unwrap_or_default();
            let (text, error) = match call_own(chat_id, tool, &params["arguments"], stopped) {
                Ok(text) => (text, false),
                Err(error) => (error.to_string(), true),
            };
            Ok(json!({"content": [{"type": "text", "text": text}], "isError": error}))
        }
        _ => Err(format!("Patchwork's own server does not answer {method}").into()),
    }
}

/// The file the relay reads its port and token from, once Patchwork is listening.
pub fn relay_file() -> Option<PathBuf> {
    RELAY_READY
        .load(Ordering::Relaxed)
        .then(|| crate::system::dirs().state.join(RELAY_FILE))
}

/// The file that names this copy's relay alone, for the servers a turn is launched with.
///
/// `relay_file` names whichever copy of Patchwork started last, where a turn's servers have to
/// reach the copy that started the turn.
#[must_use]
pub fn own_relay_file() -> Option<PathBuf> {
    let port = relay_endpoint()?["port"].as_u64()?;
    relay_file().map(|file| file.with_file_name(format!("mcp-relay-{port}.json")))
}

/// Removes the files that name the relays of copies of Patchwork that have since closed.
fn sweep_relay_files(state: &std::path::Path) {
    for entry in std::fs::read_dir(state).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let port = name
            .to_str()
            .and_then(|n| n.strip_prefix("mcp-relay-")?.strip_suffix(".json"))
            .and_then(|p| p.parse::<u16>().ok());
        let Some(port) = port else { continue };
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        if std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(200))
            .is_err()
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The relay this copy of Patchwork listens on, as `{"port", "token"}`.
///
/// A turn is handed it outright, so its hooks reach the app that started them: two copies of
/// Patchwork share one state folder, and `relay_file` names whichever of them started last.
pub fn relay_endpoint() -> Option<Value> {
    ENDPOINT.get().cloned()
}

/// How long a new connection has to send its first line.
const HELLO_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The longest first line taken. A permission hook's carries the whole call it asks about, which
/// can be a file's new contents, so this is far above any real one and only bounds memory.
const HELLO_LIMIT: u64 = 32 * 1024 * 1024;

/// The most connections served at once.
const MAX_CLIENTS: usize = 512;

/// Whether `given` is the relay's token, compared in the same time wherever they differ.
fn same_token(given: &str, token: &str) -> bool {
    given.len() == token.len()
        && given
            .bytes()
            .zip(token.bytes())
            .fold(0, |differing, (a, b)| differing | (a ^ b))
            == 0
}

pub(super) fn serve_relay() -> Result<(), Error> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let endpoint = json!({"port": port, "token": token});
    let _ = ENDPOINT.set(endpoint.clone());
    crate::system::ssh::set_relay_port(port);
    crate::system::processes::instance::listens(&endpoint);
    let state = &crate::system::dirs().state;
    sweep_relay_files(state);
    // Without its files the relay still answers what is handed the endpoint outright: a second
    // launch of the app, and the permission hooks.
    let written = [RELAY_FILE.to_string(), format!("mcp-relay-{port}.json")]
        .iter()
        .try_for_each(|name| {
            crate::system::files::paths::write_private(&state.join(name), &endpoint.to_string())
        });
    match written {
        Ok(()) => RELAY_READY.store(true, Ordering::Relaxed),
        Err(e) => log::warn!(error:% = e; "MCP servers are not shared with the CLIs: {{error}}"),
    }
    let open = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming().flatten() {
        // Over the most that Patchwork's own agents, hooks and servers open at once, a
        // connection is something else on this machine holding chats, and is dropped.
        if open.fetch_add(1, Ordering::Relaxed) >= MAX_CLIENTS {
            open.fetch_sub(1, Ordering::Relaxed);
            continue;
        }
        let (token, open) = (token.clone(), Arc::clone(&open));
        std::thread::spawn(move || {
            relay_client(stream, &token);
            open.fetch_sub(1, Ordering::Relaxed);
        });
    }
    Ok(())
}

/// Serves one CLI's relay: its first line names the server and carries the token, and later
/// lines are JSON-RPC messages, each answered from Patchwork's copy of the server.
fn relay_client(stream: std::net::TcpStream, token: &str) {
    let Ok(reader) = stream.try_clone() else {
        return;
    };
    // Until it has shown the token a connection may be anything on this machine, so its first
    // line has a time and a size it must come within.
    let _ = stream.set_read_timeout(Some(HELLO_WAIT));
    let mut reader = BufReader::new(reader);
    let mut first = String::new();
    let read = std::io::Read::take(&mut reader, HELLO_LIMIT).read_line(&mut first);
    if read.is_err() || !first.ends_with('\n') {
        return;
    }
    let Ok(hello) = serde_json::from_str::<Value>(&first) else {
        return;
    };
    if !same_token(hello["token"].as_str().unwrap_or_default(), token) {
        return;
    }
    // A server's session then lasts as long as its agent's turn.
    let _ = stream.set_read_timeout(None);
    let lines = reader.lines().map_while(Result::ok);
    // A second launch of the app asks this one to come forward, and exits once it has.
    if hello["raise"] == true {
        let Some(handlers) = HANDLERS.get() else {
            return;
        };
        (handlers.raise)();
        let mut stream = stream;
        let _ = writeln!(stream, "raised");
        return;
    }
    // Git's helper asks one sign-in question and waits for the user's answer.
    if let Some(prompt) = hello["askpass"].as_str() {
        let answer = crate::source_control::answer_askpass(prompt);
        let mut stream = stream;
        let _ = writeln!(stream, "{}", json!({"answer": answer}));
        return;
    }
    // A permission hook asks once and waits for the answer.
    if !hello["hook"].is_null() {
        let answer = HANDLERS
            .get()
            .map_or(json!({"deny": "Patchwork is still starting"}), |h| {
                (h.hook)(&hello["hook"])
            });
        let mut stream = stream;
        let _ = writeln!(stream, "{answer}");
        return;
    }
    let server = hello["server"].as_str().unwrap_or_default().to_string();
    // A relay that names its chat is to Patchwork's own server, for that chat's agent.
    let chat = hello["chat"].as_str().map(str::to_string);
    let out = Arc::new(Mutex::new(stream));
    let reply = move |out: &Mutex<std::net::TcpStream>, v: Value| {
        let mut out = out.locked();
        let _ = writeln!(out, "{v}");
        let _ = out.flush();
    };
    // Requests still running, by the CLI's id, so a cancel can stop them.
    let running: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>> = Arc::default();
    // Progress tokens this CLI's requests carry, by the token sent on in their place: tokens
    // are the client's own, and two CLIs could pick the same one.
    let tokens: Arc<Mutex<HashMap<String, Value>>> = Arc::default();
    // The server's notifications: progress goes to the CLI whose request it is about, and the
    // rest, such as a changed tool list, to every CLI sharing the server.
    let listening = pool().get(&server).cloned().filter(|_| chat.is_none());
    let listener = LISTENERS.fetch_add(1, Ordering::Relaxed);
    if let Some(entry) = &listening {
        let (tx, rx) = mpsc::channel::<Value>();
        entry.notices.locked().push((listener, tx));
        let (out, tokens) = (Arc::clone(&out), Arc::clone(&tokens));
        std::thread::spawn(move || {
            for mut notice in rx {
                if notice["method"] == "notifications/progress" {
                    let ours = notice["params"]["progressToken"].to_string();
                    let Some(theirs) = tokens.locked().get(&ours).cloned() else {
                        continue;
                    };
                    notice["params"]["progressToken"] = theirs;
                }
                let mut out = out.locked();
                if writeln!(out, "{notice}")
                    .and_then(|()| out.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
    }
    for line in lines {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let method = message["method"].as_str().unwrap_or_default().to_string();
        let Some(id) = message.get("id").cloned() else {
            if method == "notifications/cancelled" {
                let key = message["params"]["requestId"].to_string();
                if let Some(flag) = running.locked().get(&key) {
                    flag.store(true, Ordering::Relaxed);
                }
            }
            continue;
        };
        let (out, running, server, tokens, chat) = (
            Arc::clone(&out),
            Arc::clone(&running),
            server.clone(),
            Arc::clone(&tokens),
            chat.clone(),
        );
        let mut params = message["params"].clone();
        let token = params["_meta"]["progressToken"].clone();
        let ours = (!token.is_null()).then(|| {
            let ours = Value::from(format!("patchwork-{}", uuid::Uuid::new_v4().simple()));
            params["_meta"]["progressToken"] = ours.clone();
            tokens.locked().insert(ours.to_string(), token);
            ours
        });
        let stop = Arc::new(AtomicBool::new(false));
        running.locked().insert(id.to_string(), Arc::clone(&stop));
        std::thread::spawn(move || {
            let stopped = || stop.load(Ordering::Relaxed);
            let result = match &chat {
                Some(chat) => own_request(chat, &method, &params, &stopped),
                None => pool()
                    .get(&server)
                    .cloned()
                    .ok_or_else(|| {
                        format!("Patchwork is not running the MCP server {server}").into()
                    })
                    .and_then(|entry| entry.connect())
                    .and_then(|conn| match method.as_str() {
                        "initialize" => Ok(conn.info.clone()),
                        "ping" => Ok(json!({})),
                        "tools/call" => conn.request(&method, params, CALL_TIMEOUT, &stopped),
                        _ => conn.request(&method, params, REQUEST_TIMEOUT, &stopped),
                    }),
            };
            running.locked().remove(&id.to_string());
            if let Some(ours) = ours {
                tokens.locked().remove(&ours.to_string());
            }
            let answer = match result {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                Err(message) => {
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": message}})
                }
            };
            reply(&out, answer);
        });
    }
    // The CLI has gone: its listener stops with its channel, and nothing reads the answers to
    // what it still had running.
    if let Some(entry) = listening {
        entry.notices.locked().retain(|(id, _)| *id != listener);
    }
    running
        .locked()
        .values()
        .for_each(|stop| stop.store(true, Ordering::Relaxed));
}
