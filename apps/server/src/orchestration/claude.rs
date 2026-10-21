//! Launch arguments for Claude Code turns.

use crate::types::Settings;
use crate::{available_shells, effective_shell};

/// Arguments that append the shell instructions to Claude's system prompt.
pub fn launch_args(settings: &Settings) -> Vec<String> {
    let mut prompt = String::new();
    let installed = available_shells();
    if !installed.is_empty() {
        prompt.push_str(&format!(
            "Shells on this machine: {}.",
            installed.join(", ")
        ));
    }
    let chosen = settings.shell.as_deref();
    if let Some(shell) = effective_shell(chosen) {
        if chosen == Some(shell.as_str()) {
            prompt.push_str(&format!(
                " The user chose {shell} for shell commands; run them in it and write them for it."
            ));
        } else {
            prompt.push_str(&format!(
                " Shell commands run in {shell}, the user's login shell; write them for it."
            ));
        }
    }
    vec![
        "--append-system-prompt".to_string(),
        prompt.trim_start().to_string(),
    ]
}
