//! The login shells agents and the terminal run in.

use std::path::{Path, PathBuf};

use crate::system::path_var;

/// Entries in /etc/shells that aren't shells to run agents in: git's push-only shell, and
/// systemd's placeholder for accounts without a shell.
const NOT_AGENT_SHELLS: [&str; 2] = ["git-shell", "systemd-home-fallback-shell"];

/// Shells from /etc/shells that are installed, for the shell choice in settings. Entries that
/// resolve to the same binary (such as /bin/zsh and /usr/bin/zsh) are listed once, under the
/// real path when /etc/shells has it. Windows has no such file, so its shells are looked for.
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
    let mut buf = vec![0 as libc::c_char; 4096];
    // SAFETY: passwd is plain C data, so the all-zero bit pattern is a valid value; getpwuid_r
    // overwrites it before it is read.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found = std::ptr::null_mut();
    // SAFETY: entry, buf, and found are valid for the lengths passed, and getpwuid_r writes only
    // within them.
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut entry,
            buf.as_mut_ptr(),
            buf.len(),
            &mut found,
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

/// The shell agents run commands in: the user's choice in Settings if it is still installed,
/// otherwise the login shell, otherwise the SHELL environment variable. On Windows, which has no
/// login shell, the best installed one comes before SHELL, which Git Bash sets to a path only it
/// can read. None when nothing is known.
pub fn effective_shell(chosen: Option<&str>) -> Option<String> {
    let installed = available_shells();
    let windows_default = cfg!(windows).then(|| installed.first().cloned()).flatten();
    chosen
        .filter(|shell| installed.iter().any(|s| s == shell))
        .map(str::to_string)
        .or_else(login_shell)
        .or(windows_default)
        .or_else(|| std::env::var("SHELL").ok().filter(|s| !s.is_empty()))
}

/// The arguments that make `shell` run `command` and exit.
pub fn shell_args(shell: &str, command: &str) -> Vec<String> {
    let name = Path::new(shell)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match name.as_str() {
        "cmd" => vec!["/C".into(), command.into()],
        "powershell" | "pwsh" => ["-NoProfile", "-NonInteractive", "-Command", command]
            .map(String::from)
            .into(),
        _ => vec!["-c".into(), command.into()],
    }
}
