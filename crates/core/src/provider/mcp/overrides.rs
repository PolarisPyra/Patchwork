//! The arguments that hand a CLI Patchwork's own server and the servers it shares, for one turn.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;
use serde_json::Value;
use super::clis::{claude_servers, codex_servers};
use crate::provider::mcp::{self};
use crate::projects::Project;
use crate::provider::AgentKind;

/// How long an agent waits on one of Patchwork's own tools, which may wait an hour for the user.
const OWN_TIMEOUT_SECS: u64 = 65 * 60;

/// The command and arguments that reach Patchwork's own server for the agent of `chat_id`.
pub fn own_server(chat_id: &str) -> Option<(String, Vec<String>)> {
    let relay = mcp::own_relay_file()?;
    let exe = std::env::var_os("APPIMAGE")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| std::env::current_exe().ok())?;
    let args = [
        mcp::PROXY_ARG,
        &relay.display().to_string(),
        mcp::OWN_SERVER,
        chat_id,
    ];
    Some((exe.display().to_string(), args.map(String::from).into()))
}

/// OpenCode's config for Patchwork's own server, for the agent of `chat_id`.
#[must_use]
pub fn opencode_config(chat_id: &str) -> Option<Value> {
    let (exe, args) = own_server(chat_id)?;
    let command: Vec<String> = std::iter::once(exe).chain(args).collect();
    Some(serde_json::json!({"mcp": {mcp::OWN_SERVER: {
        "type": "local",
        "command": command,
        "enabled": true,
        "timeout": OWN_TIMEOUT_SECS * 1000,
    }}}))
}

/// Builds the arguments that give a CLI turn Patchwork's MCP server.
///
/// Arguments that give a CLI's turn Patchwork's own server, and point it at Patchwork's copy of
/// each server it shares, so the two do not each start one: `--mcp-config` replaces a Claude
/// Code server of the same name, and `-c` a Codex one.
#[must_use]
pub fn overrides(agent: AgentKind, root: &str, chat_id: &str) -> Vec<String> {
    let Some((exe, own)) = own_server(chat_id) else {
        return Vec::new();
    };
    let mut relays = shared_relays(agent, root);
    relays.insert(mcp::OWN_SERVER.to_string(), own);
    let tool = crate::vault::requests::TOOL;
    match agent {
        AgentKind::Claude => {
            let servers: serde_json::Map<String, Value> = relays
                .into_iter()
                .map(|(name, args)| {
                    let mut entry = serde_json::json!({"command": exe, "args": args});
                    if name == mcp::OWN_SERVER {
                        entry["timeout"] = (OWN_TIMEOUT_SECS * 1000).into();
                    }
                    (name, entry)
                })
                .collect();
            vec![
                "--mcp-config".into(),
                serde_json::json!({"mcpServers": servers}).to_string(),
                // Asking for a secret is the user's to answer, a sub agent on another agent is what
                // the user set the chat to start, and Android devices are used only once the user
                // has allowed it in Settings, and a quilt only saves a page the user can go
                // back from, so none is asked about first.
                "--allowedTools".into(),
                [
                    tool.to_string(),
                    crate::chats::delegate::TOOL.to_string(),
                    crate::quilts::TOOL.to_string(),
                ]
                .into_iter()
                .chain(crate::android::tool_names())
                .map(|tool| format!("mcp__{}__{tool}", mcp::OWN_SERVER))
                .collect::<Vec<_>>()
                .join(","),
            ]
        }
        AgentKind::Codex => relays
            .into_iter()
            // `-c` splits its key on dots, so a name must not hold one.
            .filter(|(name, _)| {
                name.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            })
            .flat_map(|(name, args)| {
                // JSON strings and arrays of strings are also TOML.
                let mut config = vec![
                    "-c".to_string(),
                    format!("mcp_servers.{name}.command={}", Value::from(exe.clone())),
                    "-c".to_string(),
                    format!("mcp_servers.{name}.args={}", Value::from(args)),
                ];
                if name == mcp::OWN_SERVER {
                    config.extend([
                        "-c".to_string(),
                        format!("mcp_servers.{name}.tool_timeout_sec={OWN_TIMEOUT_SECS}"),
                        "-c".to_string(),
                        format!("mcp_servers.{name}.default_tools_approval_mode=\"approve\""),
                        // Codex otherwise leaves MCP tools to be found from inside its `exec`
                        // code, where the model never learns of `request_secret`.
                        "-c".to_string(),
                        format!("features.code_mode.direct_only_tool_namespaces=[\"mcp__{name}\"]"),
                    ]);
                }
                config
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Builds the arguments that give a dispatcher's Claude turn Patchwork's server and no other.
///
/// Every tool the server lists for the chat is allowed up front: they act inside Patchwork,
/// which asks the person itself where a tool needs their word.
#[must_use]
pub fn dispatcher_overrides(chat_id: &str) -> Vec<String> {
    let Some((exe, args)) = own_server(chat_id) else {
        return Vec::new();
    };
    let server = serde_json::json!({
        "command": exe, "args": args, "timeout": OWN_TIMEOUT_SECS * 1000,
    });
    let tools: Vec<String> = mcp::own_turn_tools(chat_id)
        .into_iter()
        .map(|tool| tool.tool)
        .chain([crate::vault::requests::TOOL.to_string()])
        .map(|tool| format!("mcp__{}__{tool}", mcp::OWN_SERVER))
        .collect();
    vec![
        "--mcp-config".into(),
        serde_json::json!({"mcpServers": {mcp::OWN_SERVER: server}}).to_string(),
        "--strict-mcp-config".into(),
        "--allowedTools".into(),
        tools.join(","),
    ]
}

/// The relay arguments for each server the CLI shares with Patchwork, by the CLI's name for it.
fn shared_relays(agent: AgentKind, root: &str) -> BTreeMap<String, Vec<String>> {
    let Some(relay) = mcp::own_relay_file() else {
        return BTreeMap::new();
    };
    let shared = mcp::shared_servers();
    if shared.is_empty() {
        return BTreeMap::new();
    }
    let project = Project {
        id: String::new(),
        name: String::new(),
        path: root.to_string(),
    };
    let found = match agent {
        AgentKind::Claude => claude_servers(
            &crate::provider::claude::settings_file(),
            std::slice::from_ref(&project),
        ),
        AgentKind::Codex => codex_servers(
            &crate::provider::codex::config_file(),
            std::slice::from_ref(&project),
        ),
        _ => return BTreeMap::new(),
    };
    found
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
        .collect()
}
