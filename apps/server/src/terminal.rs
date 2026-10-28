//! What the integrated terminal starts: the shell, its arguments, its folder and its
//! environment, built in the order VS Code's `createTerminalEnvironment` builds them. The
//! desktop shell runs it in a pseudo-terminal.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::types::{DetectLocale, Settings};

/// A shell to start.
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

/// Variables Patchwork sets on itself, which a shell should not start with.
const OWN_ENV: [&str; 1] = ["WEBKIT_DISABLE_DMABUF_RENDERER"];

/// The shell for a terminal in `workspace`, the project folder when there is one.
pub fn launch(settings: &Settings, workspace: Option<&Path>) -> Launch {
    let terminal = &settings.terminal;
    let shell = crate::effective_shell(settings.shell.as_deref()).unwrap_or_else(|| {
        if cfg!(windows) {
            "powershell.exe".into()
        } else {
            "/bin/sh".into()
        }
    });
    let args = terminal.shell_args.clone().unwrap_or_else(|| {
        if cfg!(target_os = "macos") {
            vec!["-l".into()]
        } else {
            Vec::new()
        }
    });

    // The process's environment, or the minimal one.
    let mut env: BTreeMap<String, String> = crate::host_env()
        .into_iter()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .filter(|(key, _)| {
            terminal.inherit_env || cfg!(windows) || MINIMAL_ENV.contains(&key.as_str())
        })
        .filter(|(key, _)| !OWN_ENV.contains(&key.as_str()))
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

    let home = crate::provider::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let base = workspace
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.clone());
    let cwd = terminal
        .cwd
        .as_deref()
        .filter(|c| !c.trim().is_empty())
        .map(|c| base.join(resolve(c, &env, &folder)))
        .filter(|c| c.is_dir())
        .unwrap_or(base);
    env.insert("PWD".into(), cwd.to_string_lossy().into_owned());
    Launch {
        shell,
        args,
        cwd,
        env,
    }
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
