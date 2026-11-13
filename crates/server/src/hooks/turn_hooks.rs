//! The hooks Patchwork runs itself around a turn, for every agent, with the turn in their environment.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};
use crate::settings::Settings;
use super::{HookEvent, HookHandler, list};

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

/// Runs one hook command in a range of its own, stopping it and all it started at its timeout,
/// and what it left running once it exits. Its output goes to the log.
fn run_command(command: &str, timeout_secs: u64, context: &HookContext<'_>) -> Result<(), Error> {
    let mut cmd = crate::system::remote::shell_in(context.root, command)
        .ok_or("there is no shell to run it in")?;
    cmd.env("PATCHWORK_EVENT", context.event.name())
        .env("PATCHWORK_THREAD", context.thread_id)
        .env("PATCHWORK_CWD", crate::system::remote::here(context.root))
        .env("PATCHWORK_AGENT", context.agent)
        .env("PATCHWORK_STATUS", context.status.unwrap_or_default());
    let cmd = crate::system::remote::supervised(cmd)?;
    let (mut child, range) = crate::system::contained::spawn(cmd, Stdio::null())
        .map_err(|e| format!("did not start: {e}"))?;
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
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            // Short, so a quick hook returns at once instead of after a fixed wait.
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                range.stop();
                let _ = child.wait();
                return Err(format!("ran past {timeout_secs}s and was stopped").into());
            }
        }
    };
    // What it left running holds the output pipes open.
    range.stop();
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    let stdout = stdout.trim();
    if !stdout.is_empty() {
        log::info!(event = context.event.name(), stdout:%; "hook {{event}}: {{stdout}}");
    }
    if status.success() {
        Ok(())
    } else {
        let stderr = stderr.trim();
        Err(if stderr.is_empty() {
            "exited with an error".into()
        } else {
            stderr.to_string().into()
        })
    }
}
