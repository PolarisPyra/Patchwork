//! The relay a CLI's turn reaches Patchwork's servers through: `patchwork --mcp-proxy` in the CLI's config, forwarding to the app.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use serde_json::{Value, json};

use super::{CALL_TIMEOUT, pool};

/// The argument that starts Patchwork's binary as a relay to one of Patchwork's servers.
pub const PROXY_ARG: &str = "--mcp-proxy";
const RELAY_FILE: &str = "mcp-relay.json";
static RELAY_READY: AtomicBool = AtomicBool::new(false);
/// This copy's own relay, as it bound it. Kept in memory rather than read back from `RELAY_FILE`,
/// which every copy of the app on the machine writes to the same state folder.
static ENDPOINT: OnceLock<Value> = OnceLock::new();

/// Answers a permission hook's request (see `crate::permissions::hook`).
type HookHandler = Box<dyn Fn(&Value) -> Value + Send + Sync>;
static HOOK_HANDLER: OnceLock<HookHandler> = OnceLock::new();

/// Sets what answers permission hooks; called once when the server opens.
pub fn set_hook_handler(handler: HookHandler) {
    let _ = HOOK_HANDLER.set(handler);
}

/// Shows the app's window, for a second launch of the app (see `crate::system::instance`).
type RaiseHandler = Box<dyn Fn() + Send + Sync>;
static RAISE_HANDLER: OnceLock<RaiseHandler> = OnceLock::new();

/// Sets what shows the app's window when a second launch asks; called once by the app.
pub fn set_raise_handler(handler: RaiseHandler) {
    let _ = RAISE_HANDLER.set(handler);
}

/// The server Patchwork's own tools are served under, to every agent.
pub const OWN_SERVER: &str = "patchwork";

/// Runs one of Patchwork's own tools for a thread's agent: the thread, the tool, its arguments,
/// and whether to stop waiting.
type ToolHandler =
    Box<dyn Fn(&str, &str, &Value, &dyn Fn() -> bool) -> Result<String, Error> + Send + Sync>;
static TOOL_HANDLER: OnceLock<ToolHandler> = OnceLock::new();

/// Sets what runs Patchwork's own tools; called once when the server opens.
pub fn set_tool_handler(handler: ToolHandler) {
    let _ = TOOL_HANDLER.set(handler);
}

/// Runs Patchwork's own tool `tool` for the agent of `thread_id`, until `stopped`.
pub(crate) fn call_own(
    thread_id: &str,
    tool: &str,
    args: &Value,
    stopped: &dyn Fn() -> bool,
) -> Result<String, Error> {
    TOOL_HANDLER
        .get()
        .ok_or_else(|| "Patchwork is still starting".into())
        .and_then(|handle| handle(thread_id, tool, args, stopped))
}

/// Answers a request a CLI sends Patchwork's own server, for the agent of `thread_id`.
fn own_request(
    thread_id: &str,
    method: &str,
    params: &Value,
    stopped: &dyn Fn() -> bool,
) -> Result<Value, Error> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": params["protocolVersion"].as_str().unwrap_or(super::PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": OWN_SERVER, "version": crate::VERSION},
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": [{
            "name": crate::secrets::TOOL,
            "description": crate::secrets::DESCRIPTION,
            "inputSchema": crate::secrets::input_schema(),
        }]})),
        "tools/call" => {
            let tool = params["name"].as_str().unwrap_or_default();
            let (text, error) = match call_own(thread_id, tool, &params["arguments"], stopped) {
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

/// The relay this copy of Patchwork listens on, as `{"port", "token"}`.
///
/// A turn is handed it outright, so its hooks reach the app that started them: two copies of
/// Patchwork share one state folder, and `relay_file` names whichever of them started last.
pub fn relay_endpoint() -> Option<Value> {
    ENDPOINT.get().cloned()
}

pub(super) fn serve_relay() -> Result<(), Error> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let endpoint = json!({"port": port, "token": token});
    let _ = ENDPOINT.set(endpoint.clone());
    crate::system::instance::listens(&endpoint);
    crate::system::fs::write_private(
        &crate::system::dirs().state.join(RELAY_FILE),
        &endpoint.to_string(),
    )?;
    RELAY_READY.store(true, Ordering::Relaxed);
    for stream in listener.incoming().flatten() {
        let token = token.clone();
        std::thread::spawn(move || relay_client(stream, &token));
    }
    Ok(())
}

/// Serves one CLI's relay: its first line names the server and carries the token, and later
/// lines are JSON-RPC messages, each answered from Patchwork's copy of the server.
fn relay_client(stream: std::net::TcpStream, token: &str) {
    let Ok(reader) = stream.try_clone() else {
        return;
    };
    let mut lines = BufReader::new(reader).lines().map_while(Result::ok);
    let Some(hello) = lines
        .next()
        .and_then(|l| serde_json::from_str::<Value>(&l).ok())
    else {
        return;
    };
    if hello["token"].as_str() != Some(token) {
        return;
    }
    // A second launch of the app asks this one to come forward, and exits once it has.
    if hello["raise"] == true {
        let Some(raise) = RAISE_HANDLER.get() else {
            return;
        };
        raise();
        let mut stream = stream;
        let _ = writeln!(stream, "raised");
        return;
    }
    // A permission hook asks once and waits for the answer.
    if !hello["hook"].is_null() {
        let answer = HOOK_HANDLER
            .get()
            .map_or(json!({"deny": "Patchwork is still starting"}), |handle| {
                handle(&hello["hook"])
            });
        let mut stream = stream;
        let _ = writeln!(stream, "{answer}");
        return;
    }
    let server = hello["server"].as_str().unwrap_or_default().to_string();
    // A relay that names its thread is to Patchwork's own server, for that thread's agent.
    let thread = hello["thread"].as_str().map(str::to_string);
    let out = Arc::new(Mutex::new(stream));
    let reply = move |out: &Mutex<std::net::TcpStream>, v: Value| {
        let mut out = out.lock().unwrap_or_else(PoisonError::into_inner);
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
    if let Some(entry) = pool().get(&server).cloned().filter(|_| thread.is_none()) {
        let (tx, rx) = mpsc::channel::<Value>();
        entry
            .notices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(tx);
        let (out, tokens) = (Arc::clone(&out), Arc::clone(&tokens));
        std::thread::spawn(move || {
            for mut notice in rx {
                if notice["method"] == "notifications/progress" {
                    let ours = notice["params"]["progressToken"].to_string();
                    let Some(theirs) = tokens
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .get(&ours)
                        .cloned()
                    else {
                        continue;
                    };
                    notice["params"]["progressToken"] = theirs;
                }
                let mut out = out.lock().unwrap_or_else(PoisonError::into_inner);
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
                if let Some(flag) = running
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&key)
                {
                    flag.store(true, Ordering::Relaxed);
                }
            }
            continue;
        };
        let (out, running, server, tokens, thread) = (
            Arc::clone(&out),
            Arc::clone(&running),
            server.clone(),
            Arc::clone(&tokens),
            thread.clone(),
        );
        let mut params = message["params"].clone();
        let token = params["_meta"]["progressToken"].clone();
        let ours = (!token.is_null()).then(|| {
            let ours = Value::from(format!("patchwork-{}", uuid::Uuid::new_v4().simple()));
            params["_meta"]["progressToken"] = ours.clone();
            tokens
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(ours.to_string(), token);
            ours
        });
        let stop = Arc::new(AtomicBool::new(false));
        running
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.to_string(), Arc::clone(&stop));
        std::thread::spawn(move || {
            let stopped = || stop.load(Ordering::Relaxed);
            let result = match &thread {
                Some(thread) => own_request(thread, &method, &params, &stopped),
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
                        _ => conn.request(&method, params, CALL_TIMEOUT, &stopped),
                    }),
            };
            running
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id.to_string());
            if let Some(ours) = ours {
                tokens
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&ours.to_string());
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
    // The CLI has gone, so nothing reads the answers to what it still had running.
    running
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .values()
        .for_each(|stop| stop.store(true, Ordering::Relaxed));
}

/// Runs the MCP relay process that a CLI starts, and returns its exit code.
///
/// Runs the relay a CLI starts: `--mcp-proxy <relay file> <server> [thread]`, with the thread
/// for Patchwork's own server. It passes stdin to Patchwork and Patchwork's answers to stdout; the
/// process exits with what this returns.
#[expect(
    clippy::print_stderr,
    reason = "a CLI runs this as a program of its own and shows its stderr; it has no log"
)]
pub fn run_proxy() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let (file, server, thread) = match args.as_slice() {
        [file, server] => (file, server, None),
        [file, server, thread] => (file, server, Some(thread)),
        _ => {
            eprintln!("Usage: {PROXY_ARG} <relay file> <server> [thread]");
            return 2;
        }
    };
    let relay: Value = if let Some(relay) = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        relay
    } else {
        eprintln!("Patchwork is not running");
        return 1;
    };
    let Some(port) = relay["port"].as_u64().and_then(|p| u16::try_from(p).ok()) else {
        eprintln!("Patchwork's relay file names no port");
        return 1;
    };
    let mut stream = match std::net::TcpStream::connect(("127.0.0.1", port)) {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("Could not reach Patchwork: {e}");
            return 1;
        }
    };
    let hello = json!({"token": relay["token"], "server": server, "thread": thread});
    if writeln!(stream, "{hello}").is_err() {
        return 1;
    }
    let Ok(incoming) = stream.try_clone() else {
        return 1;
    };
    std::thread::spawn(move || {
        let mut stdout = std::io::stdout();
        for line in BufReader::new(incoming).lines().map_while(Result::ok) {
            if writeln!(stdout, "{line}")
                .and_then(|()| stdout.flush())
                .is_err()
            {
                break;
            }
        }
        // Patchwork closed the relay, e.g. on quitting.
        std::process::exit(0);
    });
    for line in std::io::stdin().lines().map_while(Result::ok) {
        if writeln!(stream, "{line}").is_err() {
            return 1;
        }
    }
    // The CLI is done sending; the answers still owed arrive before Patchwork closes the relay,
    // which ends the process above.
    let _ = stream.shutdown(std::net::Shutdown::Write);
    loop {
        std::thread::park();
    }
}
