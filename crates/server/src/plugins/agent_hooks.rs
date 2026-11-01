//! The hooks an agent runs on its own events, such as before a tool call, and Patchwork's own agent
//! running the tool hooks around its calls. Each agent CLI gets them in the shape it reads: see
//! `provider::hooks`.

use std::io::{Read, Write};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::builtin::BuiltinHook;
use super::{HookEvent, PluginHookHandler, list};
use crate::settings::Settings;

/// A hook an agent runs on one of its own events. The agent gives it the event as JSON on stdin
/// and reads its answer, so a tool hook can refuse a call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHook {
    pub event: HookEvent,
    pub matcher: Option<String>,
    pub action: HookAction,
    pub timeout_secs: u64,
}

/// What a hook runs: a shell command, or a check Patchwork carries itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HookAction {
    Command { command: String },
    Builtin { hook: BuiltinHook },
}

impl AgentHook {
    /// Whether the hook runs for a call to `tool`. A matcher is a regular expression over the whole
    /// tool name, so `Bash` is that tool alone and `Edit|Write` either.
    pub fn matches(&self, tool: &str) -> bool {
        self.matcher.as_deref().is_none_or(|matcher| {
            regex::Regex::new(&format!("^(?:{matcher})$")).is_ok_and(|re| re.is_match(tool))
        })
    }
}

/// The hooks of the enabled plugins that the agent runs, rather than Patchwork. A built-in hook
/// this version does not carry is left out.
pub fn agent_hooks(settings: &Settings) -> Vec<AgentHook> {
    list(settings)
        .into_iter()
        .filter(|plugin| plugin.enabled)
        .flat_map(|plugin| plugin.hooks)
        .filter(|hook| !hook.event.runs_in_patchwork())
        .filter_map(|hook| {
            let (action, timeout_secs) = match hook.handler {
                PluginHookHandler::Command {
                    command,
                    timeout_secs,
                } => (HookAction::Command { command }, timeout_secs),
                PluginHookHandler::Builtin {
                    name, timeout_secs, ..
                } => {
                    let Some(builtin) = BuiltinHook::from_name(&name) else {
                        log::warn!("plugin hook {}: no built-in hook {name}", hook.event.name());
                        return None;
                    };
                    (HookAction::Builtin { hook: builtin }, timeout_secs)
                }
                PluginHookHandler::McpTool { .. }
                | PluginHookHandler::Prompt
                | PluginHookHandler::Agent
                | PluginHookHandler::Unsupported { .. } => return None,
            };
            Some(AgentHook {
                event: hook.event,
                matcher: hook.matcher,
                action,
                timeout_secs,
            })
        })
        .collect()
}

/// Runs a tool hook for Patchwork's own agent the way Claude Code runs one, and returns the reason
/// the call is refused, or None to let it run. A built-in hook runs in-process. A command gets
/// `input` as JSON on stdin, and exit code 2 with a reason on stderr, or a `deny` decision on
/// stdout, refuses the call; one that fails or runs past its timeout lets the call run, as in the
/// CLIs.
pub fn run_tool_hook(hook: &AgentHook, input: &Value, root: &str) -> Option<String> {
    match &hook.action {
        HookAction::Builtin { hook: builtin } => builtin.check(input),
        HookAction::Command { command } => run_command(hook, command, input, root),
    }
}

fn run_command(hook: &AgentHook, command: &str, input: &Value, root: &str) -> Option<String> {
    let event = hook.event.name();
    let shell = crate::system::posix_shell(crate::system::effective_shell(None))?;
    let mut cmd = crate::system::host_command(&shell);
    cmd.current_dir(root)
        .args(crate::system::shell_args(&shell, command))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd
        .spawn()
        .inspect_err(|e| log::warn!("plugin hook {event} did not start: {e}"))
        .ok()?;
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        })
    };
    let stdout = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    if let Some(mut stdin) = child.stdin.take() {
        // On its own thread, so a hook that does not read its input cannot hold off the timeout.
        let input = input.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let deadline = Instant::now() + Duration::from_secs(hook.timeout_secs);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                crate::workspace::signal_process_group(child.id(), crate::workspace::SIGKILL);
                let _ = child.kill();
                let _ = child.wait();
                log::warn!(
                    "plugin hook {event} ran past {}s and was stopped",
                    hook.timeout_secs
                );
                return None;
            }
        }
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    match status.code() {
        Some(2) => Some(
            Some(stderr.trim())
                .filter(|reason| !reason.is_empty())
                .unwrap_or("A plugin hook refused this call")
                .to_string(),
        ),
        Some(0) => {
            let answer: Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
            let output = &answer["hookSpecificOutput"];
            let reason = |key: &str, value: &Value| {
                value[key]
                    .as_str()
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or("A plugin hook refused this call")
                    .to_string()
            };
            if output["permissionDecision"] == "deny" {
                Some(reason("permissionDecisionReason", output))
            } else if answer["decision"] == "block" {
                Some(reason("reason", &answer))
            } else {
                None
            }
        }
        _ => {
            log::warn!("plugin hook {event} failed: {}", stderr.trim());
            None
        }
    }
}
