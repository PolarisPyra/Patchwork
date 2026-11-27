//! Locating `agy`, checking its version, and running its read-only diagnostics.

// Rust guideline compliant 2026-11-23

use std::path::PathBuf;
use std::process::Command;

use patchwork_provider_core::cli_info::{self, InfoCommand};
use patchwork_provider_core::{AgentKind, CliInfo, command_at, find_binary, located_or};
use patchwork_platform::Error;
use patchwork_platform::files::paths::home_dir;

use crate::models::models;

/// The Antigravity CLI: the one the user located, else `agy` on PATH or default install folders.
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

/// Builds a command for `agy` with its directory prepended to PATH.
pub(crate) fn agy_command() -> Result<Command, Error> {
    let bin = antigravity_bin().ok_or("Antigravity CLI is not installed")?;
    Ok(command_at(&bin))
}

/// `~/.gemini`, where Antigravity and the earlier Gemini CLI save sessions and config.
#[must_use]
pub fn home() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".gemini"))
}

/// The oldest agy whose ACP server reports the settings, modes and updates Patchwork reads.
pub const MIN_VERSION: &str = "1.5.0";

/// Whether `version`, as `agy --version` prints it, is older than `MIN_VERSION`.
///
/// A version that cannot be read is let through.
#[must_use]
pub fn is_older_than_minimum(version: &str) -> bool {
    let parts = |text: &str| -> Option<Vec<u32>> {
        let number = text.split_whitespace().next()?.trim_start_matches('v');
        let number = number.split(['-', '+']).next()?;
        number.split('.').map(|part| part.parse().ok()).collect()
    };
    match (parts(version), parts(MIN_VERSION)) {
        (Some(have), Some(need)) => have < need,
        _ => false,
    }
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
    let version = cli_info::version(&path);
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
