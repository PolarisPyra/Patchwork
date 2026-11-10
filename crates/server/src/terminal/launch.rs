//! The shell a terminal starts, with its arguments, folder and environment.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::TerminalShell;
use crate::Error;
use crate::settings::{DetectLocale, Settings};

/// A shell to start.
#[derive(Debug)]
pub struct Launch {
    pub shell: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
}

/// What `inherit_env: false` keeps of Patchwork's environment: enough to log in and reach the
/// desktop session.
const MINIMAL_ENV: [&str; 12] = [
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "PATH",
    "LANG",
    "TMPDIR",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "SystemRoot",
];

/// The shells offered after the user's own, each where the PATH finds it.
const SHELL_CANDIDATES: [&str; 6] = ["zsh", "bash", "fish", "pwsh", "powershell", "cmd"];

/// The user's shell: SHELL, else the account's login shell, else `/bin/sh`; on Windows, the
/// `ComSpec`, which is cmd.
fn default_shell() -> String {
    if cfg!(windows) {
        return std::env::var("ComSpec")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "cmd.exe".into());
    }
    crate::system::login_var("SHELL")
        .or_else(crate::system::login_shell)
        .unwrap_or_else(|| "/bin/sh".into())
}

/// Where `shell` is: the file it names, else the program by that name on the PATH.
fn find_shell(shell: &str) -> Option<String> {
    let path = Path::new(shell);
    if path.components().count() > 1 {
        return path.is_file().then(|| shell.to_string());
    }
    crate::system::find_shell(shell)
}

/// The shells a terminal can start: the user's first, then those of [`SHELL_CANDIDATES`] that
/// are installed, each kind once, so `/usr/bin/bash` is not listed again beside `/bin/bash`.
#[must_use]
pub fn shells() -> Vec<TerminalShell> {
    let mut found: Vec<TerminalShell> = Vec::new();
    let default = find_shell(&default_shell());
    for path in default
        .into_iter()
        .chain(SHELL_CANDIDATES.iter().filter_map(|name| find_shell(name)))
    {
        let name = crate::system::shell_name(&path);
        if !found
            .iter()
            .any(|s| crate::system::shell_name(&s.path) == name)
        {
            found.push(TerminalShell {
                name: file_name(&path),
                path,
            });
        }
    }
    found
}

pub(super) fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// Builds the shell launch for a terminal in `workspace`.
///
/// The shell for a terminal in `workspace`, the project folder when there is one: `shell`, else
/// the user's, interactive, so its startup files and completion are its own. A shell that is not
/// there is an error rather than another shell.
///
/// # Errors
///
/// Returns an error when the chosen shell is not installed.
pub fn launch(
    settings: &Settings,
    workspace: Option<&Path>,
    shell: Option<&str>,
) -> Result<Launch, Error> {
    let terminal = &settings.terminal;
    let wanted = shell.map_or_else(default_shell, str::to_string);
    let shell = find_shell(&wanted).ok_or_else(|| format!("{wanted} is not installed"))?;
    let args: Vec<String> = match crate::system::shell_name(&shell).as_str() {
        "cmd" => Vec::new(),
        "pwsh" | "powershell" => vec!["-NoLogo".into()],
        _ => vec!["-i".into()],
    };

    // The process's environment with the one the user's shell sets up, or the minimal one.
    let mut env: BTreeMap<String, String> = crate::system::host_env()
        .into_iter()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .chain(crate::system::login_env().iter().cloned())
        .filter(|(key, _)| {
            terminal.inherit_env || cfg!(windows) || MINIMAL_ENV.contains(&key.as_str())
        })
        .collect();

    // The settings' variables, resolved against that environment; null removes one.
    let folder = workspace
        .map(|w| w.to_string_lossy().into_owned())
        .unwrap_or_default();
    let resolved: Vec<(String, Option<String>)> = terminal
        .env
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value.as_deref().map(|v| resolve(v, &env, &folder)),
            )
        })
        .collect();
    for (key, value) in resolved {
        match value {
            Some(value) => env.insert(key, value),
            None => env.remove(&key),
        };
    }

    // The terminal's own variables.
    env.insert("TERM".into(), "xterm-256color".into());
    env.insert("COLORTERM".into(), "truecolor".into());
    env.insert("TERM_PROGRAM".into(), "patchwork".into());
    env.insert("TERM_PROGRAM_VERSION".into(), crate::VERSION.into());
    let lang = env.get("LANG").cloned().unwrap_or_default();
    let utf8 =
        lang.to_ascii_lowercase().contains("utf-8") || lang.to_ascii_lowercase().contains("utf8");
    let set_lang = match terminal.detect_locale {
        DetectLocale::On => true,
        DetectLocale::Auto => !utf8,
        DetectLocale::Off => false,
    };
    if set_lang {
        let language = lang
            .split(['.', '@'])
            .next()
            .filter(|l| l.contains('_'))
            .unwrap_or("en_US");
        env.insert("LANG".into(), format!("{language}.UTF-8"));
    }

    let home = crate::system::fs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let base = workspace.map_or_else(|| home.clone(), Path::to_path_buf);
    let cwd = terminal
        .cwd
        .as_deref()
        .filter(|c| !c.trim().is_empty())
        .map(|c| base.join(resolve(c, &env, &folder)))
        .filter(|c| c.is_dir())
        .unwrap_or(base);
    env.insert("PWD".into(), cwd.to_string_lossy().into_owned());
    Ok(Launch {
        shell,
        args,
        cwd,
        env,
    })
}

/// `${env:NAME}` and `${workspaceFolder}` in a setting; any other `${…}` is left as written.
fn resolve(value: &str, env: &BTreeMap<String, String>, folder: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find('}') else {
            rest = &rest[start..];
            break;
        };
        let name = &rest[start + 2..start + len];
        match name.strip_prefix("env:") {
            Some(var) => out.push_str(env.get(var).map_or("", String::as_str)),
            None if name == "workspaceFolder" => out.push_str(folder),
            None => out.push_str(&rest[start..=start + len]),
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}
