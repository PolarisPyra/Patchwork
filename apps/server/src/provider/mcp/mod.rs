//! MCP servers for API providers. The servers live in `mcp-servers.json` in the config folder,
//! in the `mcpServers` shape other MCP clients use, so a server's README snippet pastes in as
//! is. Each enabled server starts once, when Patchwork opens, and stays up: every API turn
//! gets its tools without waiting, and a server keeps its state (a browser, a login) between
//! turns. A server runs as a command speaking JSON-RPC over stdio, or at a URL over MCP's
//! streamable HTTP. Environment and header values go to the system credential store, as API
//! keys do, and the file keeps only their names; a value typed into the file by hand is used
//! as is until the server is next saved.

pub mod clis;
mod config;
mod pool;
mod stdio;
mod http;
mod relay;

pub use config::{
    McpServer, ServerView, ToolInfo, TurnTool, init, path, list, save, remove, restart,
    shared_servers, identity, same_server,
};
use config::{read_config, resolved, restart_changed};
pub use pool::{tool_budget, turn_tools, probe, call};
use pool::{pool, live_servers, Entry, Notices, announce, Answer, Waiters};
use stdio::{StdioServer};
use http::{HttpServer};
pub use relay::{PROXY_ARG, set_hook_handler, relay_endpoint, relay_file, run_proxy};
use relay::serve_relay;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

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
