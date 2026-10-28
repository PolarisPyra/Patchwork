//! The MCP servers Claude Code and Codex use, read from their own config, so the settings can
//! show them beside Patchwork's. Those CLIs start their servers themselves; Patchwork only
//! starts each one briefly to list its tools, and keeps the list per config, so a server is
//! started again only when its config changes or the user refreshes it. A project's own
//! servers come with the project, so one runs only once the CLI has approved it: Claude Code
//! asks before using a server from a project's `.mcp.json`, and Codex reads a project's
//! `.codex/config.toml` only in a project the user trusts.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use super::mcp::{self, McpServer, ToolInfo};
use crate::types::{AgentKind, Project};

/// Tool lists that were read, kept across runs so opening the settings starts nothing.
const CACHE_FILE: &str = "mcp-cli-tools.json";

/// Each server's tools by the hash of its config: None while they are being read.
type Listings = HashMap<String, Option<Result<Vec<ToolInfo>, String>>>;
static TOOLS: OnceLock<Mutex<Listings>> = OnceLock::new();

/// One CLI's servers, and the file that holds its own servers.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CliGroup {
    pub agent: AgentKind,
    pub file: String,
    pub servers: Vec<CliServer>,
}

/// A server as the settings show it. Environment and header values stay out, since they may
/// hold secrets; only their names are shown.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CliServer {
    /// Its identity (see `mcp::identity`), which names it for a refresh, and puts it on the
    /// same card as Patchwork's or the other CLI's copy of it.
    pub key: String,
    pub name: String,
    /// `user`, `project`, or `local` (Claude Code's own setting for one project).
    pub scope: &'static str,
    /// The Patchwork project a project or local server belongs to.
    pub project: Option<String>,
    /// The file it is defined in.
    pub file: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub url: Option<String>,
    pub env: Vec<String>,
    /// `off`, `unapproved` (a project's server the CLI has not approved), `listing`, `ready`,
    /// `signin` (the server wants the CLI's own sign-in), or `error`.
    pub status: &'static str,
    /// Its turns in Patchwork use Patchwork's copy of it, through the relay.
    pub relayed: bool,
    pub tools: Vec<ToolInfo>,
    pub error: Option<String>,
}

/// One server read from a CLI's config.
struct Found {
    name: String,
    scope: &'static str,
    project: Option<String>,
    file: PathBuf,
    server: McpServer,
    /// The CLI would run it: always for the user's own servers, and for a project's once the
    /// CLI has approved it.
    approved: bool,
}

pub fn list(projects: &[Project]) -> Vec<CliGroup> {
    let claude = claude_file();
    let codex = super::codex::home().unwrap_or_default().join("config.toml");
    let shared = mcp::shared_servers();
    [
        (
            AgentKind::Claude,
            claude.clone(),
            claude_servers(&claude, projects),
        ),
        (
            AgentKind::Codex,
            codex.clone(),
            codex_servers(&codex, projects),
        ),
    ]
    .into_iter()
    .map(|(agent, file, found)| CliGroup {
        agent,
        file: file.display().to_string(),
        servers: found.into_iter().map(|f| view(f, &shared)).collect(),
    })
    .collect()
}

/// Reads the tools of the server with this key again.
pub fn refresh(key: &str, projects: &[Project]) {
    cache().remove(key);
    for found in all(projects)
        .into_iter()
        .filter(|f| mcp::identity(&f.server) == key)
    {
        view(found, &[]);
    }
}

/// Adds the server with this key to Patchwork's own, under its name, for API tasks.
pub fn enable(key: &str, projects: &[Project]) -> Result<(), String> {
    let found = all(projects)
        .into_iter()
        .find(|f| mcp::identity(&f.server) == key)
        .ok_or("That server is no longer in the CLI's config")?;
    if !found.approved {
        return Err("The CLI has not approved this project's server".into());
    }
    if mcp::list().iter().any(|s| s.name == found.name) {
        return Err(format!(
            "Patchwork already has a server named {}",
            found.name
        ));
    }
    mcp::save(
        found.name,
        None,
        McpServer {
            disabled: false,
            ..found.server
        },
    )
}

/// Whether `path` is a config file a server was read from, which the settings may open.
pub fn is_config_file(path: &Path, projects: &[Project]) -> bool {
    path == mcp::path()
        || path == claude_file()
        || path == super::codex::home().unwrap_or_default().join("config.toml")
        || all(projects).iter().any(|f| f.file == path)
}

fn all(projects: &[Project]) -> Vec<Found> {
    let codex = super::codex::home().unwrap_or_default().join("config.toml");
    let mut found = claude_servers(&claude_file(), projects);
    found.extend(codex_servers(&codex, projects));
    found
}

fn cache() -> std::sync::MutexGuard<'static, Listings> {
    TOOLS
        .get_or_init(|| {
            let saved: HashMap<String, Vec<ToolInfo>> = std::fs::read_to_string(cache_file())
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            Mutex::new(saved.into_iter().map(|(k, v)| (k, Some(Ok(v)))).collect())
        })
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn cache_file() -> PathBuf {
    mcp::path().with_file_name(CACHE_FILE)
}

/// Saves the tool lists that were read; failures are not kept, so they are tried again.
fn save_cache(tools: &Listings) {
    let ok: HashMap<&String, &Vec<ToolInfo>> = tools
        .iter()
        .filter_map(|(k, v)| match v {
            Some(Ok(list)) => Some((k, list)),
            _ => None,
        })
        .collect();
    if let Ok(text) = serde_json::to_string(&ok) {
        let _ = super::api_config::write_private(&cache_file(), &text);
    }
}

/// The view of a server, starting to read its tools if they are not known yet.
fn view(found: Found, shared: &[(String, McpServer)]) -> CliServer {
    let relayed = !found.server.disabled
        && found.approved
        && mcp::relay_file().is_some()
        && shared
            .iter()
            .any(|(_, s)| mcp::same_server(&found.server, s));
    let key = mcp::identity(&found.server);
    let state = if found.server.disabled || !found.approved {
        None
    } else {
        let mut tools = cache();
        let state = tools.get(&key).cloned();
        if state.is_none() {
            tools.insert(key.clone(), None);
            let (key, server) = (key.clone(), found.server.clone());
            std::thread::spawn(move || {
                let result = mcp::probe(&server);
                let mut tools = cache();
                tools.insert(key, Some(result));
                save_cache(&tools);
            });
        }
        Some(state.flatten())
    };
    let (status, tools, error) = match state {
        None if !found.approved => ("unapproved", Vec::new(), None),
        None => ("off", Vec::new(), None),
        Some(None) => ("listing", Vec::new(), None),
        Some(Some(Ok(tools))) => ("ready", tools, None),
        Some(Some(Err(e)))
            if found.server.url.is_some()
                && (e.contains("answered 401") || e.contains("answered 403")) =>
        {
            ("signin", Vec::new(), None)
        }
        Some(Some(Err(e))) => ("error", Vec::new(), Some(e)),
    };
    let names = |map: &BTreeMap<String, String>| map.keys().cloned().collect::<Vec<_>>();
    CliServer {
        key,
        name: found.name,
        scope: found.scope,
        project: found.project,
        file: found.file.display().to_string(),
        command: found.server.command.clone(),
        args: found.server.args.clone(),
        url: found.server.url.clone(),
        env: if found.server.url.is_some() {
            names(&found.server.headers)
        } else {
            names(&found.server.env)
        },
        status,
        relayed,
        tools,
        error,
    }
}

/// A variable from Patchwork's environment or the login shell's, which is read once, since
/// the settings list the servers again while their tools are being read.
fn env_var(name: &str) -> Option<String> {
    static VARS: Mutex<BTreeMap<String, Option<String>>> = Mutex::new(BTreeMap::new());
    let mut vars = VARS.lock().unwrap_or_else(PoisonError::into_inner);
    vars.entry(name.into())
        .or_insert_with(|| std::env::var(name).ok().or_else(|| super::shell_var(name)))
        .clone()
}

/// Claude Code's settings file: `.claude.json` in `CLAUDE_CONFIG_DIR`, or in the home folder.
fn claude_file() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir).join(".claude.json"),
        None => super::home_dir().unwrap_or_default().join(".claude.json"),
    }
}

/// Claude Code's servers: its user servers, each project's local ones in `.claude.json`, and
/// the project's shared `.mcp.json`. A project server turned off in Claude Code shows as off.
fn claude_servers(file: &Path, projects: &[Project]) -> Vec<Found> {
    let config: Value = std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    // Approvals of project servers from the user's own settings; a project's settings files
    // come with the project, so they cannot approve its servers.
    let user_settings: Value = super::claude::home()
        .map(|home| home.join("settings.json"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let mut out = Vec::new();
    type Check<'a> = &'a dyn Fn(&str) -> bool;
    let mut add = |map: &Value,
                   scope,
                   project: Option<&Project>,
                   file: &Path,
                   off: Check,
                   approved: Check| {
        for (name, server) in map.as_object().into_iter().flatten() {
            let Ok(mut server) = serde_json::from_value::<McpServer>(server.clone()) else {
                continue;
            };
            server.disabled = off(name);
            out.push(Found {
                name: name.clone(),
                scope,
                project: project.map(|p| p.name.clone()),
                file: file.to_path_buf(),
                server,
                approved: approved(name),
            });
        }
    };
    let all = |_: &str| true;
    add(&config["mcpServers"], "user", None, file, &|_| false, &all);
    for project in projects {
        let settings = &config["projects"][&project.path];
        let listed = |key: &str, name: &str| {
            settings[key]
                .as_array()
                .is_some_and(|names| names.iter().any(|n| n == name))
        };
        let off = |name: &str| {
            listed("disabledMcpServers", name) || listed("disabledMcpjsonServers", name)
        };
        add(
            &settings["mcpServers"],
            "local",
            Some(project),
            file,
            &off,
            &all,
        );
        let approved = |name: &str| {
            let names = |v: &Value| {
                v.as_array()
                    .is_some_and(|names| names.iter().any(|n| n == name))
            };
            settings["enableAllProjectMcpServers"] == true
                || names(&settings["enabledMcpjsonServers"])
                || user_settings["enableAllProjectMcpServers"] == true
                || names(&user_settings["enabledMcpjsonServers"])
        };
        let shared = Path::new(&project.path).join(".mcp.json");
        if let Some(json) = std::fs::read_to_string(&shared)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        {
            add(
                &json["mcpServers"],
                "project",
                Some(project),
                &shared,
                &off,
                &approved,
            );
        }
    }
    out
}

/// Codex's servers: `[mcp_servers.*]` in its `config.toml`, and in each project's
/// `.codex/config.toml`, which Codex reads only in a project the user trusts.
fn codex_servers(file: &Path, projects: &[Project]) -> Vec<Found> {
    let user: Option<toml::Value> = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| toml::from_str(&t).ok());
    let trusted = |path: &str| {
        let level = user.as_ref().and_then(|u| {
            u.get("projects")?
                .get(path)?
                .get("trust_level")?
                .as_str()
                .map(String::from)
        });
        level.as_deref() == Some("trusted")
    };
    let mut out = codex_file(file, "user", None, true);
    for project in projects {
        let shared = Path::new(&project.path).join(".codex/config.toml");
        // A project in the home folder has the user's own file as its project file.
        if shared == file {
            continue;
        }
        out.extend(codex_file(
            &shared,
            "project",
            Some(project),
            trusted(&project.path),
        ));
    }
    out
}

fn codex_file(
    file: &Path,
    scope: &'static str,
    project: Option<&Project>,
    approved: bool,
) -> Vec<Found> {
    let Some(config) = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| toml::from_str::<toml::Value>(&t).ok())
    else {
        return Vec::new();
    };
    let strings = |v: Option<&toml::Value>| -> BTreeMap<String, String> {
        v.and_then(toml::Value::as_table)
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
            .collect()
    };
    let Some(servers) = config.get("mcp_servers").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    servers
        .iter()
        .map(|(name, s)| {
            let text = |key: &str| s.get(key).and_then(toml::Value::as_str).map(String::from);
            let mut headers = strings(s.get("http_headers"));
            // Codex reads the bearer token from a variable it names; the settings need it only
            // to list the tools.
            if let Some(var) = text("bearer_token_env_var")
                && let Some(token) = env_var(&var)
            {
                headers.insert("Authorization".into(), format!("Bearer {token}"));
            }
            let server = McpServer {
                command: text("command"),
                args: s
                    .get("args")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|a| a.as_str().map(String::from))
                    .collect(),
                env: strings(s.get("env")),
                url: text("url"),
                headers,
                disabled: s.get("enabled").and_then(toml::Value::as_bool) == Some(false),
                ..Default::default()
            };
            Found {
                name: name.clone(),
                scope,
                project: project.map(|p| p.name.clone()),
                file: file.to_path_buf(),
                server,
                approved,
            }
        })
        .collect()
}

/// Arguments that point a CLI's turn at Patchwork's copy of each server it shares, so the two
/// do not each start one: `--mcp-config` replaces a Claude Code server of the same name, and
/// `-c` a Codex one.
pub fn overrides(agent: AgentKind, root: &str) -> Vec<String> {
    let (Some(relay), Ok(exe)) = (mcp::relay_file(), std::env::current_exe()) else {
        return Vec::new();
    };
    let shared = mcp::shared_servers();
    if shared.is_empty() {
        return Vec::new();
    }
    let project = Project {
        id: String::new(),
        name: String::new(),
        path: root.to_string(),
    };
    let found = match agent {
        AgentKind::Claude => claude_servers(&claude_file(), std::slice::from_ref(&project)),
        AgentKind::Codex => codex_servers(
            &super::codex::home().unwrap_or_default().join("config.toml"),
            std::slice::from_ref(&project),
        ),
        _ => return Vec::new(),
    };
    let relays: BTreeMap<String, Vec<String>> = found
        .into_iter()
        .filter(|f| !f.server.disabled && f.approved)
        .filter_map(|f| {
            let (ours, _) = shared
                .iter()
                .find(|(_, s)| mcp::same_server(&f.server, s))?;
            let args = vec![
                mcp::PROXY_ARG.to_string(),
                relay.display().to_string(),
                ours.clone(),
            ];
            Some((f.name, args))
        })
        .collect();
    if relays.is_empty() {
        return Vec::new();
    }
    let exe = exe.display().to_string();
    match agent {
        AgentKind::Claude => {
            let servers: serde_json::Map<String, Value> = relays
                .into_iter()
                .map(|(name, args)| (name, serde_json::json!({"command": exe, "args": args})))
                .collect();
            vec![
                "--mcp-config".into(),
                serde_json::json!({"mcpServers": servers}).to_string(),
            ]
        }
        _ => relays
            .into_iter()
            // `-c` splits its key on dots, so a name must not hold one.
            .filter(|(name, _)| {
                name.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            })
            .flat_map(|(name, args)| {
                // JSON strings and arrays of strings are also TOML.
                [
                    "-c".to_string(),
                    format!("mcp_servers.{name}.command={}", Value::from(exe.clone())),
                    "-c".to_string(),
                    format!("mcp_servers.{name}.args={}", Value::from(args)),
                ]
            })
            .collect(),
    }
}
