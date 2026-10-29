//! Finding the agent CLIs and the commands that start them: the paths the user chose, PATH, the usual install folders, and the user's login-shell PATH.

use std::collections::HashMap;
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};
use crate::system::fs::home_dir;
use crate::workspace::{signal_process_group, SIGKILL};
use super::{antigravity, opencode, AgentKind};
use super::{PROBE_TIMEOUT, read_pipe, stop, text};

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

pub(super) fn find_binary(name: &str) -> Option<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_default();
    which::which_in(name, crate::system::path_var(), cwd)
        .ok()
        .or_else(|| {
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

/// Resolves the agent CLI (searching PATH plus common install dirs, since GUI
/// launches may not inherit the shell PATH) and returns a Command for it with
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
        paths.extend(shell_path().into_iter().flat_map(std::env::split_paths));
        if let Some(existing) = crate::system::path_var() {
            paths.extend(std::env::split_paths(&existing));
        }
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
/// login shell sets up, as agents get it.
pub fn user_command(program: &str) -> Command {
    let mut paths: Vec<PathBuf> = shell_path()
        .into_iter()
        .flat_map(std::env::split_paths)
        .collect();
    if let Some(existing) = crate::system::path_var() {
        paths.extend(std::env::split_paths(&existing));
    }
    let path = std::env::join_paths(paths).ok();
    let bin = path
        .as_ref()
        .and_then(|p| which::which_in(program, Some(p), ".").ok())
        .or_else(|| find_binary(program))
        .unwrap_or_else(|| PathBuf::from(program));
    let mut cmd = program_command(&bin);
    if let Some(path) = path {
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
        return Ok(cmd);
    }
    agent_command(kind)
}

/// Marks the PATH line in the login shell's output, since profile scripts may print too.
const PATH_MARKER: &str = "__PATCHWORK_PATH__";

/// The PATH the user's login shell sets up, read once. An app launched from the desktop gets
/// the session's PATH, which lacks what shell profiles add (nvm, pnpm, cargo), so agents and
/// the commands they run would not find those tools. None if the shell can't be read.
fn shell_path() -> Option<&'static OsString> {
    static PATH: OnceLock<Option<OsString>> = OnceLock::new();
    PATH.get_or_init(read_shell_path).as_ref()
}

fn read_shell_path() -> Option<OsString> {
    login_shell_value("PATH", false).map(OsString::from)
}

/// An environment variable as the user's interactive login shell sets it, for variables
/// exported from shell profiles that a desktop launch does not inherit. None if unset or empty.
pub fn shell_var(name: &str) -> Option<String> {
    let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    valid.then(|| login_shell_value(name, true)).flatten()
}

/// `$name` in the user's login shell, read with a marker since profile scripts may print too.
fn login_shell_value(name: &str, interactive: bool) -> Option<String> {
    let shell = crate::system::login_shell().or_else(|| std::env::var("SHELL").ok())?;
    let mut cmd = crate::system::host_command(shell);
    let flags = if interactive { "-lic" } else { "-lc" };
    cmd.args([
        flags,
        &format!("printf '\\n{PATH_MARKER}%s\\n' \"${name}\""),
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null());
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().ok()?;
    let (out, reader) = read_pipe(child.stdout.take()?);
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                stop(&mut child);
                return None;
            }
        }
    }
    // Kill anything the profile left running, so it cannot hold the pipe open.
    signal_process_group(child.id(), SIGKILL);
    let _ = reader.join();
    let value = text(&out)
        .lines()
        .find_map(|line| line.strip_prefix(PATH_MARKER))?
        .to_string();
    (!value.is_empty()).then_some(value)
}
