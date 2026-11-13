//! The hooks Patchwork runs itself around a turn, for every agent, with the turn in their environment.

// Rust guideline compliant 2026-10-09

use super::process::run_hook_process;
use super::{HookEvent, HookHandler, list};
use crate::Error;
use crate::settings::Settings;

/// The turn a hook runs for, as its environment tells it.
#[derive(Debug)]
pub struct HookContext<'a> {
    pub event: HookEvent,
    pub thread_id: &'a str,
    /// The turn's project folder, where the hook runs.
    pub root: &'a str,
    pub agent: &'a str,
    /// How the turn's reply ended, for the events that end one.
    pub status: Option<&'a str>,
}

/// Runs the enabled bundles' command hooks for an event, returning the failures.
///
/// Runs the enabled bundles' command hooks for `context.event`, one after another, in the user's
/// shell, or bash where it is not a POSIX one, in the project folder. Returns
/// `"<bundle name>: <why>"` for each one that failed; a failed hook never stops the turn.
#[must_use]
pub fn run_hooks(settings: &Settings, context: &HookContext<'_>) -> Vec<String> {
    let commands: Vec<(String, String, u64)> = list(settings)
        .into_iter()
        .filter(|bundle| bundle.enabled)
        .flat_map(|bundle| {
            let name = bundle.name;
            bundle
                .hooks
                .into_iter()
                .filter(|hook| hook.event == context.event)
                .filter_map(move |hook| match hook.handler {
                    HookHandler::Command {
                        command,
                        timeout_secs,
                    } => Some((name.clone(), command, timeout_secs)),
                    HookHandler::McpTool { .. }
                    | HookHandler::Prompt
                    | HookHandler::Agent
                    | HookHandler::Unsupported { .. } => None,
                })
        })
        .collect();
    if commands.is_empty() {
        return Vec::new();
    }
    commands
        .into_iter()
        .filter_map(|(name, command, timeout_secs)| {
            run_command(&command, timeout_secs, context)
                .err()
                .map(|why| format!("{name}: {why}"))
        })
        .collect()
}

/// Runs one hook command in the project folder, with the turn in its environment. Its output
/// goes to the log.
fn run_command(command: &str, timeout_secs: u64, context: &HookContext<'_>) -> Result<(), Error> {
    let env = [
        ("PATCHWORK_EVENT", context.event.name()),
        ("PATCHWORK_THREAD", context.thread_id),
        ("PATCHWORK_CWD", crate::system::remote::here(context.root)),
        ("PATCHWORK_AGENT", context.agent),
        ("PATCHWORK_STATUS", context.status.unwrap_or_default()),
    ];
    let done = run_hook_process(context.root, command, &env, None, timeout_secs)?;
    let stdout = done.stdout.trim();
    if !stdout.is_empty() {
        log::info!(event = context.event.name(), stdout:%; "hook {{event}}: {{stdout}}");
    }
    if done.status.success() {
        Ok(())
    } else {
        let stderr = done.stderr.trim();
        Err(if stderr.is_empty() {
            "exited with an error".into()
        } else {
            stderr.to_string().into()
        })
    }
}
