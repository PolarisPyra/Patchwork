//! The login shells agents and the terminal run in.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::system::{find_program, host_command, login_env, path_var, search_path};

/// Entries in /etc/shells that aren't shells to run agents in: git's push-only shell, and
/// systemd's placeholder for accounts without a shell.
const NOT_AGENT_SHELLS: [&str; 2] = ["git-shell", "systemd-home-fallback-shell"];

/// Shells from /etc/shells that are installed, for the shell choice in settings.
///
/// Entries that resolve to the same binary (such as /bin/zsh and /usr/bin/zsh) are listed once,
/// under the real path when /etc/shells has it. Windows has no such file, so its shells are looked
/// for.
pub fn available_shells() -> Vec<String> {
    if cfg!(windows) {
        return windows_shells();
    }
    let listed: Vec<(String, PathBuf)> = std::fs::read_to_string("/etc/shells")
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('/') && Path::new(line).is_file())
        .filter(|line| {
            let name = Path::new(line).file_name().and_then(|n| n.to_str());
            !name.is_some_and(|n| NOT_AGENT_SHELLS.contains(&n))
        })
        .map(|line| {
            let real = std::fs::canonicalize(line).unwrap_or_else(|_| PathBuf::from(line));
            (line.to_string(), real)
        })
        .collect();
    listed
        .iter()
        .filter(|(path, real)| {
            let kept = listed
                .iter()
                .find(|(p, r)| r == real && Path::new(p) == real.as_path())
                .or_else(|| listed.iter().find(|(_, r)| r == real));
            kept.is_some_and(|(p, _)| p == path)
        })
        .map(|(path, _)| path.clone())
        .collect()
}

/// The login shell in the passwd database for the current user. Windows has none.
#[cfg(unix)]
pub fn login_shell() -> Option<String> {
    // Room for the passwd entry's strings, which are a few hundred bytes in practice. A longer
    // entry makes `getpwuid_r` fail with `ERANGE`, and no shell is found.
    let mut buf = vec![libc::c_char::default(); 4096];
    // SAFETY: passwd is plain C data, so the all-zero bit pattern is a valid value; getpwuid_r
    // overwrites it before it is read.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found = std::ptr::null_mut();
    // SAFETY: entry, buf, and found are valid for the lengths passed, and getpwuid_r writes only
    // within them.
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &raw mut entry,
            buf.as_mut_ptr(),
            buf.len(),
            &raw mut found,
        )
    };
    if rc != 0 || found.is_null() {
        return None;
    }
    // SAFETY: the rc and null checks passed, so pw_shell points at a NUL-terminated string inside
    // buf, which is still alive here.
    let shell = unsafe { std::ffi::CStr::from_ptr(entry.pw_shell) };
    shell
        .to_str()
        .ok()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(not(unix))]
pub fn login_shell() -> Option<String> {
    None
}

/// The shells installed on Windows, best for agents first: Git Bash, since models write commands
/// for a POSIX shell, then PowerShell and cmd. The `bash.exe` in System32 is WSL's, which runs in
/// another system, so Git's is found from `git` or where Git installs.
fn windows_shells() -> Vec<String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let find = |name: &str| which::which_in(name, path_var(), &cwd).ok();
    let mut git_bash: Vec<PathBuf> = find("git")
        .and_then(|git| Some(git.parent()?.parent()?.join("bin").join("bash.exe")))
        .into_iter()
        .collect();
    for var in ["ProgramFiles", "LOCALAPPDATA"] {
        if let Some(dir) = std::env::var_os(var) {
            let programs = if var == "LOCALAPPDATA" {
                "Programs"
            } else {
                ""
            };
            git_bash.push(PathBuf::from(dir).join(programs).join(r"Git\bin\bash.exe"));
        }
    }
    let shells = [
        git_bash.into_iter().find(|p| p.is_file()),
        find("pwsh"),
        find("powershell"),
        std::env::var_os("ComSpec")
            .map(PathBuf::from)
            .or_else(|| find("cmd")),
    ];
    shells
        .into_iter()
        .flatten()
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// The user's shell: the login shell, otherwise the SHELL environment variable.
///
/// On Windows, which has no login shell, the best installed one comes before SHELL, which Git Bash
/// sets to a path only it can read. None when nothing is known.
#[must_use]
pub fn effective_shell() -> Option<String> {
    let windows_default = cfg!(windows)
        .then(|| available_shells().first().cloned())
        .flatten();
    login_shell()
        .or(windows_default)
        .or_else(|| std::env::var("SHELL").ok().filter(|s| !s.is_empty()))
}

/// Shells that read the POSIX command language, which models write their commands in.
const POSIX_SHELLS: [&str; 5] = ["sh", "bash", "zsh", "dash", "ksh"];

/// A shell's program name, such as `zsh` for `/usr/bin/zsh` or `pwsh` for `pwsh.exe`.
#[must_use]
pub fn shell_name(shell: &str) -> String {
    Path::new(shell)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Whether `shell` reads POSIX commands, as fish and Nushell do not.
#[must_use]
pub fn is_posix(shell: &str) -> bool {
    POSIX_SHELLS.contains(&shell_name(shell).as_str())
}

/// The installed shell named `name`, from the PATH the user's shell sets up.
#[must_use]
pub fn find_shell(name: &str) -> Option<String> {
    find_program(name).map(|path| path.to_string_lossy().into_owned())
}

/// The shell that runs commands a model, a plugin or a check writes: bash, which they are written
/// for, else `sh`. On Windows, the user's shell, since agents there are told which one runs.
pub fn command_shell() -> Option<String> {
    if cfg!(windows) {
        return std::env::var("SHELL")
            .ok()
            .filter(|shell| Path::new(shell).is_file())
            .or_else(effective_shell);
    }
    find_shell("bash").or_else(|| Some("/bin/sh".into()))
}

/// A command that runs `command` in [`command_shell`] and exits.
///
/// The shell skips its startup files, which may print, prompt or start tmux, and runs in the
/// environment they set up instead ([`login_env`]), with the PATH a terminal has. Output stays
/// plain text, without colour or a pager waiting for keys. The caller picks the folder, pipes and
/// process group.
#[must_use]
pub fn shell_command(command: &str) -> Option<Command> {
    let shell = command_shell()?;
    let mut cmd = host_command(&shell);
    if shell_name(&shell) == "bash" {
        cmd.args(["--noprofile", "--norc"]);
    }
    cmd.args(shell_args(&shell, command))
        .envs(login_env().iter().map(|(k, v)| (k, v)))
        .env_remove("BASH_ENV")
        .env_remove("ENV")
        .envs([
            ("TERM", "dumb"),
            ("NO_COLOR", "1"),
            ("PAGER", "cat"),
            ("GIT_PAGER", "cat"),
        ]);
    if let Some(path) = search_path() {
        cmd.env("PATH", path);
    }
    Some(cmd)
}

/// The arguments that make `shell` run `command` and exit.
pub fn shell_args(shell: &str, command: &str) -> Vec<String> {
    match shell_name(shell).as_str() {
        "cmd" => vec!["/C".into(), command.into()],
        "powershell" | "pwsh" => ["-NoProfile", "-NonInteractive", "-Command", command]
            .map(String::from)
            .into(),
        _ => vec!["-c".into(), command.into()],
    }
}
