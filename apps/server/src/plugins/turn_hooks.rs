//! The hooks Patchwork runs itself around a turn, for every agent, with the turn in their environment.

use std::time::Duration;
use crate::settings::Settings;
use super::{HookEvent, PluginHookHandler, list};

/// The turn a hook runs for, as its environment tells it.
pub struct HookContext<'a> {
    pub event: HookEvent,
    pub thread_id: &'a str,
    /// The turn's project folder, where the hook runs.
    pub root: &'a str,
    pub agent: &'a str,
    /// How the turn's reply ended, for the events that end one.
    pub status: Option<&'a str>,
}

/// Runs the enabled plugins' command hooks for `context.event`, one after another, in the user's
/// shell in the project folder. Returns `"<plugin name>: <why>"` for each one that failed; a
/// failed hook never stops the turn.
pub fn run_hooks(settings: &Settings, context: &HookContext<'_>) -> Vec<String> {
    let commands: Vec<(String, String, u64)> = list(settings)
        .into_iter()
        .filter(|plugin| plugin.enabled)
        .flat_map(|plugin| {
            let name = plugin.name;
            plugin
                .hooks
                .into_iter()
                .filter(|hook| hook.event == context.event)
                .filter_map(move |hook| match hook.handler {
                    PluginHookHandler::Command {
                        command,
                        timeout_secs,
                    } => Some((name.clone(), command, timeout_secs)),
                    PluginHookHandler::Builtin { .. }
                    | PluginHookHandler::McpTool { .. }
                    | PluginHookHandler::Prompt
                    | PluginHookHandler::Agent
                    | PluginHookHandler::Unsupported { .. } => None,
                })
        })
        .collect();
    if commands.is_empty() {
        return Vec::new();
    }
    let Some(shell) = crate::system::effective_shell(None) else {
        return vec!["hooks: no shell to run them in".into()];
    };
    commands
        .into_iter()
        .filter_map(|(name, command, timeout_secs)| {
            run_command(&shell, &command, timeout_secs, context)
                .err()
                .map(|why| format!("{name}: {why}"))
        })
        .collect()
}

/// Runs one hook command, killing it at its timeout. Its output goes to the log.
fn run_command(
    shell: &str,
    command: &str,
    timeout_secs: u64,
    context: &HookContext<'_>,
) -> Result<(), String> {
    let mut cmd = crate::system::host_command(shell);
    cmd.current_dir(context.root)
        .env("PATCHWORK_EVENT", context.event.name())
        .env("PATCHWORK_THREAD", context.thread_id)
        .env("PATCHWORK_CWD", context.root)
        .env("PATCHWORK_AGENT", context.agent)
        .env("PATCHWORK_STATUS", context.status.unwrap_or_default());
    let timeout = Duration::from_secs(timeout_secs);
    let args = crate::system::shell_args(shell, command);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let probe = crate::provider::run_probe(cmd, &args, timeout, |_| false)
        .ok_or_else(|| format!("did not start, or ran past {timeout_secs}s and was stopped"))?;
    let stdout = probe.stdout.trim();
    if !stdout.is_empty() {
        log::info!("plugin hook {}: {stdout}", context.event.name());
    }
    if probe.success {
        Ok(())
    } else {
        let stderr = probe.stderr.trim();
        Err(if stderr.is_empty() {
            "exited with an error".into()
        } else {
            stderr.to_string()
        })
    }
}
