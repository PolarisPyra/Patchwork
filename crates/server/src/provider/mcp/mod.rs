//! MCP servers for API providers. Each server is a file in `mcp/` in the config folder, named
//! after it, holding the entry other MCP clients list under `mcpServers`, so a server's README
//! snippet pastes in as is. Each enabled server starts once, when Patchwork opens, and stays
//! up: every API turn gets its tools without waiting, and a server keeps its state (a browser,
//! a login) between turns. A server runs as a command speaking JSON-RPC over stdio, or at a URL
//! over MCP's streamable HTTP. Environment and header values go to the system credential store, as API
//! keys do, and the file keeps only their names; a value typed into the file by hand is used
//! as is until the server is next saved.

pub mod clis;
mod config;
mod pool;
mod stdio;
mod http;
mod relay;

pub use config::{
    McpServer, ServerView, ToolInfo, TurnTool, init, dir, list, save, remove, restart,
    shared_servers, identity, same_server,
};
use config::{
    CALL_TIMEOUT, MAX_NAME, MAX_TOOLS, POLL, PROTOCOL, START_TIMEOUT, STDERR_TAIL, read_config,
    resolved, restart_changed,
};
pub use pool::{tool_budget, turn_tools, probe, call};
use pool::{pool, live_servers, Notices, announce, Answer, Waiters};
use stdio::{StdioServer};
use http::{HttpServer};
pub use relay::{
    OWN_SERVER, PROXY_ARG, call_own, relay_endpoint, relay_file, run_proxy, set_hook_handler,
    set_tool_handler,
};
use relay::serve_relay;
