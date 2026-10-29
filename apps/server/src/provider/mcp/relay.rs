//! The relay a CLI's turn reaches Patchwork's servers through: `patchwork --mcp-proxy` in the CLI's config, forwarding to the app.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use serde_json::{Value, json};

use super::{CALL_TIMEOUT, path, pool};

/// The argument that starts Patchwork's binary as a relay to one of Patchwork's servers.
pub const PROXY_ARG: &str = "--mcp-proxy";
const RELAY_FILE: &str = "mcp-relay.json";
static RELAY_READY: AtomicBool = AtomicBool::new(false);
/// This copy's own relay, as it bound it. Kept in memory rather than read back from `RELAY_FILE`,
/// which every copy of the app on the machine writes to the same config folder.
static ENDPOINT: OnceLock<Value> = OnceLock::new();

/// Answers a permission hook's request (see `crate::permissions::hook`).
type HookHandler = Box<dyn Fn(&Value) -> Value + Send + Sync>;
static HOOK_HANDLER: OnceLock<HookHandler> = OnceLock::new();

/// Sets what answers permission hooks; called once when the server opens.
pub fn set_hook_handler(handler: HookHandler) {
    let _ = HOOK_HANDLER.set(handler);
}

/// The file the relay reads its port and token from, once Patchwork is listening.
pub fn relay_file() -> Option<PathBuf> {
    RELAY_READY
        .load(Ordering::Relaxed)
        .then(|| path().with_file_name(RELAY_FILE))
}

/// The relay this copy of Patchwork listens on, as `{"port", "token"}`. A turn is handed it
/// outright, so its hooks reach the app that started them: two copies of Patchwork share one
/// config folder, and `relay_file` names whichever of them started last.
pub fn relay_endpoint() -> Option<Value> {
    ENDPOINT.get().cloned()
}

pub(super) fn serve_relay() -> Result<(), String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let endpoint = json!({"port": port, "token": token});
    let _ = ENDPOINT.set(endpoint.clone());
    crate::system::fs::write_private(&path().with_file_name(RELAY_FILE), &endpoint.to_string())?;
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
    if let Some(entry) = pool().get(&server).cloned() {
        let (tx, rx) = mpsc::channel::<Value>();
        entry
            .notices
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(tx);
        let (out, tokens) = (out.clone(), tokens.clone());
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
                if writeln!(out, "{notice}").and_then(|_| out.flush()).is_err() {
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
        let (out, running, server, tokens) =
            (out.clone(), running.clone(), server.clone(), tokens.clone());
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
            .insert(id.to_string(), stop.clone());
        std::thread::spawn(move || {
            let entry = pool().get(&server).cloned();
            let result = entry
                .ok_or_else(|| format!("Patchwork is not running the MCP server {server}"))
                .and_then(|entry| entry.connect())
                .and_then(|conn| match method.as_str() {
                    "initialize" => Ok(conn.info.clone()),
                    "ping" => Ok(json!({})),
                    _ => conn.request(&method, params, CALL_TIMEOUT, &|| {
                        stop.load(Ordering::Relaxed)
                    }),
                });
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
}

/// Runs the relay a CLI starts: `--mcp-proxy <relay file> <server>`. It passes stdin to
/// Patchwork and Patchwork's answers to stdout; the process exits with what this returns.
pub fn run_proxy() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let [file, server] = args.as_slice() else {
        eprintln!("Usage: {PROXY_ARG} <relay file> <server>");
        return 2;
    };
    let relay: Value = match std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
    {
        Some(relay) => relay,
        None => {
            eprintln!("Patchwork is not running");
            return 1;
        }
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
    let hello = json!({"token": relay["token"], "server": server});
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
                .and_then(|_| stdout.flush())
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
