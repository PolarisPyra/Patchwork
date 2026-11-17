//! The `bash` tool: a command in bash, in the environment the user's shell sets up, with a time
//! limit. Patchwork runs it rather than the agent's process, so one still running at its limit
//! carries on as a background job; this is the command it runs and what the model is told of it.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::Path;
use std::process::Command;

pub const BASH_TIMEOUT: u64 = 120;

pub const MAX_BASH_TIMEOUT: u64 = 600;

/// `command` as the bash tool runs it, in `root`.
pub(crate) fn bash_command(command: &str, root: &Path) -> Result<Command, Error> {
    let mut cmd = crate::system::shell_command(command)
        .ok_or_else(|| "No shell was found to run the command in".to_string())?;
    cmd.current_dir(root);
    Ok(cmd)
}

/// What the model is told of a command that printed `text` and exited with `code`, None when a
/// signal ended it.
///
/// # Errors
///
/// Returns an error, holding the output and the exit code, when the command exited with a nonzero
/// code or was ended by a signal.
pub fn outcome(text: &str, code: Option<i32>) -> Result<String, Error> {
    let text = text.trim_end();
    match code {
        Some(0) => Ok(if text.is_empty() {
            "(no output)".into()
        } else {
            text.into()
        }),
        code => {
            let code = code.map_or_else(|| "a signal".into(), |c| c.to_string());
            Err(format!("{text}\n\nExit code {code}").trim_start().into())
        }
    }
}
