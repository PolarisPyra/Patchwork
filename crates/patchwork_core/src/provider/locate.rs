//! Finding the agent CLIs and building the commands that start turns.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::process::Command;

use crate::Error;

use super::{AgentKind, ProviderKind};

#[doc(inline)]
pub use patchwork_provider_core::{set_agent_paths, user_command};
pub(crate) use patchwork_provider_core::find_binary;
pub(super) use patchwork_provider_core::{command_at, located, located_or};

/// Where `kind`'s CLI is: the path the user chose, else the one found on PATH or in its usual
/// install folder.
pub(crate) fn agent_binary(kind: AgentKind) -> Result<PathBuf, Error> {
    let identity = kind.identity();
    if identity.cli.is_none() {
        return Err("API providers have no CLI".into());
    }
    kind.provider()
        .locate()
        .ok_or_else(|| format!("{} is not installed", identity.title).into())
}

/// Resolves the agent CLI (searching the PATH the user's shell sets up, then common install dirs)
/// and returns a Command for it with the binary's directory prepended to PATH (codex needs `node`
/// beside it).
pub(crate) fn agent_command(kind: AgentKind) -> Result<Command, Error> {
    let bin = if kind.identity().cli.is_none() {
        std::env::current_exe().map_err(|e| format!("Failed to find Patchwork: {e}"))?
    } else {
        agent_binary(kind)?
    };
    let mut cmd = command_at(&bin);
    // Turns, probes and one-off calls all start here; the in-app terminal does not.
    cmd.envs(kind.provider().command_env(false));
    Ok(cmd)
}

/// The command a turn runs. An OpenCode 2.x turn runs Patchwork's own binary as the bridge to
/// `opencode serve`; every other turn runs the agent CLI.
pub(crate) fn turn_command(kind: AgentKind) -> Result<Command, Error> {
    let caps = kind.capabilities();
    if caps.runs_in_patchwork {
        let exe = std::env::current_exe().map_err(|e| format!("Failed to find Patchwork: {e}"))?;
        let mut cmd = Command::new(exe);
        crate::system::hide_console(&mut cmd);
        // Patchwork's own agent runs commands in the environment the user's shell sets up, so it
        // gets it whole; the bridge only needs the PATH to find `opencode`. Either way the shell
        // need not be read again.
        if caps.patchwork_runs_commands {
            cmd.envs(crate::system::login_env().iter().map(|(k, v)| (k, v)));
        }
        if let Some(path) = crate::system::search_path() {
            cmd.env("PATH", path);
        }
        cmd.env(crate::system::ENV_READ, "1");
        return Ok(cmd);
    }
    agent_command(kind)
}

/// The command that runs a turn of `kind` in a project on an SSH host, before
/// [`crate::system::ssh::supervised`] moves it there. The agent CLIs are found on the host by
/// name; Patchwork's own agent and the OpenCode bridge run as Patchwork's helper there.
///
/// # Errors
///
/// Returns an error when Patchwork's own path cannot be found.
pub(crate) fn remote_turn_command(kind: AgentKind) -> Result<Command, Error> {
    let name = match kind.identity().cli {
        Some(name) if !kind.capabilities().runs_in_patchwork => name,
        _ => return turn_command(kind),
    };
    let mut cmd = Command::new(name);
    cmd.envs(kind.provider().command_env(true));
    Ok(cmd)
}

/// The shell `agent` runs its commands in, which the turn sets as SHELL.
#[must_use]
pub fn agent_shell(agent: AgentKind) -> Option<String> {
    patchwork_provider_core::agent_shell_for_choice(agent.capabilities().shell)
}
