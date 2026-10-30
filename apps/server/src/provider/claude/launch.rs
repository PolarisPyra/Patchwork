//! Launch arguments for Claude Code turns.

use std::path::Path;

use crate::agents::{self, Agent};
use crate::plugins::AgentHook;
use crate::provider::AgentKind;
use crate::settings::Settings;

/// Arguments that append the checklist and shell instructions to Claude's system prompt, and load
/// Patchwork's skills and the plugins' hooks from the `claude` folder as a plugin. Claude records
/// a conversation's system prompt on its first request and resumes with that record, so a prompt
/// edited in Settings would never reach a chat already started; with the record off the prompt
/// is built on each request, and the cache still holds it while it stays the same.
pub fn launch_args(settings: &Settings, hooks: &[AgentHook]) -> Vec<String> {
    let mut args = vec![
        "--system-prompt-snapshot".to_string(),
        "off".to_string(),
        "--append-system-prompt".to_string(),
        crate::orchestration::agent_instructions(settings, AgentKind::Claude),
    ];
    if let Some(plugin) = crate::skills::sync(settings, "claude") {
        write_hooks(&plugin.join("hooks").join("hooks.json"), hooks);
        args.extend([
            "--plugin-dir".to_string(),
            plugin.to_string_lossy().into_owned(),
        ]);
    }
    args
}

/// Writes the hooks Claude runs to `file` in its plugin folder, or removes the file when there are
/// none. Turns share the folder, so the file is only written when it changes.
fn write_hooks(file: &Path, hooks: &[AgentHook]) {
    let config = crate::provider::hooks::hooks_json(hooks, AgentKind::Claude);
    if config["hooks"]
        .as_object()
        .is_none_or(|events| events.is_empty())
    {
        let _ = std::fs::remove_file(file);
        return;
    }
    let Ok(text) = serde_json::to_string_pretty(&config) else {
        return;
    };
    if std::fs::read_to_string(file).ok().as_deref() == Some(text.as_str()) {
        return;
    }
    let written = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(file, text));
    if let Err(e) = written {
        log::warn!("could not write Claude's plugin hooks: {e}");
    }
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
/// lists and instructions are added to the context again whenever a session resumes. Claude only
/// offers its task list tools to newer models when asked, and they are what fills the plan above
/// the chat box. A set compact window, the chat's or Settings', makes Claude compact once its
/// context reaches it.
pub fn launch_env(compact_window: u64) -> Vec<(&'static str, String)> {
    let mut env = vec![
        ("ENABLE_CLAUDEAI_MCP_SERVERS", "false".to_string()),
        ("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1".to_string()),
    ];
    if compact_window > 0 {
        env.push((
            "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
            compact_window.to_string(),
        ));
    }
    env
}

/// The tools a permission rule can answer in Claude, as the matcher of the hook that asks
/// Patchwork before each of them. The rest of Claude's tools, such as `TodoWrite`, have no rule
/// and are left to Claude.
pub const PERMISSION_TOOLS: &str =
    "^(Bash|Edit|Write|MultiEdit|NotebookEdit|Read|Glob|Grep|LS|WebFetch|mcp__.*)$";

/// `--settings` with Patchwork's permission hook, which asks the app before every call a rule can
/// answer. Installed on every turn, whether or not a rule guards anything yet, so a rule saved
/// while the turn runs decides the calls after it: the hook asks at each call, and the app reads
/// its rules then. This is also the only way to gate a call in full access, which auto-approves
/// every tool before asking anything (`--permission-prompt-tool` is never consulted). Empty when
/// there is no relay for the hook to ask.
pub fn permission_args() -> Vec<String> {
    if crate::provider::mcp::relay_endpoint().is_none() {
        return Vec::new();
    }
    let Some(command) = crate::provider::hooks::patchwork_command(
        AgentKind::Claude,
        &[crate::permissions::hook::HOOK_ARG, "claude"],
    ) else {
        return Vec::new();
    };
    vec![
        "--settings".into(),
        permission_settings(&command).to_string(),
    ]
}

/// The `--settings` document that runs `command` as Claude's permission hook.
fn permission_settings(command: &str) -> serde_json::Value {
    // The user may take a while to answer a request the hook holds.
    serde_json::json!({ "hooks": { "PreToolUse": [{
        "matcher": PERMISSION_TOOLS,
        "hooks": [{ "type": "command", "command": command, "timeout": 86400 }],
    }] } })
}

#[cfg(test)]
mod tests {
    use super::{PERMISSION_TOOLS, permission_settings};

    /// The hook has to run for every tool a rule can answer, and only those: the rest are left to
    /// Claude, which is what keeps `TodoWrite` and its kin out of the permission path.
    #[test]
    fn the_permission_hook_matcher_names_the_tools_a_rule_answers() {
        let matcher = regex::Regex::new(PERMISSION_TOOLS).expect("the matcher is a regex");
        for tool in [
            "Bash",
            "Edit",
            "Write",
            "MultiEdit",
            "NotebookEdit",
            "Read",
            "Glob",
            "Grep",
            "LS",
            "WebFetch",
            "mcp__github__search",
        ] {
            assert!(matcher.is_match(tool), "{tool} is asked about");
        }
        for tool in ["TodoWrite", "ExitPlanMode", "AskUserQuestion"] {
            assert!(!matcher.is_match(tool), "{tool} is left to Claude");
        }
    }

    #[test]
    fn the_settings_run_patchwork_as_the_pre_tool_hook() {
        let settings = permission_settings("'patchwork' --permission-hook claude");
        let hook = &settings["hooks"]["PreToolUse"][0]["hooks"][0];
        assert_eq!(hook["type"], "command");
        assert_eq!(hook["command"], "'patchwork' --permission-hook claude");
        // The user may take a while to answer a held call.
        assert_eq!(hook["timeout"], 86400);
        assert_eq!(
            settings["hooks"]["PreToolUse"][0]["matcher"],
            PERMISSION_TOOLS
        );
    }
}
