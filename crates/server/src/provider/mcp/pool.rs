//! The running servers: starting them, their tools, and calling those tools for an API turn.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{
    CALL_TIMEOUT, HttpServer, MAX_NAME, MAX_TOOLS, McpServer, POLL, POOL, PROTOCOL, read_config,
    resolved, restart_changed, START_TIMEOUT, StdioServer, ToolInfo, TurnTool,
};

pub(super) fn pool() -> std::sync::MutexGuard<'static, HashMap<String, Arc<Entry>>> {
    POOL.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The enabled servers, each started if it is not yet, in parallel. A server that failed
/// stays failed until it is changed or restarted, so turns do not wait on it again.
pub(super) fn live_servers() -> Vec<(String, Arc<Conn>)> {
    let servers: BTreeMap<String, McpServer> = read_config()
        .mcp_servers
        .into_iter()
        .filter(|(_, s)| !s.disabled)
        .map(|(name, s)| {
            let config = resolved(&name, &s);
            (name, config)
        })
        .collect();
    let entries: Vec<(String, Arc<Entry>)> = {
        let mut pool = pool();
        pool.retain(|name, entry| servers.get(name).is_some_and(|s| *s == entry.config));
        servers
            .into_iter()
            .map(|(name, config)| {
                let entry = pool
                    .entry(name.clone())
                    .or_insert_with(|| {
                        Arc::new(Entry {
                            config,
                            conn: Mutex::new(None),
                            notices: Arc::default(),
                        })
                    })
                    .clone();
                (name, entry)
            })
            .collect()
    };
    let handles: Vec<_> = entries
        .into_iter()
        .map(|(name, entry)| std::thread::spawn(move || entry.connect().ok().map(|c| (name, c))))
        .collect();
    handles
        .into_iter()
        .filter_map(|h| h.join().ok().flatten())
        .collect()
}

/// How many MCP tools a turn may offer, after Patchwork's own.
pub fn tool_budget() -> usize {
    MAX_TOOLS.saturating_sub(
        crate::harness::tools::definitions(false)
            .as_array()
            .map_or(0, Vec::len),
    )
}

/// The servers that are up, without waiting on any. One still starting is left out of the
/// turn; one not started yet, or that stopped, is started for the turns after it.
fn ready_servers() -> Vec<(String, McpServer, Arc<Conn>)> {
    let mut out = Vec::new();
    let mut start = false;
    {
        let pool = pool();
        for (name, server) in read_config()
            .mcp_servers
            .into_iter()
            .filter(|(_, s)| !s.disabled)
        {
            let Some(entry) = pool
                .get(&name)
                .filter(|e| e.config == resolved(&name, &server))
            else {
                start = true;
                continue;
            };
            // Locked while it starts.
            let Ok(conn) = entry.conn.try_lock().map(|c| c.clone()) else {
                continue;
            };
            match conn {
                Some(Ok(conn)) if conn.alive() => out.push((name, server, conn)),
                Some(Err(_)) => {}
                _ => start = true,
            }
        }
    }
    if start {
        restart_changed();
    }
    out
}

/// The tools of every running server, for a turn, up to `tool_budget`. Plan mode gets only
/// the read-only ones.
pub fn turn_tools(plan: bool) -> Vec<TurnTool> {
    let mut taken = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (server, config, conn) in ready_servers() {
        for tool in &conn.tools {
            if (plan && !tool.read_only) || config.disabled_tools.contains(&tool.name) {
                continue;
            }
            let mut name = format!("mcp__{}__{}", clean(&server), clean(&tool.name));
            name.truncate(MAX_NAME);
            if !taken.insert(name.clone()) {
                continue;
            }
            out.push(TurnTool {
                name,
                server: server.clone(),
                tool: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.schema.clone(),
                read_only: tool.read_only,
            });
        }
    }
    out.truncate(tool_budget());
    out
}

/// Starts a server just to list its tools, then stops it.
pub fn probe(config: &McpServer) -> Result<Vec<ToolInfo>, String> {
    Conn::start(config, Arc::default()).map(|conn| conn.tool_infos())
}

/// A name with only the characters tool names allow.
fn clean(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Calls a tool, giving up once `stopped` says the turn was stopped. Ok carries its result for
/// the model; Err a failure the model is told about.
pub fn call(
    server: &str,
    tool: &str,
    arguments: &Value,
    stopped: &dyn Fn() -> bool,
) -> Result<String, String> {
    let entry = pool()
        .get(server)
        .cloned()
        .ok_or_else(|| format!("The MCP server {server} is not running"))?;
    let params = json!({"name": tool, "arguments": arguments});
    let conn = entry.connect()?;
    let result = match conn.request("tools/call", params.clone(), CALL_TIMEOUT, stopped) {
        // The server died or dropped the session; one new start, then the call once more.
        Err(e) if !conn.alive() && !stopped() => {
            entry.drop_conn();
            entry
                .connect()?
                .request("tools/call", params, CALL_TIMEOUT, stopped)
                .map_err(|_| e)?
        }
        other => other?,
    };
    let text = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|part| match part["type"].as_str() {
            Some("text") => part["text"].as_str().unwrap_or_default().to_string(),
            Some("resource") => part["resource"]["text"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| {
                    format!(
                        "(resource {})",
                        part["resource"]["uri"].as_str().unwrap_or_default()
                    )
                }),
            Some("resource_link") => {
                format!("(resource {})", part["uri"].as_str().unwrap_or_default())
            }
            Some(kind) => format!("({kind} content left out)"),
            None => String::new(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let text = if text.is_empty() && !result["structuredContent"].is_null() {
        result["structuredContent"].to_string()
    } else {
        text
    };
    if result["isError"] == true {
        Err(text)
    } else {
        Ok(text)
    }
}

pub(super) struct Entry {
    pub(super) config: McpServer,
    /// None until started; held while starting, so callers wait for one start.
    pub(super) conn: Mutex<Option<Result<Arc<Conn>, String>>>,
    /// Where the server's notifications go: the CLIs sharing it. Kept across restarts.
    pub(super) notices: Notices,
}

/// Listeners for a server's notifications.
pub(super) type Notices = Arc<Mutex<Vec<Sender<Value>>>>;

/// Passes a notification to every listener, forgetting those that have gone.
pub(super) fn announce(notices: &Notices, notice: &Value) {
    notices
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|l| l.send(notice.clone()).is_ok());
}

impl Entry {
    pub(super) fn connect(&self) -> Result<Arc<Conn>, String> {
        let mut conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(Ok(c)) = conn.as_ref()
            && !c.alive()
        {
            *conn = None;
        }
        conn.get_or_insert_with(|| Conn::start(&self.config, self.notices.clone()).map(Arc::new))
            .clone()
    }

    fn drop_conn(&self) {
        *self.conn.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

struct Tool {
    name: String,
    description: String,
    schema: Value,
    read_only: bool,
}

pub(super) struct Conn {
    transport: Transport,
    tools: Vec<Tool>,
    next_id: AtomicU64,
    /// The server's answer to `initialize`, given again to the CLIs that share it.
    pub(super) info: Value,
}

enum Transport {
    Stdio(StdioServer),
    Http(HttpServer),
}

impl Conn {
    fn start(config: &McpServer, notices: Notices) -> Result<Conn, String> {
        let transport = match (&config.command, &config.url) {
            (Some(command), _) => Transport::Stdio(StdioServer::spawn(
                command,
                &config.args,
                &config.env,
                notices,
            )?),
            (None, Some(url)) => Transport::Http(HttpServer::new(url, &config.headers, notices)),
            (None, None) => return Err("No command or URL".into()),
        };
        let mut conn = Conn {
            transport,
            tools: Vec::new(),
            next_id: AtomicU64::new(1),
            info: Value::Null,
        };
        let init = json!({
            "protocolVersion": PROTOCOL,
            "capabilities": {},
            "clientInfo": {"name": "Patchwork", "version": crate::VERSION},
        });
        conn.info = conn
            .request("initialize", init, START_TIMEOUT, &|| false)
            .map_err(|e| conn.explain(e))?;
        conn.notify("notifications/initialized");
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor {
                Some(c) => json!({"cursor": c}),
                None => json!({}),
            };
            let page = conn
                .request("tools/list", params, START_TIMEOUT, &|| false)
                .map_err(|e| conn.explain(e))?;
            for t in page["tools"].as_array().into_iter().flatten() {
                let Some(name) = t["name"].as_str() else {
                    continue;
                };
                let mut schema = t["inputSchema"].clone();
                if !schema.is_object() {
                    schema = json!({"type": "object", "properties": {}});
                }
                // Some endpoints refuse a tool whose parameters name their JSON Schema dialect.
                if let Some(schema) = schema.as_object_mut() {
                    schema.remove("$schema");
                }
                conn.tools.push(Tool {
                    name: name.into(),
                    description: t["description"].as_str().unwrap_or_default().into(),
                    schema,
                    read_only: t["annotations"]["readOnlyHint"] == true,
                });
            }
            cursor = page["nextCursor"].as_str().map(String::from);
            if cursor.is_none() {
                break;
            }
        }
        Ok(conn)
    }

    pub(super) fn tool_infos(&self) -> Vec<ToolInfo> {
        self.tools
            .iter()
            .map(|t| ToolInfo {
                name: t.name.clone(),
                description: t.description.clone(),
            })
            .collect()
    }

    pub(super) fn alive(&self) -> bool {
        match &self.transport {
            Transport::Stdio(s) => s.alive(),
            Transport::Http(h) => h.alive.load(Ordering::Relaxed),
        }
    }

    /// A start failure, with the end of the server's stderr when it has any.
    fn explain(&self, error: String) -> String {
        match &self.transport {
            Transport::Stdio(s) => {
                let tail = s.stderr.lock().unwrap_or_else(PoisonError::into_inner);
                match tail.trim() {
                    "" => error,
                    tail => format!("{error}: {tail}"),
                }
            }
            Transport::Http(_) => error,
        }
    }

    /// Sends a request and waits for its answer. A request that runs out of time, or whose turn
    /// is stopped, is cancelled at the server.
    pub(super) fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        stopped: &dyn Fn() -> bool,
    ) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut message = json!({"jsonrpc": "2.0", "id": id, "method": method});
        // A request without params leaves them out; some servers refuse a null.
        if !params.is_null() {
            message["params"] = params;
        }
        let answer = match &self.transport {
            Transport::Stdio(s) => s.send_request(id, &message)?,
            Transport::Http(h) => h.send_request(id, message),
        };
        let deadline = Instant::now() + timeout;
        let reply = loop {
            match answer.recv_timeout(POLL) {
                Ok(reply) => break reply?,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("The MCP server exited".into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let stop = stopped();
                    if stop || Instant::now() >= deadline {
                        let reason = if stop {
                            "The turn was stopped"
                        } else {
                            "Timed out"
                        };
                        self.cancel(id, reason);
                        return Err(if stop {
                            reason.into()
                        } else {
                            format!("The MCP server did not answer in {} s", timeout.as_secs())
                        });
                    }
                }
            }
        };
        if let Some(error) = reply.get("error") {
            return Err(error["message"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| error.to_string()));
        }
        Ok(reply["result"].clone())
    }

    fn notify(&self, method: &str) {
        let message = json!({"jsonrpc": "2.0", "method": method});
        match &self.transport {
            Transport::Stdio(s) => {
                let _ = s.send(&message);
            }
            Transport::Http(h) => {
                let _ = h.post(&message);
            }
        }
    }

    /// Tells the server to stop working on a request nobody waits for any more.
    fn cancel(&self, id: u64, reason: &str) {
        let message = json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": id, "reason": reason}});
        match &self.transport {
            Transport::Stdio(s) => {
                s.waiters
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&id);
                let _ = s.send(&message);
            }
            Transport::Http(h) => {
                let h = h.clone();
                std::thread::spawn(move || h.post(&message));
            }
        }
    }
}

/// Where a request's answer arrives.
pub(super) type Answer = mpsc::Receiver<Result<Value, String>>;
pub(super) type Waiters = Arc<Mutex<HashMap<u64, Sender<Result<Value, String>>>>>;
