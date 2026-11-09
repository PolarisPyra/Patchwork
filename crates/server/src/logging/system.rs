//! Machine and build details attached to bug reports and exported logs.
//!
//! What Patchwork runs on, for a bug report: the app's build, the platform, the desktop session,
//! the hardware's size, the environment the window reads, and each agent CLI's path and version.
//! It is worked out once per launch, written to the launch's `system.json`, and put in exported
//! logs and each chat's report. It names no user or machine, and the home folder reads `~`.

// Rust guideline compliant 2026-10-09

use std::path::Path;
use std::sync::OnceLock;

use serde_json::{json, Value};

use crate::provider::{self, AgentKind};
use crate::system::fs::home_dir;

/// This launch's report, worked out by the first caller.
static REPORT: OnceLock<Value> = OnceLock::new();

/// The window's web engine version, which the desktop shell knows.
static WEBVIEW: OnceLock<String> = OnceLock::new();

/// Environment variables that change how the window draws or how agents start. Their values are
/// switches and names, never secrets.
const ENV: [&str; 10] = [
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "DESKTOP_SESSION",
    "GDK_BACKEND",
    "GDK_SCALE",
    "WEBKIT_DISABLE_DMABUF_RENDERER",
    "WEBKIT_DISABLE_COMPOSITING_MODE",
    "LIBGL_ALWAYS_SOFTWARE",
    "APPIMAGE",
    "FLATPAK_ID",
];

/// Notes the window's web engine version, before the server opens.
pub fn set_webview(version: String) {
    let _ = WEBVIEW.set(version);
}

/// Writes this launch's `system.json` in `session` and logs a summary to `main.log`, on its own
/// thread, since asking each agent CLI its version takes a moment.
pub fn record(session: &Path) {
    let session = session.to_path_buf();
    std::thread::spawn(move || {
        let report = report();
        log::info!(system:% = summary(report); "system: {{system}}");
        let written = serde_json::to_string_pretty(report)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                std::fs::create_dir_all(&session).map_err(|e| e.to_string())?;
                std::fs::write(session.join("system.json"), text).map_err(|e| e.to_string())
            });
        if let Err(e) = written {
            log::warn!(error:% = e; "could not write system.json: {{error}}");
        }
    });
}

/// This launch's report. The first call works it out, and others wait for it.
pub fn report() -> &'static Value {
    REPORT.get_or_init(|| {
        let env: serde_json::Map<String, Value> = ENV
            .iter()
            .filter_map(|name| Some((name.to_string(), json!(std::env::var(name).ok()?))))
            .collect();
        json!({
            "app": "Patchwork",
            "version": crate::VERSION,
            "build": if cfg!(debug_assertions) { "debug" } else { "release" },
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "distro": distro(),
            "kernel": read_trimmed("/proc/sys/kernel/osrelease"),
            "webview": WEBVIEW.get(),
            "cpus": std::thread::available_parallelism().map(std::num::NonZero::get).ok(),
            "memoryMb": memory_mb(),
            "locale": std::env::var("LANG").ok(),
            "shell": std::env::var("SHELL").ok(),
            "wayland": std::env::var_os("WAYLAND_DISPLAY").is_some(),
            "env": env,
            "agents": agents(),
        })
    })
}

/// One line for `main.log`.
fn summary(report: &Value) -> String {
    let text = |key: &str| report[key].as_str().unwrap_or("?").to_string();
    let agents: Vec<String> = report["agents"]
        .as_object()
        .map(|agents| {
            agents
                .iter()
                .map(|(name, agent)| format!("{name} {}", agent["version"].as_str().unwrap_or("?")))
                .collect()
        })
        .unwrap_or_default();
    format!(
        "Patchwork {} ({}) on {} {}, kernel {}, {} session, webview {}; agents: {}",
        text("version"),
        text("build"),
        text("distro"),
        text("arch"),
        text("kernel"),
        report["env"]["XDG_SESSION_TYPE"].as_str().unwrap_or("?"),
        text("webview"),
        if agents.is_empty() {
            "none".into()
        } else {
            agents.join(", ")
        },
    )
}

/// Each agent CLI that is installed: where it is and its version. Patchwork's own agent has none.
fn agents() -> serde_json::Map<String, Value> {
    AgentKind::ALL
        .into_iter()
        .filter(|&kind| kind != AgentKind::Api)
        .filter_map(|kind| {
            let path = provider::agent_command(kind).ok()?.get_program().to_owned();
            let agent = json!({
                "path": shown(Path::new(&path)),
                "version": provider::cli_version(kind),
            });
            Some((format!("{kind:?}"), agent))
        })
        .collect()
}

/// The distribution's own name for itself, such as "CachyOS Linux".
fn distro() -> Option<String> {
    let text = std::fs::read_to_string("/etc/os-release").ok()?;
    let line = text.lines().find(|line| line.starts_with("PRETTY_NAME="))?;
    Some(line["PRETTY_NAME=".len()..].trim_matches('"').to_string())
}

fn memory_mb() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = text.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

fn read_trimmed(path: &str) -> Option<String> {
    Some(std::fs::read_to_string(path).ok()?.trim().to_string())
}

/// `path` with the home folder as `~`, so a report names no user.
pub(crate) fn shown(path: &Path) -> String {
    match home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
