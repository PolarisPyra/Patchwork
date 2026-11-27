//! Finding agent CLIs and building commands to launch them.
//!
//! Uses user-configured paths, the PATH the user's shell sets up, and common install directories.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use patchwork_platform::Locked;
use patchwork_platform::files::paths::home_dir;

use crate::{AgentKind, ShellChoice};

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

/// The path the user explicitly configured for `kind`, if any.
#[must_use]
pub fn located(kind: AgentKind) -> Option<PathBuf> {
    AGENT_PATHS
        .locked()
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, path)| path.clone())
}

/// The CLI the user located for `kind`, else what `search` finds.
///
/// A located path is used alone, so a missing file reads as not installed rather than falling back
/// to another copy.
pub fn located_or(kind: AgentKind, search: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
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
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("npm"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join(r"Programs\OpenAI\Codex\bin"));
    }
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
#[must_use]
pub fn find_binary(name: &str) -> Option<PathBuf> {
    patchwork_platform::find_program(name).or_else(|| {
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

/// A command for the agent CLI at `bin`, with its directory prepended to PATH.
#[must_use]
pub fn command_at(bin: &Path) -> Command {
    let mut cmd = program_command(bin);
    if let Some(dir) = bin.parent() {
        let mut paths = vec![dir.to_path_buf()];
        paths.extend(
            patchwork_platform::search_path()
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
            let mut cmd = patchwork_platform::host_command(program);
            cmd.args(script);
            cmd
        }
        None => patchwork_platform::host_command(bin),
    }
}

/// Shims already warned about, so a launch says it once for each.
static WARNED_SHIMS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// What an npm `.cmd` shim on Windows starts: node with the CLI's script, or a native binary.
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
        let name = bin
            .file_name()
            .map_or_else(|| bin.to_string_lossy(), std::ffi::OsStr::to_string_lossy);
        log::warn!(
            shim:% = name, why:% = why;
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

/// The script a `.cmd` shim runs, relative to its folder.
fn cmd_shim_target(text: &str) -> Option<&str> {
    text.split('"')
        .filter_map(|part| {
            part.strip_prefix("%dp0%\\")
                .or_else(|| part.strip_prefix("%~dp0\\"))
        })
        .rfind(|rest| !rest.eq_ignore_ascii_case("node.exe"))
}

/// A command the user configured, such as an MCP server, run with the user's shell PATH.
#[must_use]
pub fn user_command(program: &str) -> Command {
    let bin = find_binary(program).unwrap_or_else(|| PathBuf::from(program));
    let mut cmd = program_command(&bin);
    if let Some(path) = patchwork_platform::search_path() {
        cmd.env("PATH", path);
    }
    cmd
}

/// Resolves the shell executable for `choice`, returning `None` when unknown.
#[must_use]
pub fn agent_shell_for_choice(choice: ShellChoice) -> Option<String> {
    use patchwork_platform::{command_shell, find_shell, is_posix, login_shell};
    if cfg!(windows) {
        return patchwork_platform::effective_shell();
    }
    match choice {
        ShellChoice::BashOrZsh => find_shell("bash").or_else(|| find_shell("zsh")),
        ShellChoice::LoginPosix => login_shell().filter(|shell| is_posix(shell)),
        ShellChoice::Command => command_shell(),
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
