//! Antigravity CLI (`agy --output-format stream-json`).

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::PathBuf;

use serde_json::Value;

use crate::threads::{AccessMode, TokenUsage};
use crate::provider::AgentKind;
use crate::provider::AgentStatus;
use crate::provider::CliInfo;
use crate::provider::cli_info::{self, InfoCommand};
use crate::provider::ModelOption;
use super::find_binary;
use super::GoalUpdate;
use crate::system::fs::home_dir;
use super::goal_argument;
use super::located_or;
use super::one_line;
use super::probe;
use super::Parsed;
use super::prompt;
use super::relative;
use super::summarize;
use super::text_delta;
use super::tool_finished;
use super::tool_started;
use super::ToolStart;
use super::TurnSpec;

/// The Antigravity CLI: the one the user located, else `agy` on PATH or in the usual install
/// folders, else where Antigravity's Windows installer puts it by default.
#[must_use]
pub fn antigravity_bin() -> Option<PathBuf> {
    located_or(AgentKind::Antigravity, || {
        find_binary("agy").or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .filter(|_| cfg!(windows))
                .map(|d| PathBuf::from(d).join(r"agy\bin\agy.exe"))
                .filter(|p| p.is_file())
        })
    })
}

/// `~/.gemini`, where the earlier Gemini CLI saved its chats for the import to read.
#[must_use]
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

/// The models the CLI offers, from `agy models`, which prints each as its id and name.
#[must_use]
pub fn models() -> Vec<ModelOption> {
    let Some(listed) = probe(AgentKind::Antigravity, &["models"]).filter(|p| p.success) else {
        return Vec::new();
    };
    listed
        .stdout
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(id, name)| ModelOption {
            id: Some(id.trim().into()),
            label: name.trim().into(),
            default_model: None,
            is_legacy: false,
            efforts: None,
            context_window: None,
        })
        .collect()
}

/// The commands the Antigravity pane offers. It has no flags and no `doctor`.
const INFO_COMMANDS: &[InfoCommand] = &[
    InfoCommand::McpList,
    InfoCommand::PluginList,
    InfoCommand::Changelog,
];

/// The CLI's path, version and models, for the Settings pane.
///
/// # Errors
///
/// Returns an error when the Antigravity CLI is not installed.
pub fn info() -> Result<CliInfo, Error> {
    let path = antigravity_bin().ok_or("The Antigravity CLI is not installed")?;
    let version = cli_info::run(&path, &["--version"]).map_or_else(
        |_| "unknown".into(),
        |text| text.lines().next().unwrap_or_default().trim().to_string(),
    );
    Ok(CliInfo {
        path: path.to_string_lossy().into_owned(),
        version,
        settings: Vec::new(),
        models: models(),
    })
}

/// Runs one of the read-only diagnostics commands. See `cli_info::command`.
///
/// # Errors
///
/// Returns an error when Antigravity has no such command, when its CLI is not installed, or when
/// the command fails or takes too long.
pub fn command(kind: InfoCommand) -> Result<String, Error> {
    cli_info::check("Antigravity", kind, INFO_COMMANDS)?;
    let binary = antigravity_bin().ok_or("The Antigravity CLI is not installed")?;
    cli_info::command(&binary, kind)
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
    // Patchwork's permission hook runs before each tool call when the rules guard any, and the
    // user's hooks on the events Antigravity has.
    if let Some(dir) = crate::provider::hooks::antigravity_dir(spec) {
        args.push(format!("--add-dir={}", dir.display()));
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
#[must_use]
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
                // Antigravity names a file `Path` and a shell command `CommandLine`.
                let summary = match (params["Path"].as_str(), params["CommandLine"].as_str()) {
                    (Some(path), _) => relative(spec.root, path),
                    (None, Some(command)) => one_line(command),
                    (None, None) => summarize(params, spec.root),
                };
                out.push(tool_started(
                    message_id,
                    ToolStart {
                        id: &id,
                        name,
                        summary,
                        subagent: None,
                        parent: None,
                    },
                ));
                if step["state"] == "DONE" {
                    let failed = tool["error"].as_str().is_some_and(|e| !e.is_empty());
                    let output = tool["output"].as_str().unwrap_or_default();
                    out.push(tool_finished(message_id, &id, failed, output));
                }
            }
            // A step's usage is its one model request; `input_tokens` is the whole prompt, the
            // cached part included. The result's usage totals every turn of the conversation.
            let usage = &step["usage"];
            if usage.is_object() {
                let count = |key: &str| usage[key].as_u64().unwrap_or(0);
                let prompt = count("input_tokens");
                let cached = count("cache_read_tokens");
                out.push(Parsed::RequestUsage {
                    request_id: format!("{}-{}", step["conversation_id"], step["step_index"]),
                    usage: TokenUsage {
                        input: prompt.saturating_sub(cached),
                        cache_read: cached,
                        cache_write: 0,
                        output: count("output_tokens") + count("thinking_tokens"),
                        context: prompt,
                        ..Default::default()
                    },
                    top_level: true,
                });
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
    goal_argument(spec.text).filter(|o| !o.is_empty())
}
