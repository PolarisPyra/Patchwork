//! Finding the agent CLIs and the commands that start them: the paths the user chose, the PATH
//! the user's shell sets up, and the usual install folders.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use crate::Error;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use crate::system::files::paths::home_dir;
use super::{acp, antigravity, opencode, AgentKind};

/// Agent CLIs the user located by hand, mirrored from the settings.
static AGENT_PATHS: Mutex<Vec<(AgentKind, PathBuf)>> = Mutex::new(Vec::new());

/// Called whenever the settings are loaded or saved.
pub fn set_agent_paths(paths: &HashMap<AgentKind, String>) {
    *AGENT_PATHS.locked() = paths
        .iter()
        .map(|(kind, path)| (*kind, PathBuf::from(path.trim())))
        .filter(|(_, path)| !path.as_os_str().is_empty())
        .collect();
}

pub(super) fn located(kind: AgentKind) -> Option<PathBuf> {
    AGENT_PATHS
        .locked()
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, path)| path.clone())
}

/// The CLI the user located for `kind`, else what `search` finds. A located path is used alone,
/// so a missing file reads as not installed rather than falling back to another copy.
pub(super) fn located_or(
    kind: AgentKind,
    search: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    match located(kind) {
        Some(path) => path.is_file().then_some(path),
        None => search(),
    }
}

/// Directories searched when `which` cannot find the agent CLI on PATH.
fn fallback_dirs() -> Vec<PathBuf> {
    let home = home_dir().unwrap_or_default();
    let mut dirs: Vec<PathBuf> = [".local/bin", ".cargo/bin", ".bun/bin"]
        .iter()
        .map(|d| home.join(d))
        .collect();
    dirs.extend(["/usr/local/bin", "/opt/homebrew/bin"].map(PathBuf::from));
    // npm's global bin on Windows, where `npm i -g @openai/codex` puts `codex.cmd`.
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("npm"));
    }
    // Where Codex's own installer puts its CLI on Windows.
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join(r"Programs\OpenAI\Codex\bin"));
    }
    // The Codex desktop app on Windows keeps its CLI in a folder named by a hash that changes
    // with each update, so every one is searched, newest first.
    if let Some(entries) = std::env::var_os("LOCALAPPDATA")
        .and_then(|local| std::fs::read_dir(PathBuf::from(local).join("OpenAI/Codex/bin")).ok())
    {
        let mut bins: Vec<_> = entries
            .flatten()
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        bins.sort_by_key(|a| std::cmp::Reverse(a.0));
        dirs.extend(bins.into_iter().map(|(_, path)| path));
    }
    if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
        let mut nvm: Vec<PathBuf> = entries.flatten().map(|e| e.path().join("bin")).collect();
        nvm.sort();
        dirs.extend(nvm);
    }
    dirs
}

/// `name` on the PATH the user's shell sets up, else in the usual install folders.
pub(super) fn find_binary(name: &str) -> Option<PathBuf> {
    crate::system::find_program(name).or_else(|| {
        let names: &[&str] = if cfg!(windows) {
            &["exe", "cmd", "bat"]
        } else {
            &[""]
        };
        fallback_dirs().into_iter().find_map(|d| {
            names
                .iter()
                .map(|ext| d.join(name).with_extension(ext))
                .find(|p| p.is_file())
        })
    })
}

/// Where `kind`'s CLI is: the path the user chose, else the one found on PATH or in its usual
/// install folder.
pub(crate) fn agent_binary(kind: AgentKind) -> Result<PathBuf, Error> {
    Ok(match kind {
        AgentKind::Claude => {
            located_or(kind, || find_binary("claude")).ok_or("Claude Code is not installed")?
        }
        AgentKind::Codex => {
            located_or(kind, || find_binary("codex")).ok_or("Codex is not installed")?
        }
        AgentKind::Antigravity => {
            antigravity::antigravity_bin().ok_or("Antigravity CLI is not installed")?
        }
        AgentKind::OpenCode => opencode::opencode_bin().ok_or("OpenCode CLI is not installed")?,
        AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => {
            let spec = acp::spec(kind).ok_or("Unknown agent")?;
            acp::binary(spec).ok_or_else(|| format!("{} is not installed", spec.label))?
        }
        AgentKind::Api => return Err("API providers have no CLI".into()),
    })
}

/// Resolves the agent CLI (searching the PATH the user's shell sets up, then common install dirs)
/// and returns a Command for it with the binary's directory prepended to PATH (codex needs `node`
/// beside it).
pub(crate) fn agent_command(kind: AgentKind) -> Result<Command, Error> {
    let bin = match kind {
        AgentKind::Api => {
            std::env::current_exe().map_err(|e| format!("Failed to find Patchwork: {e}"))?
        }
        _ => agent_binary(kind)?,
    };
    let mut cmd = command_at(&bin);
    // Turns, probes and one-off calls all start here; the in-app terminal does not.
    if kind == AgentKind::Claude {
        cmd.env("CLAUDE_CODE_ENTRYPOINT", super::claude::entrypoint());
    }
    if kind == AgentKind::OpenCode && opencode::needs_own_database() {
        cmd.env("OPENCODE_DB", opencode::V1_DATABASE);
    }
    Ok(cmd)
}

/// A command for the agent CLI at `bin`, with its directory prepended to PATH.
pub(super) fn command_at(bin: &Path) -> Command {
    let mut cmd = program_command(bin);
    if let Some(dir) = bin.parent() {
        let mut paths = vec![dir.to_path_buf()];
        paths.extend(
            crate::system::search_path()
                .iter()
                .flat_map(std::env::split_paths),
        );
        if let Ok(joined) = std::env::join_paths(paths) {
            cmd.env("PATH", joined);
        }
    }
    cmd
}

/// A command for the program at `bin`, past the npm shim when it is one.
fn program_command(bin: &Path) -> Command {
    match unwrap_cmd_shim(bin) {
        Some((program, script)) => {
            let mut cmd = crate::system::host_command(program);
            cmd.args(script);
            cmd
        }
        None => crate::system::host_command(bin),
    }
}

/// Shims already warned about, so a launch says it once for each.
static WARNED_SHIMS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// What an npm `.cmd` shim on Windows starts: node with the CLI's script, or a native binary.
/// Through the shim the agent runs under `cmd.exe`, which cannot take arguments with newlines
/// or `%` (Rust refuses to start it with them) and holds the command line to 8,191 characters
/// rather than 32,767, so a turn's instructions or custom agents would fail it.
fn unwrap_cmd_shim(bin: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    let is_cmd = bin
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd"));
    if !cfg!(windows) || !is_cmd {
        return None;
    }
    let unwrapped = shim_target(bin);
    if let Err(why) = &unwrapped
        && !WARNED_SHIMS.locked().contains(&bin.to_path_buf())
    {
        WARNED_SHIMS.locked().push(bin.to_path_buf());
        log::warn!(
            shim:% = crate::logging::system::shown(bin), why:% = why;
            "{{shim}} runs through cmd.exe, whose command line is held to 8,191 characters: {{why}}"
        );
    }
    unwrapped.ok()
}

/// The program and script behind the `.cmd` shim at `bin`, or why there is none.
fn shim_target(bin: &Path) -> Result<(PathBuf, Option<PathBuf>), &'static str> {
    let text = std::fs::read_to_string(bin).map_err(|_e| "the shim cannot be read")?;
    let dir = bin.parent().ok_or("the shim has no folder")?;
    let target = cmd_shim_target(&text).ok_or("the shim names no script in its folder")?;
    let target = dir.join(target);
    if !target.is_file() {
        return Err("the script the shim names is missing");
    }
    if target
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        return Ok((target, None));
    }
    let node = Some(dir.join("node.exe"))
        .filter(|p| p.is_file())
        .or_else(|| find_binary("node"))
        .ok_or("node is not found")?;
    Ok((node, Some(target)))
}

/// The script a `.cmd` shim runs, relative to its folder: the last quoted path that starts at
/// the shim's folder, written `%dp0%\` by npm and `%~dp0\` by pnpm and yarn.
fn cmd_shim_target(text: &str) -> Option<&str> {
    text.split('"')
        .filter_map(|part| {
            part.strip_prefix("%dp0%\\")
                .or_else(|| part.strip_prefix("%~dp0\\"))
        })
        .rfind(|rest| !rest.eq_ignore_ascii_case("node.exe"))
}

/// A command the user configured, such as an MCP server: found and run with the PATH their
/// shell sets up, as agents get it.
#[must_use]
pub fn user_command(program: &str) -> Command {
    let bin = find_binary(program).unwrap_or_else(|| PathBuf::from(program));
    let mut cmd = program_command(&bin);
    if let Some(path) = crate::system::search_path() {
        cmd.env("PATH", path);
    }
    cmd
}

/// The command a turn runs. An OpenCode 2.x turn runs Patchwork's own binary as the bridge to
/// `opencode serve`; every other turn runs the agent CLI.
pub(crate) fn turn_command(kind: AgentKind) -> Result<Command, Error> {
    if kind == AgentKind::Api || (kind == AgentKind::OpenCode && opencode::uses_serve()) {
        let exe = std::env::current_exe().map_err(|e| format!("Failed to find Patchwork: {e}"))?;
        let mut cmd = Command::new(exe);
        crate::system::hide_console(&mut cmd);
        // Patchwork's own agent runs commands in the environment the user's shell sets up, so it
        // gets it whole; the bridge only needs the PATH to find `opencode`. Either way the shell
        // need not be read again.
        if kind == AgentKind::Api {
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
    let name = match kind {
        AgentKind::Claude => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Antigravity => "agy",
        AgentKind::Qwen | AgentKind::Kimi | AgentKind::Auggie => {
            acp::spec(kind).ok_or("Unknown agent")?.binary
        }
        AgentKind::OpenCode if !opencode::uses_serve() => "opencode",
        AgentKind::OpenCode | AgentKind::Api => return turn_command(kind),
    };
    let mut cmd = Command::new(name);
    if kind == AgentKind::Claude {
        cmd.env("CLAUDE_CODE_ENTRYPOINT", super::claude::entrypoint());
    }
    Ok(cmd)
}

/// The shell `agent` runs its commands in, which the turn sets as SHELL.
///
/// Each CLI picks its own: Claude Code runs only bash or zsh, so it gets bash, else zsh; Codex
/// always runs the account's login shell. The rest run SHELL, so they get bash, which models write
/// commands for: OpenCode refuses fish and Nushell, and Patchwork's own agent calls its tool
/// `bash`. Windows keeps the user's shell. None when it is not known.
#[must_use]
pub fn agent_shell(agent: AgentKind) -> Option<String> {
    use crate::system::{command_shell, find_shell, is_posix, login_shell};
    if cfg!(windows) {
        return crate::system::effective_shell();
    }
    match agent {
        AgentKind::Claude => find_shell("bash").or_else(|| find_shell("zsh")),
        AgentKind::Codex => login_shell().filter(|shell| is_posix(shell)),
        AgentKind::Antigravity
        | AgentKind::OpenCode
        | AgentKind::Qwen
        | AgentKind::Kimi
        | AgentKind::Auggie
        | AgentKind::Api => command_shell(),
    }
}

#[cfg(test)]
mod shim_tests {
    use super::cmd_shim_target;

    #[test]
    fn reads_npm_and_pnpm_shims() {
        let npm = "@ECHO off\r\nSET dp0=%~dp0\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n) \
                   ELSE (\r\n  SET \"_prog=node\"\r\n)\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \
                   \"%_prog%\"  \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n";
        assert_eq!(
            cmd_shim_target(npm),
            Some("node_modules\\@openai\\codex\\bin\\codex.js")
        );
        let pnpm = "@SETLOCAL\r\n@IF EXIST \"%~dp0\\node.exe\" (\r\n  \"%~dp0\\node.exe\"  \
                    \"%~dp0\\global\\5\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n) ELSE (\r\n  node  \
                    \"%~dp0\\global\\5\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n)\r\n";
        assert_eq!(
            cmd_shim_target(pnpm),
            Some("global\\5\\node_modules\\@openai\\codex\\bin\\codex.js")
        );
        assert_eq!(cmd_shim_target("@echo off\r\nnode codex.js %*"), None);
    }
}
