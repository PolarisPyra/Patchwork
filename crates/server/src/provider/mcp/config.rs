//! The servers in `mcp/`: reading and saving them, their stored secrets, and matching a CLI's servers against them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use super::{CONFIG_DIR, DIR, live_servers, pool, serve_relay, WRITE};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "provider.ts")]
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

#[derive(Default)]
pub(super) struct Config {
    pub(super) mcp_servers: BTreeMap<String, McpServer>,
}

/// A server as the settings show it, without its environment and header values.
#[derive(Serialize, TS)]
#[ts(export, export_to = "provider.ts")]
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
    /// The file it is configured in.
    pub file: String,
}

/// A server's tool, as the settings list it.
#[derive(Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "provider.ts")]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
}

/// A tool as an API turn offers it to the model.
#[derive(Serialize, TS)]
#[ts(export, export_to = "provider.ts")]
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

/// The folder Patchwork's own servers are configured in, a file each.
pub fn dir() -> PathBuf {
    DIR.get().cloned().unwrap_or_default().join(CONFIG_DIR)
}

fn file(name: &str) -> PathBuf {
    dir().join(format!("{name}.json"))
}

/// Each `.json` file in `mcp/`, by its name without `.json`; a file that is not valid JSON is
/// left out.
pub(super) fn read_config() -> Config {
    let mut mcp_servers = BTreeMap::new();
    for entry in std::fs::read_dir(dir()).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|n| crate::system::fs::is_plain_file_name(n))
        else {
            continue;
        };
        if let Some(server) = crate::system::fs::read_json_or_default::<Option<McpServer>>(&path) {
            mcp_servers.insert(name.to_string(), server);
        }
    }
    Config { mcp_servers }
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
            let file = file(&name).display().to_string();
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
                file,
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
    if !crate::system::fs::is_plain_file_name(&name) {
        return Err("The name cannot start with a dot or have any of / \\ : * ? \" < > |".into());
    }
    match (&server.command, &server.url) {
        (Some(c), None) if !c.trim().is_empty() => {}
        (None, Some(u)) if u.starts_with("http://") || u.starts_with("https://") => {}
        (None, Some(_)) => return Err("The URL must start with http:// or https://".into()),
        _ => return Err("Give a command or a URL".into()),
    }
    {
        let _guard = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
        let old = previous.clone().unwrap_or_else(|| name.clone());
        // An empty value keeps the one the server has now, from the store or the file.
        let kept = read_config()
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
            crate::provider::api::config::delete_secret(&secret_name(&name));
        } else {
            let text = serde_json::to_string(&secrets).map_err(|e| e.to_string())?;
            crate::provider::api::config::store_secret(&secret_name(&name), &text)?;
        }
        write_server(&name, &server)?;
        if let Some(previous) = previous.filter(|p| *p != name) {
            remove_file(&previous)?;
            pool().remove(&previous);
            crate::provider::api::config::delete_secret(&secret_name(&previous));
        }
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
pub(super) fn resolved(name: &str, server: &McpServer) -> McpServer {
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
        let stored: Secrets = crate::provider::api::config::read_secret(&secret_name(name))
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
    remove_file(name)?;
    pool().remove(name);
    crate::provider::api::config::delete_secret(&secret_name(name));
    Ok(())
}

/// Stops a server and starts it again, e.g. after it failed.
pub fn restart(name: &str) {
    pool().remove(name);
    restart_changed();
}

fn write_server(name: &str, server: &McpServer) -> Result<(), String> {
    let text = serde_json::to_string_pretty(server).map_err(|e| e.to_string())?;
    crate::system::fs::write_private(&file(name), &text)
}

fn remove_file(name: &str) -> Result<(), String> {
    match std::fs::remove_file(file(name)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// Starts, in the background, each enabled server that is not running as configured.
pub(super) fn restart_changed() {
    std::thread::spawn(|| {
        let _ = live_servers();
    });
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
