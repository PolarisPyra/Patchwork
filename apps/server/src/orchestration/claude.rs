//! Launch arguments for Claude Code turns.

use std::path::Path;

use crate::agents::{self, Agent};
use crate::permissions::Guarded;
use crate::provider::api_tools::resolve;
use crate::types::{AgentKind, PermissionKind, Settings};

/// Arguments that append the checklist and shell instructions to Claude's system prompt, and load
/// Patchwork's skills as a plugin.
pub fn launch_args(settings: &Settings) -> Vec<String> {
    let mut args = vec![
        "--append-system-prompt".to_string(),
        super::agent_instructions(settings, AgentKind::Claude),
    ];
    if let Some(plugin) = crate::skills::claude_plugin(settings) {
        args.extend([
            "--plugin-dir".to_string(),
            plugin.to_string_lossy().into_owned(),
        ]);
    }
    args
}

/// `--agents` with the custom agents, so Claude can start them as subagents by name. They go
/// in a file, since a long list could pass the size an argument may have.
pub fn agents_args(agents: &[Agent], root: &str) -> Vec<String> {
    if agents.is_empty() {
        return Vec::new();
    }
    let models = crate::provider::claude::models();
    let ids = || models.iter().filter_map(|m| m.id.as_deref());
    let list: serde_json::Map<String, serde_json::Value> = agents
        .iter()
        .map(|agent| {
            let mut entry =
                serde_json::json!({ "description": agent.about(), "prompt": agent.prompt.trim() });
            // Claude can't run another CLI's model, so the agent takes the chat's.
            if let Some(model) = agent
                .model
                .as_deref()
                .and_then(|m| agents::offered(m, ids()))
            {
                entry["model"] = model.into();
            }
            (agent.name.clone(), entry)
        })
        .collect();
    match agents::launch_file(
        "claude",
        root,
        "agents.json",
        &serde_json::Value::Object(list).to_string(),
    ) {
        Ok(file) => vec!["--agents".to_string(), file.to_string_lossy().into_owned()],
        Err(e) => {
            log::warn!("failed to hand Claude the custom agents: {e}");
            Vec::new()
        }
    }
}

/// Environment for Claude's process. The claude.ai connectors are left out, since their tool
/// lists and instructions are added to the context again whenever a session resumes. A set
/// compact window makes Claude compact once its context reaches it.
pub fn launch_env(settings: &Settings) -> Vec<(&'static str, String)> {
    let mut env = vec![("ENABLE_CLAUDEAI_MCP_SERVERS", "false".to_string())];
    let window = settings.tokens.claude_compact_window;
    if window > 0 {
        env.push(("CLAUDE_CODE_AUTO_COMPACT_WINDOW", window.to_string()));
    }
    env
}

/// Patchwork's deny and ask rules as Claude ask rules, in `--settings`, so Claude asks Patchwork
/// before a call they cover in every mode, bypassing permissions too, and Patchwork answers it by
/// its own rules. Empty when no rule guards anything Claude can name.
pub fn permission_args(guarded: &[Guarded], root: &str) -> Vec<String> {
    let root = Path::new(root);
    let ask: Vec<String> = guarded.iter().flat_map(|g| claude_rules(g, root)).collect();
    // No rule names every MCP tool, so a hook asks about them all.
    let every_mcp = guarded
        .iter()
        .any(|g| g.kind == PermissionKind::Mcp && g.all());
    if ask.is_empty() && !every_mcp {
        return Vec::new();
    }
    let mut settings = serde_json::json!({ "permissions": { "ask": ask } });
    if every_mcp {
        let answer =
            r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask"}}"#;
        settings["hooks"] = serde_json::json!({ "PreToolUse": [{
            "matcher": "mcp__.*",
            "hooks": [{ "type": "command", "command": format!("printf %s '{answer}'") }],
        }] });
    }
    vec!["--settings".into(), settings.to_string()]
}

/// One kind's guarded patterns in Claude's rule syntax.
fn claude_rules(guarded: &Guarded, root: &Path) -> Vec<String> {
    let all = guarded.all();
    let patterns = guarded.patterns().filter(|p| *p != "*");
    match guarded.kind {
        PermissionKind::Command if all => vec!["Bash".into()],
        PermissionKind::Command => patterns
            .flat_map(|p| [format!("Bash({p})"), format!("Bash({p} *)")])
            .collect(),
        PermissionKind::Edit if all => vec!["Edit".into()],
        PermissionKind::Edit => patterns
            .map(|p| format!("Edit({})", folder_rule(p, root)))
            .collect(),
        // Every read asks, and Patchwork runs those in the thread's own folder at once.
        PermissionKind::Folder if all => vec!["Read".into()],
        PermissionKind::Folder => patterns
            .filter(|p| *p != ".")
            .map(|p| format!("Read({})", folder_rule(p, root)))
            .collect(),
        PermissionKind::Web if all => vec!["WebFetch".into()],
        PermissionKind::Web => patterns
            .map(|p| format!("WebFetch(domain:{})", p.trim_start_matches("*.")))
            .collect(),
        // Claude has no screen tool of its own; Patchwork's rules still decide before it looks.
        PermissionKind::Screen => Vec::new(),
        PermissionKind::Mcp => patterns
            .map(|p| match p.split_once('/') {
                Some((server, tool)) => format!("mcp__{server}__{tool}"),
                None => format!("mcp__{p}"),
            })
            .collect(),
    }
}

/// A folder pattern as a Claude path rule: `//` starts an absolute path.
fn folder_rule(pattern: &str, root: &Path) -> String {
    match pattern.trim_end_matches(['*', '/']) {
        "." => "./**".into(),
        "~" => "~/**".into(),
        folder => format!("/{}/**", resolve(root, folder).display()),
    }
}
