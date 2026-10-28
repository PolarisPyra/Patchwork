//! MCP servers for API providers. The servers live in `mcp-servers.json` in the config folder,
//! in the `mcpServers` shape other MCP clients use, so a server's README snippet pastes in as
//! is. Each enabled server starts once, when Patchwork opens, and stays up: every API turn
//! gets its tools without waiting, and a server keeps its state (a browser, a login) between
//! turns. A server runs as a command speaking JSON-RPC over stdio, or at a URL over MCP's
//! streamable HTTP. Environment and header values go to the system credential store, as API
//! keys do, and the file keeps only their names; a value typed into the file by hand is used
//! as is until the server is next saved.

use ts_rs::TS;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const CONFIG_FILE: &str = "mcp-servers.json";
const PROTOCOL: &str = "2025-06-18";
/// Starting a server and listing its tools; `npx` may download the server first.
const START_TIMEOUT: Duration = Duration::from_secs(90);
const CALL_TIMEOUT: Duration = Duration::from_secs(600);
/// Stderr kept to explain a server that failed to start.
const STDERR_TAIL: usize = 2000;
/// The longest tool name endpoints accept.
const MAX_NAME: usize = 64;
/// The most tools a request may offer: OpenAI's limit, which other endpoints share.
const MAX_TOOLS: usize = 128;
/// How often a call checks whether its turn was stopped.
const POLL: Duration = Duration::from_millis(200);

static DIR: OnceLock<PathBuf> = OnceLock::new();
static POOL: OnceLock<Mutex<HashMap<String, Arc<Entry>>>> = OnceLock::new();
/// Serializes changes to the config file.
static WRITE: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    /// Tools left out of API turns, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_tools: Vec<String>,
    /// Claude Code and Codex turns use Patchwork's copy instead of starting their own, for a
    /// server that cannot run twice.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub share: bool,
}

/// A server's environment and header values, kept in the credential store.
#[derive(Default, Serialize, Deserialize)]
struct Secrets {
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    #[serde(default)]
    mcp_servers: BTreeMap<String, McpServer>,
}

/// A server as the settings show it, without its environment and header values.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServerView {
    pub name: String,
    #[serde(flatten)]
    pub server: McpServer,
    /// `off`, `starting`, `ready`, or `error`.
    pub status: &'static str,
    pub tools: Vec<ToolInfo>,
    pub error: Option<String>,
    /// See `identity`.
    pub identity: String,
}

/// A server's tool, as the settings list it.
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
}

/// A tool as an API turn offers it to the model.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TurnTool {
    /// `mcp__server__tool`, cut to what endpoints accept.
    pub name: String,
    pub server: String,
    pub tool: String,
    pub description: String,
    pub parameters: Value,
    pub read_only: bool,
}

/// Sets the config folder and starts the enabled servers; called once when the server opens.
pub fn init(config_dir: &Path) {
    let _ = DIR.set(config_dir.to_path_buf());
    std::thread::spawn(|| {
        let _ = live_servers();
    });
    std::thread::spawn(|| {
        if let Err(e) = serve_relay() {
            log::warn!("MCP relay for the CLIs did not start: {e}");
        }
    });
}

/// Where Patchwork's own servers are configured.
pub fn path() -> PathBuf {
    DIR.get().cloned().unwrap_or_default().join(CONFIG_FILE)
}

fn read_config() -> Config {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn pool() -> std::sync::MutexGuard<'static, HashMap<String, Arc<Entry>>> {
    POOL.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

pub fn list() -> Vec<ServerView> {
    let servers = read_config().mcp_servers;
    let pool = pool();
    // A server added to the file by hand has not been started yet.
    if servers
        .iter()
        .any(|(name, s)| !s.disabled && !pool.contains_key(name))
    {
        restart_changed();
    }
    servers
        .into_iter()
        .map(|(name, mut server)| {
            let config = resolved(&name, &server);
            let identity = identity(&config);
            let running = pool.get(&name).filter(|e| e.config == config);
            for value in server.env.values_mut().chain(server.headers.values_mut()) {
                value.clear();
            }
            let (status, tools, error) = match running {
                _ if server.disabled => ("off", Vec::new(), None),
                None => ("starting", Vec::new(), None),
                Some(entry) => match entry.conn.try_lock().as_deref() {
                    Err(_) | Ok(None) => ("starting", Vec::new(), None),
                    Ok(Some(Ok(conn))) if !conn.alive() => {
                        ("error", Vec::new(), Some("The server exited".into()))
                    }
                    Ok(Some(Ok(conn))) => ("ready", conn.tool_infos(), None),
                    Ok(Some(Err(e))) => ("error", Vec::new(), Some(e.clone())),
                },
            };
            ServerView {
                name,
                server,
                status,
                tools,
                error,
                identity,
            }
        })
        .collect()
}

/// Adds or changes a server; `previous` is its old name when renamed.
pub fn save(name: String, previous: Option<String>, server: McpServer) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Give the server a name".into());
    }
    match (&server.command, &server.url) {
        (Some(c), None) if !c.trim().is_empty() => {}
        (None, Some(u)) if u.starts_with("http://") || u.starts_with("https://") => {}
        (None, Some(_)) => return Err("The URL must start with http:// or https://".into()),
        _ => return Err("Give a command or a URL".into()),
    }
    {
        let _guard = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
        let mut config = read_config();
        let old = previous.clone().unwrap_or_else(|| name.clone());
        // An empty value keeps the one the server has now, from the store or the file.
        let kept = config
            .mcp_servers
            .get(&old)
            .map(|s| resolved(&old, s))
            .unwrap_or_default();
        let mut server = server;
        let secrets = Secrets {
            env: take_values(&mut server.env, &kept.env),
            headers: take_values(&mut server.headers, &kept.headers),
        };
        if secrets.env.is_empty() && secrets.headers.is_empty() {
            super::api_config::delete_secret(&secret_name(&name));
        } else {
            let text = serde_json::to_string(&secrets).map_err(|e| e.to_string())?;
            super::api_config::store_secret(&secret_name(&name), &text)?;
        }
        if let Some(previous) = previous.filter(|p| *p != name) {
            config.mcp_servers.remove(&previous);
            pool().remove(&previous);
            super::api_config::delete_secret(&secret_name(&previous));
        }
        config.mcp_servers.insert(name, server);
        write_config(&config)?;
    }
    restart_changed();
    Ok(())
}

/// Empties each value, returning the values to store: the one given, or else the kept one.
fn take_values(
    values: &mut BTreeMap<String, String>,
    kept: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (key, value) in values.iter_mut() {
        let value = match std::mem::take(value) {
            v if v.is_empty() => kept.get(key).cloned().unwrap_or_default(),
            v => v,
        };
        if !value.is_empty() {
            out.insert(key.clone(), value);
        }
    }
    out
}

fn secret_name(name: &str) -> String {
    format!("mcp/{name}")
}

/// The server as it runs: its stored values filled in, and without the tools turned off or
/// the sharing, which change only who uses it, so changing them restarts nothing.
fn resolved(name: &str, server: &McpServer) -> McpServer {
    let mut server = McpServer {
        disabled_tools: Vec::new(),
        share: false,
        ..server.clone()
    };
    if server
        .env
        .values()
        .chain(server.headers.values())
        .any(String::is_empty)
    {
        let stored: Secrets = super::api_config::read_secret(&secret_name(name))
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        for (values, stored) in [
            (&mut server.env, stored.env),
            (&mut server.headers, stored.headers),
        ] {
            for (key, value) in values.iter_mut() {
                if value.is_empty()
                    && let Some(v) = stored.get(key)
                {
                    value.clone_from(v);
                }
            }
        }
    }
    server
}

pub fn remove(name: &str) -> Result<(), String> {
    let _guard = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
    let mut config = read_config();
    config.mcp_servers.remove(name);
    pool().remove(name);
    super::api_config::delete_secret(&secret_name(name));
    write_config(&config)
}

/// Stops a server and starts it again, e.g. after it failed.
pub fn restart(name: &str) {
    pool().remove(name);
    restart_changed();
}

fn write_config(config: &Config) -> Result<(), String> {
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    super::api_config::write_private(&path(), &text)
}

/// Starts, in the background, each enabled server that is not running as configured.
fn restart_changed() {
    std::thread::spawn(|| {
        let _ = live_servers();
    });
}

/// The enabled servers, each started if it is not yet, in parallel. A server that failed
/// stays failed until it is changed or restarted, so turns do not wait on it again.
fn live_servers() -> Vec<(String, Arc<Conn>)> {
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
        super::api_tools::definitions(false)
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

struct Entry {
    config: McpServer,
    /// None until started; held while starting, so callers wait for one start.
    conn: Mutex<Option<Result<Arc<Conn>, String>>>,
    /// Where the server's notifications go: the CLIs sharing it. Kept across restarts.
    notices: Notices,
}

/// Listeners for a server's notifications.
type Notices = Arc<Mutex<Vec<Sender<Value>>>>;

/// Passes a notification to every listener, forgetting those that have gone.
fn announce(notices: &Notices, notice: &Value) {
    notices
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|l| l.send(notice.clone()).is_ok());
}

impl Entry {
    fn connect(&self) -> Result<Arc<Conn>, String> {
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

struct Conn {
    transport: Transport,
    tools: Vec<Tool>,
    next_id: AtomicU64,
    /// The server's answer to `initialize`, given again to the CLIs that share it.
    info: Value,
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

    fn tool_infos(&self) -> Vec<ToolInfo> {
        self.tools
            .iter()
            .map(|t| ToolInfo {
                name: t.name.clone(),
                description: t.description.clone(),
            })
            .collect()
    }

    fn alive(&self) -> bool {
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
    fn request(
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
type Answer = mpsc::Receiver<Result<Value, String>>;
type Waiters = Arc<Mutex<HashMap<u64, Sender<Result<Value, String>>>>>;

struct StdioServer {
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    waiters: Waiters,
    stderr: Arc<Mutex<String>>,
}

impl StdioServer {
    fn spawn(
        command: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
        notices: Notices,
    ) -> Result<Self, String> {
        let mut cmd = super::user_command(command.trim());
        cmd.args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Its own process group, so stopping it also stops what it started (npx's node).
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Could not start {command}: {e}"))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("No stdin")?));
        let stdout = child.stdout.take().ok_or("No stdout")?;
        let stderr_pipe = child.stderr.take().ok_or("No stderr")?;
        let waiters: Waiters = Arc::default();
        let stderr: Arc<Mutex<String>> = Arc::default();
        {
            let stderr = stderr.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr_pipe).lines().map_while(Result::ok) {
                    let mut tail = stderr.lock().unwrap_or_else(PoisonError::into_inner);
                    tail.push_str(&line);
                    tail.push('\n');
                    if tail.len() > STDERR_TAIL {
                        let cut = tail.len() - STDERR_TAIL;
                        let cut = (cut..tail.len())
                            .find(|&i| tail.is_char_boundary(i))
                            .unwrap_or(0);
                        tail.drain(..cut);
                    }
                }
            });
        }
        {
            let (waiters, stdin) = (waiters.clone(), stdin.clone());
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let Ok(v) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    match (v.get("id"), v.get("method")) {
                        // A request from the server: answer pings, refuse the rest.
                        (Some(id), Some(method)) => {
                            let reply = if method == "ping" {
                                json!({"jsonrpc": "2.0", "id": id, "result": {}})
                            } else {
                                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Not supported"}})
                            };
                            let mut stdin = stdin.lock().unwrap_or_else(PoisonError::into_inner);
                            let _ = writeln!(stdin, "{reply}");
                            let _ = stdin.flush();
                        }
                        (Some(id), None) => {
                            let waiter = id.as_u64().and_then(|id| {
                                waiters
                                    .lock()
                                    .unwrap_or_else(PoisonError::into_inner)
                                    .remove(&id)
                            });
                            if let Some(waiter) = waiter {
                                let _ = waiter.send(Ok(v));
                            }
                        }
                        (None, Some(_)) => announce(&notices, &v),
                        _ => {}
                    }
                }
                // The server exited: wake everyone still waiting.
                waiters
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clear();
            });
        }
        Ok(StdioServer {
            child: Mutex::new(child),
            stdin,
            waiters,
            stderr,
        })
    }

    fn alive(&self) -> bool {
        matches!(
            self.child
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .try_wait(),
            Ok(None)
        )
    }

    fn send(&self, message: &Value) -> Result<(), String> {
        let mut stdin = self.stdin.lock().unwrap_or_else(PoisonError::into_inner);
        writeln!(stdin, "{message}")
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("The MCP server closed: {e}"))
    }

    fn send_request(&self, id: u64, message: &Value) -> Result<Answer, String> {
        let (tx, rx) = mpsc::channel();
        self.waiters
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, tx);
        if let Err(e) = self.send(message) {
            self.waiters
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
            return Err(e);
        }
        Ok(rx)
    }
}

impl Drop for StdioServer {
    fn drop(&mut self) {
        super::stop(self.child.get_mut().unwrap_or_else(PoisonError::into_inner));
    }
}

#[derive(Clone)]
struct HttpServer {
    url: Arc<str>,
    headers: Arc<BTreeMap<String, String>>,
    session: Arc<Mutex<Option<String>>>,
    http: ureq::Agent,
    /// Cleared when the server cannot be reached or has dropped the session, so the next
    /// call connects again.
    alive: Arc<AtomicBool>,
    notices: Notices,
}

impl HttpServer {
    fn new(url: &str, headers: &BTreeMap<String, String>, notices: Notices) -> Self {
        let http = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_global(Some(CALL_TIMEOUT))
            .http_status_as_error(false)
            .build()
            .into();
        HttpServer {
            url: url.into(),
            headers: Arc::new(headers.clone()),
            session: Arc::default(),
            http,
            alive: Arc::new(AtomicBool::new(true)),
            notices,
        }
    }

    fn post(&self, message: &Value) -> Result<ureq::http::Response<ureq::Body>, String> {
        let mut request = self
            .http
            .post(&*self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .header("MCP-Protocol-Version", PROTOCOL);
        if let Some(session) = self
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_deref()
        {
            request = request.header("Mcp-Session-Id", session);
        }
        for (name, value) in self.headers.iter() {
            request = request.header(name, value);
        }
        let response = request.send(message.to_string()).map_err(|e| {
            self.alive.store(false, Ordering::Relaxed);
            format!("Could not reach {}: {e}", self.url)
        })?;
        if let Some(session) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            *self.session.lock().unwrap_or_else(PoisonError::into_inner) = Some(session.into());
        }
        Ok(response)
    }

    /// Sends a request on its own thread, so the caller can stop waiting for it.
    fn send_request(&self, id: u64, message: Value) -> Answer {
        let (tx, rx) = mpsc::channel();
        let server = self.clone();
        std::thread::spawn(move || {
            let _ = tx.send(server.request(id, &message));
        });
        rx
    }

    /// Sends a request; the answer comes as JSON, or as the matching event of an SSE stream.
    fn request(&self, id: u64, message: &Value) -> Result<Value, String> {
        let mut response = self.post(message)?;
        let status = response.status().as_u16();
        // A server answers 404 to a session it has ended; a new one starts on the next call.
        if status == 404
            && self
                .session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
        {
            self.alive.store(false, Ordering::Relaxed);
            return Err("The MCP server ended its session".into());
        }
        if !response.status().is_success() {
            let text = response.body_mut().read_to_string().unwrap_or_default();
            let hint = if matches!(status, 401 | 403) {
                " Check the server's headers."
            } else {
                ""
            };
            return Err(format!(
                "The MCP server answered {status}: {}.{hint}",
                text.trim().chars().take(300).collect::<String>()
            ));
        }
        let sse = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.starts_with("text/event-stream"));
        let mut body = response.into_body().into_reader();
        if !sse {
            let mut text = String::new();
            body.read_to_string(&mut text).map_err(|e| e.to_string())?;
            return serde_json::from_str(&text)
                .map_err(|e| format!("The MCP server sent bad JSON: {e}"));
        }
        let mut data = String::new();
        for line in BufReader::new(body).lines() {
            let line = line.map_err(|e| format!("The MCP server's reply broke off: {e}"))?;
            if let Some(chunk) = line.strip_prefix("data:") {
                data.push_str(chunk.trim_start());
            } else if line.is_empty()
                && !data.is_empty()
                && let Ok(v) = serde_json::from_str::<Value>(&std::mem::take(&mut data))
            {
                match (v.get("id"), v.get("method")) {
                    (Some(_), None) if v["id"].as_u64() == Some(id) => return Ok(v),
                    (None, Some(_)) => announce(&self.notices, &v),
                    _ => {}
                }
            }
        }
        Err("The MCP server ended its reply without an answer".into())
    }
}

// ---- Sharing servers with the CLIs ----------------------------------------------------------
//
// Claude Code and Codex would start their own copy of a server Patchwork already runs. Most
// servers run fine twice, and each copy keeps its client's session apart, but some fight over a
// port, a lock, or a program they drive. For a server the user sets to be shared, CLI turns
// instead start Patchwork's binary as a relay (`--mcp-proxy`), which passes the CLI's messages
// to the copy Patchwork runs. The relay reaches Patchwork over a local socket; the port and a token that
// proves the caller may use it are in a file only the user can read.

/// The argument that starts Patchwork's binary as a relay to one of Patchwork's servers.
pub const PROXY_ARG: &str = "--mcp-proxy";
const RELAY_FILE: &str = "mcp-relay.json";
static RELAY_READY: AtomicBool = AtomicBool::new(false);

/// Answers a permission hook's request (see `super::hook`).
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

fn serve_relay() -> Result<(), String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let text = json!({"port": port, "token": token}).to_string();
    super::api_config::write_private(&path().with_file_name(RELAY_FILE), &text)?;
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
    let port = relay["port"].as_u64().unwrap_or(0) as u16;
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

/// Patchwork's enabled servers set to be shared that run as a command, as they run, for a
/// CLI's servers to be matched against.
pub fn shared_servers() -> Vec<(String, McpServer)> {
    read_config()
        .mcp_servers
        .into_iter()
        .filter(|(_, s)| s.share && !s.disabled && s.command.is_some())
        .map(|(name, s)| {
            let server = resolved(&name, &s);
            (name, server)
        })
        .collect()
}

/// What makes a server the same server wherever it is configured: how it starts, or where
/// it is, and what it is given, as a short hash that stays the same across builds.
pub fn identity(server: &McpServer) -> String {
    let text = json!({
        "command": server.command.as_deref().map(str::trim),
        "args": server.args,
        "env": server.env,
        "url": server.url,
        "headers": server.headers,
    })
    .to_string();
    // FNV-1a, 64-bit.
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// Whether a CLI's server is the same as one of Patchwork's: the same command, arguments,
/// and environment, so it would be a second copy of it.
pub fn same_server(cli: &McpServer, ours: &McpServer) -> bool {
    let command = |s: &McpServer| s.command.as_deref().map(str::trim).map(String::from);
    command(cli).is_some()
        && command(cli) == command(ours)
        && cli.args == ours.args
        && cli.env == ours.env
}
