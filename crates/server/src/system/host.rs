//! Programs on the user's system: the command and environment they run with.

// Rust guideline compliant 2026-10-09

#[cfg(windows)]
use std::path::PathBuf;

/// A command for a program on the user's system.
///
/// The AppImage's launcher points the library, Python, GTK and data paths into the AppImage for
/// Patchwork itself; agents, git, and the other programs Patchwork starts get the user's own paths
/// back. A copy restarted into an update inherits the previous copy's paths too, so entries in any
/// AppImage mount (`/tmp/.mount_*`) are removed, not only the ones in `APPDIR`.
pub fn host_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    hide_console(&mut cmd);
    for (key, value) in host_env_changes() {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
    #[cfg(windows)]
    if let Some(path) = path_var() {
        cmd.env("PATH", path);
    }
    cmd
}

/// The PATH to find programs on.
///
/// On Windows that is the PATH as the system now has it, read from the registry ahead of the one
/// Patchwork started with: installers write the registry, and a running app never sees it change,
/// so a CLI installed after Patchwork opened would not be found until it restarted. Read at most
/// every few seconds.
#[must_use]
pub fn path_var() -> Option<std::ffi::OsString> {
    #[cfg(windows)]
    {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};
        static CACHE: Mutex<Option<(Instant, Option<std::ffi::OsString>)>> = Mutex::new(None);
        let mut cache = CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((at, path)) = cache.as_ref()
            // Long enough to serve a burst of calls, short enough to see a newly installed program.
            && at.elapsed() < Duration::from_secs(3)
        {
            return path.clone();
        }
        let path = windows_path();
        *cache = Some((Instant::now(), path.clone()));
        path
    }
    #[cfg(not(windows))]
    std::env::var_os("PATH")
}

/// Starts a Windows console program with a hidden console of its own.
///
/// Starts a console program on Windows with a console of its own that has no window. Patchwork
/// has no console, so without this every program it starts, and every program an agent starts
/// in turn, opens a console window. Their children share the hidden console.
pub fn hide_console(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// The system's PATH, then the user's, from the registry, then the one Patchwork started with.
/// The `%NAME%` references in them are read from the registry too, so a variable an installer
/// added after Patchwork started, such as nvm's `NVM_HOME`, still resolves.
#[cfg(windows)]
fn windows_path() -> Option<std::ffi::OsString> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let keys = [
        RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment")
            .ok(),
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Environment")
            .ok(),
    ];
    let process_var = |name: &str| std::env::var(name).ok();
    // The user's variables win over the system's, as they do at sign-in. Variables Windows sets
    // per session, such as APPDATA, are only in Patchwork's own environment.
    let var = |name: &str| {
        keys.iter()
            .rev()
            .flatten()
            .find_map(|key| key.get_value::<String, _>(name).ok())
            .map(|value| expand_windows_vars(&value, &process_var))
            .or_else(|| process_var(name))
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for value in keys
        .iter()
        .flatten()
        .filter_map(|key| key.get_value::<String, _>("Path").ok())
    {
        paths.extend(std::env::split_paths(&expand_windows_vars(&value, &var)));
    }
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| !p.as_os_str().is_empty() && seen.insert(p.to_string_lossy().to_lowercase()));
    std::env::join_paths(paths).ok()
}

/// `%NAME%` references in a registry value, replaced with the values `var` gives them.
#[cfg(windows)]
pub(crate) fn expand_windows_vars(value: &str, var: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('%') {
            let name = &after[..end];
            match var(name) {
                Some(v) if !name.is_empty() => out.push_str(&v),
                _ => out.push_str(&rest[start..start + end + 2]),
            }
            rest = &after[end + 1..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
        }
    }
    out.push_str(rest);
    out
}

/// Patchwork's environment as `host_command` passes it on, without the AppImage's paths.
#[must_use]
pub fn host_env() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let changes: std::collections::HashMap<_, _> = host_env_changes().into_iter().collect();
    std::env::vars_os()
        .filter_map(|(key, value)| match changes.get(&key) {
            Some(Some(replaced)) => Some((key, replaced.into())),
            Some(None) => None,
            None => Some((key, value)),
        })
        .collect()
}

/// The variables `host_command` changes: each with its value without AppImage entries, or None
/// to remove it.
fn host_env_changes() -> Vec<(std::ffi::OsString, Option<String>)> {
    let appdir = std::env::var("APPDIR").ok().filter(|dir| !dir.is_empty());
    let in_appimage = |entry: &str| {
        appdir.as_deref().is_some_and(|dir| entry.starts_with(dir))
            || entry.split('/').any(|part| part.starts_with(".mount_"))
    };
    let mut changes = Vec::new();
    for (key, value) in std::env::vars_os() {
        let value = value.to_string_lossy();
        if !value.split(':').any(in_appimage) {
            continue;
        }
        let kept: Vec<&str> = value
            .split(':')
            .filter(|entry| !entry.is_empty() && !in_appimage(entry))
            .collect();
        changes.push((key, (!kept.is_empty()).then(|| kept.join(":"))));
    }
    for key in ["APPDIR", "APPIMAGE", "ARGV0", "OWD"] {
        changes.push((key.into(), None));
    }
    changes.extend(
        claude_session_vars(std::env::vars_os().map(|(key, _)| key.to_string_lossy().into_owned()))
            .into_iter()
            .map(|key| (key.into(), None)),
    );
    changes
}

/// The variables a Claude Code session sets for the programs it starts, when Patchwork was itself
/// started from one. Inherited, they make an agent's `claude` act as part of that session (its
/// compact window, effort and session id) or refuse to start as a nested session, and a terminal
/// opened in Patchwork the same. The person's own choices stay: `CLAUDE_CODE_USE_*` (which provider),
/// `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CONFIG_DIR` and `ANTHROPIC_*`.
fn claude_session_vars(keys: impl Iterator<Item = String>) -> Vec<String> {
    let keys: Vec<String> = keys.collect();
    if !keys.iter().any(|key| key == "CLAUDECODE") {
        return Vec::new();
    }
    keys.into_iter()
        .filter(|key| {
            matches!(key.as_str(), "CLAUDECODE" | "CLAUDE_PID" | "CLAUDE_EFFORT")
                || key.starts_with("CLAUDE_AGENT_SDK_")
                || (key.starts_with("CLAUDE_CODE_")
                    && !key.starts_with("CLAUDE_CODE_USE_")
                    && key != "CLAUDE_CODE_OAUTH_TOKEN")
        })
        .collect()
}
