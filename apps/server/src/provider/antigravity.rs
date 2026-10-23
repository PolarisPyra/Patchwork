//! Antigravity CLI (`agy --output-format stream-json`).

use std::path::PathBuf;

use serde_json::Value;

use crate::types::AccessMode;
use crate::types::AgentStatus;
use crate::types::ModelOption;
use super::GoalUpdate;
use super::home_dir;
use super::is_goal_command;
use super::model_manifest;
use super::Parsed;
use super::prompt;
use super::relative;
use super::summarize;
use super::text_delta;
use super::tool_finished;
use super::tool_started;
use super::ToolStart;
use super::TurnSpec;


/// The Antigravity CLI: Patchwork's own install (`patchwork/clis` in the config folder),
/// else `agy` on PATH.
pub fn antigravity_bin() -> Option<PathBuf> {
    let config = if cfg!(target_os = "macos") {
        home_dir().map(|h| h.join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join(".config")))
    };
    config
        .map(|c| c.join("patchwork/clis/agy"))
        .filter(|p| p.is_file())
        .or_else(|| which::which("agy").ok())
}

/// `~/.gemini`, where the earlier Gemini CLI saved its chats for the import to read.
pub fn home() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".gemini"))
}

/// Antigravity has no auth-status command or documented credentials file, so an installed CLI
/// counts as signed in; a run that is not signed in fails with "authentication required".
pub fn auth(status: &mut AgentStatus) {
    status.signed_in = antigravity_bin().is_some();
    status.detail = if status.signed_in {
        "Using the Antigravity CLI — run `agy` once to sign in".into()
    } else {
        "Antigravity CLI is not installed".into()
    };
}

/// Current models from the fetched model manifest.
pub fn models() -> Vec<ModelOption> {
    model_manifest::models("antigravity")
}

/// Antigravity without a way to ask: in "default" mode, actions that need approval are
/// refused and the run goes on.
pub fn turn_args(spec: &TurnSpec) -> Vec<String> {
    let mut args: Vec<String> = ["--output-format", "stream-json"].map(String::from).into();
    match spec.mode {
        AccessMode::FullAccess => args.push("--dangerously-skip-permissions".into()),
        AccessMode::AutoAcceptEdits => args.push("--mode=accept-edits".into()),
        AccessMode::Auto => args.push("--mode=auto".into()),
        AccessMode::Supervised => {}
    }
    if let Some(id) = spec.session_id {
        args.push(format!("--conversation={id}"));
    }
    if let Some(m) = spec.model {
        args.push(format!("--model={m}"));
    }
    if let Some(e) = spec.effort {
        args.push(format!("--effort={e}"));
    }
    // The `=` form keeps a prompt that starts with "-" from being read as a flag. Slash
    // commands such as `/goal` are expanded in print mode.
    args.push(format!("--print={}", prompt(spec)));
    args
}

/// Maps one line of `agy --output-format stream-json`: `init`, then a `step_update` per step
/// transition or text delta, then `result`. Each payload sits under its event's name.
pub fn parse(v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    let mut out = Vec::new();
    match v["event"].as_str() {
        Some("init") => {
            if let Some(id) = v["conversation_id"].as_str().filter(|id| !id.is_empty()) {
                out.push(Parsed::SessionId(id.into()));
            }
            // Antigravity reports nothing about a goal, so one set with `/goal` is taken as set
            // once the run starts.
            if let Some(objective) = goal_objective(spec) {
                out.push(Parsed::Goal(GoalUpdate::Set {
                    objective: objective.into(),
                    checks: None,
                }));
            }
        }
        // A subagent's steps are not part of the reply.
        Some("step_update") if v["step_update"]["subagent_info"].is_null() => {
            let step = &v["step_update"];
            if let Some(text) = step["text_delta"].as_str().filter(|t| !t.is_empty()) {
                out.push(text_delta(message_id, text));
            }
            let tool = &step["tool_info"];
            if let Some(name) = tool["name"].as_str().or_else(|| step["tool_name"].as_str()) {
                let id = format!("step-{}", step["step_index"]);
                let params = &tool["parameters"];
                // Antigravity names a file `Path`.
                let summary = params["Path"].as_str().map_or_else(
                    || summarize(params, spec.root),
                    |path| relative(spec.root, path),
                );
                out.push(tool_started(message_id, ToolStart { id: &id, name, summary, subagent: None, parent: None }));
                if step["state"] == "DONE" {
                    let failed = tool["error"].as_str().is_some_and(|e| !e.is_empty());
                    out.push(tool_finished(message_id, &id, failed, ""));
                }
            }
        }
        Some("result") => {
            let result = &v["result"];
            if result["status"] != "SUCCESS" {
                let message = result["error"]
                    .as_str()
                    .filter(|e| !e.is_empty())
                    .or_else(|| result["status"].as_str())
                    .unwrap_or("Turn failed");
                out.push(Parsed::Error(message.into()));
            }
        }
        _ => {}
    }
    out
}

/// The objective of a `/goal <objective>` message.
fn goal_objective<'a>(spec: &TurnSpec<'a>) -> Option<&'a str> {
    if !is_goal_command(spec.text) {
        return None;
    }
    Some(spec.text.trim()["/goal".len()..].trim()).filter(|o| !o.is_empty())
}
