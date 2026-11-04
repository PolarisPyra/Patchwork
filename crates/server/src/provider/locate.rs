//! Finding the agent CLIs and the commands that start them: the paths the user chose, the PATH
//! the user's shell sets up, and the usual install folders.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, PoisonError};
use crate::system::fs::home_dir;
use super::{antigravity, opencode, AgentKind};

/// Agent CLIs the user located by hand, mirrored from the settings.
static AGENT_PATHS: Mutex<Vec<(AgentKind, PathBuf)>> = Mutex::new(Vec::new());

/// Called whenever the settings are loaded or saved.
pub fn set_agent_paths(paths: &HashMap<AgentKind, String>) {
    *AGENT_PATHS.lock().unwrap_or_else(PoisonError::into_inner) = paths
        .iter()
        .map(|(kind, path)| (*kind, PathBuf::from(path.trim())))
        .filter(|(_, path)| !path.as_os_str().is_empty())
        .collect();
}

pub(super) fn located(kind: AgentKind) -> Option<PathBuf> {
    AGENT_PATHS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
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

/// The Codex CLI in use: the one the user located, else `codex` on PATH.
pub fn codex_binary() -> Option<PathBuf> {
    located_or(AgentKind::Codex, || find_binary("codex"))
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

/// Resolves the agent CLI (searching the PATH the user's shell sets up, then common install
/// dirs) and returns a Command for it with
/// the binary's directory prepended to PATH (codex needs `node` beside it).
pub fn agent_command(kind: AgentKind) -> Result<Command, String> {
    let bin = match kind {
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
        AgentKind::Api => {
            std::env::current_exe().map_err(|e| format!("Failed to find Patchwork: {e}"))?
        }
    };
    let mut cmd = program_command(&bin);
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
    Ok(cmd)
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

/// What an npm `.cmd` shim on Windows starts: node with the CLI's script, or a native binary.
/// Through the shim the agent runs under `cmd.exe`, which cannot take arguments with newlines
/// or `%` (Rust refuses to start it with them), so a turn's instructions would fail it.
fn unwrap_cmd_shim(bin: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    let is_cmd = bin
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd"));
    if !cfg!(windows) || !is_cmd {
        return None;
    }
    let text = std::fs::read_to_string(bin).ok()?;
    let dir = bin.parent()?;
    // The target is the last quoted path in the shim's folder: "%dp0%\node_modules\…".
    let target = text
        .split('"')
        .filter_map(|part| part.strip_prefix("%dp0%\\"))
        .next_back()?;
    let target = dir.join(target);
    if !target.is_file() {
        return None;
    }
    if target
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        return Some((target, None));
    }
    let node = Some(dir.join("node.exe"))
        .filter(|p| p.is_file())
        .or_else(|| find_binary("node"))?;
    Some((node, Some(target)))
}

/// A command the user configured, such as an MCP server: found and run with the PATH their
/// shell sets up, as agents get it.
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
pub fn turn_command(kind: AgentKind) -> Result<Command, String> {
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

/// The shell `agent` runs its commands in, which the turn sets as SHELL. Each CLI picks its own:
/// Claude Code runs only bash or zsh, so it gets bash, else zsh; Codex always runs the account's
/// login shell. The rest run SHELL, so they get bash, which models write commands for: OpenCode
/// refuses fish and Nushell, and Patchwork's own agent calls its tool `bash`. Windows keeps the
/// user's shell. None when it is not known.
pub fn agent_shell(agent: AgentKind) -> Option<String> {
    use crate::system::{command_shell, find_shell, is_posix, login_shell};
    if cfg!(windows) {
        return crate::system::effective_shell();
    }
    match agent {
        AgentKind::Claude => find_shell("bash").or_else(|| find_shell("zsh")),
        AgentKind::Codex => login_shell().filter(|shell| is_posix(shell)),
        AgentKind::Antigravity | AgentKind::OpenCode | AgentKind::Api => command_shell(),
    }
}
