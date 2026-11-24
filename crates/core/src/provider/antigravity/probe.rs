//! One cached `agy acp` session probe for sign-in, plan tier, and default model.

// Rust guideline compliant 2026-11-23

use std::io::{BufReader, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::provider::cli_info;
use crate::provider::{AgentKind, AgentStatus, ModelOption, agent_command};
use crate::system::Locked;
use crate::system::files::paths::home_dir;
use crate::system::processes::running::{SIGKILL, signal_process_group};

use super::cli::{MIN_VERSION, antigravity_bin, is_older_than_minimum};
use super::models::parse_acp_models;

/// Upper bound for the one-shot ACP `initialize` + `session/new` probe.
const ACP_PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// How long `detect_agent` and `list_models` reuse one ACP session probe.
const PROBE_CACHE_TTL: Duration = Duration::from_secs(60);

/// How long a probe that could not run is reused. Unlike an answer about sign-in, a probe that
/// timed out or could not start says nothing about it, so it is asked again soon: a failure at
/// startup, while the machine is busy, would otherwise read as "not signed in" for a minute.
const PROBE_FAILED_TTL: Duration = Duration::from_secs(5);

/// JSON-RPC error code `agy acp` returns when the user is not signed in.
const AUTH_REQUIRED_CODE: i64 = -32000;

/// How long the probe waits for `session/close` before it kills the process anyway.
const PROBE_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// Whether an `agy acp` error means the user has to sign in. Besides its auth code, agy answers
/// `-32603 No browser is available here` when its saved sign-in expired and it cannot open a
/// browser to renew it.
pub(super) fn auth_required(code: Option<i64>, message: &str) -> bool {
    let lower = message.to_lowercase();
    code == Some(AUTH_REQUIRED_CODE)
        || lower.contains("authentication")
        || lower.contains("no browser is available")
}

/// ACP `clientInfo` for `initialize`: always identifies honestly as Patchwork.
pub(super) fn client_info() -> Value {
    json!({
        "name": "patchwork",
        "title": "Patchwork",
        "version": crate::VERSION,
    })
}

/// Outcome of a one-shot ACP probe (`initialize` + `session/new`).
#[derive(Debug, Clone)]
pub(super) enum AcpProbe {
    SignedIn {
        tier: Option<String>,
        default_model: Option<String>,
        default_effort: Option<String>,
        models: Vec<ModelOption>,
    },
    AuthRequired,
    Unavailable(String),
}

/// Cached ACP probe so `detect_agent`, `configured_model`, and `models` share one launch.
fn probe_cache() -> &'static Mutex<Option<(Instant, AcpProbe)>> {
    static CACHE: OnceLock<Mutex<Option<(Instant, AcpProbe)>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

/// Drops the cached ACP probe so the next status or model read checks `agy acp` fresh.
pub(super) fn invalidate_probe_cache() {
    *probe_cache().locked() = None;
}

/// Checks Antigravity sign-in state and tier via `agy acp` (`initialize` + `session/new`).
pub fn auth(status: &mut AgentStatus) {
    if antigravity_bin().is_none() {
        status.signed_in = false;
        status.detail = "Antigravity CLI is not installed".into();
        return;
    }
    match probe_acp_cached() {
        AcpProbe::SignedIn { tier, .. } => {
            status.signed_in = true;
            status.detail = match tier {
                Some(name) if !name.is_empty() => format!("Signed in via ACP · {name}"),
                _ => "Signed in via Antigravity ACP".into(),
            };
        }
        AcpProbe::AuthRequired => {
            status.signed_in = false;
            status.detail = "Sign in with Google to use Antigravity".into();
        }
        AcpProbe::Unavailable(reason) => {
            status.signed_in = false;
            status.detail = reason;
        }
    }
}

/// The signed-in Antigravity plan tier (e.g. "Google AI Pro"), from the cached ACP probe.
#[must_use]
pub(crate) fn tier() -> Option<String> {
    match probe_acp_cached() {
        AcpProbe::SignedIn { tier, .. } => tier.filter(|s| !s.is_empty()),
        AcpProbe::AuthRequired | AcpProbe::Unavailable(_) => None,
    }
}

/// The CLI's default base model, as reported by `agy acp`.
#[must_use]
pub fn configured_model() -> Option<String> {
    match probe_acp_cached() {
        AcpProbe::SignedIn { default_model, .. } => default_model,
        AcpProbe::AuthRequired | AcpProbe::Unavailable(_) => None,
    }
}

/// The model and thinking level agy picks itself, from its last probe however old.
///
/// Reads only what a probe already found, so it never waits on `agy`; `None` before any probe.
#[must_use]
pub fn known_defaults() -> Option<(String, Option<String>)> {
    match probe_cache().locked().as_ref() {
        Some((
            _,
            AcpProbe::SignedIn {
                default_model: Some(model),
                default_effort,
                ..
            },
        )) => Some((model.clone(), default_effort.clone())),
        _ => None,
    }
}

/// Reads the cached ACP probe or runs a fresh one. Callers arriving while a probe runs wait for
/// it and share its result instead of each starting `agy`.
pub(super) fn probe_acp_cached() -> AcpProbe {
    static RUNNING: Mutex<()> = Mutex::new(());
    let cache = probe_cache();
    let cached = || {
        cache
            .locked()
            .as_ref()
            .filter(|(at, probe)| {
                let ttl = match probe {
                    AcpProbe::Unavailable(_) => PROBE_FAILED_TTL,
                    _ => PROBE_CACHE_TTL,
                };
                at.elapsed() < ttl
            })
            .map(|(_, probe)| probe.clone())
    };
    if let Some(probe) = cached() {
        return probe;
    }
    let _running = RUNNING.locked();
    if let Some(probe) = cached() {
        return probe;
    }
    let fresh = run_acp_probe();
    *cache.locked() = Some((Instant::now(), fresh.clone()));
    fresh
}

/// Runs a short-lived `agy acp --stdio` session to check authentication and read models.
fn run_acp_probe() -> AcpProbe {
    let Ok(mut cmd) = agent_command(AgentKind::Antigravity) else {
        return AcpProbe::Unavailable("Antigravity CLI is not installed".into());
    };
    if let Some(path) = antigravity_bin() {
        let version = cli_info::version(&path);
        if is_older_than_minimum(&version) {
            return AcpProbe::Unavailable(format!(
                "Antigravity CLI {version} is too old. Run `agy update` to get {MIN_VERSION} \
                 or newer."
            ));
        }
    }
    let cwd = home_dir().unwrap_or_else(|| PathBuf::from("."));
    cmd.args(["acp", "--stdio"])
        .current_dir(&cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    cmd.process_group(0);
    let Ok(mut child) = cmd.spawn() else {
        return AcpProbe::Unavailable("Could not start Antigravity ACP server".into());
    };
    let pid = child.id();
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        signal_process_group(pid, SIGKILL);
        let _ = child.kill();
        let _ = child.wait();
        return AcpProbe::Unavailable("Antigravity ACP stdio is unavailable".into());
    };
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientInfo": client_info(),
            "clientCapabilities": {
                "fs": {"readTextFile": false, "writeTextFile": false},
                "terminal": false,
            },
        },
    });
    let session = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/new",
        "params": {
            "cwd": cwd.to_string_lossy(),
            "mcpServers": [],
        },
    });
    let wrote = writeln!(stdin, "{init}")
        .and_then(|()| writeln!(stdin, "{session}"))
        .and_then(|()| stdin.flush())
        .is_ok();
    if !wrote {
        signal_process_group(pid, SIGKILL);
        let _ = child.kill();
        let _ = child.wait();
        return AcpProbe::Unavailable("Could not write to Antigravity ACP server".into());
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in crate::system::lossy_lines(BufReader::new(stdout)) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if v["id"] == 2 || v["id"] == 3 {
                let _ = tx.send(v);
            }
        }
    });
    let response = rx.recv_timeout(ACP_PROBE_TIMEOUT).ok();
    // Close the probe's session so agy does not keep it open as a conversation of its own.
    if let Some(id) = response
        .as_ref()
        .and_then(|r| r["result"]["sessionId"].as_str())
    {
        let close = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session/close",
            "params": {"sessionId": id},
        });
        if writeln!(stdin, "{close}")
            .and_then(|()| stdin.flush())
            .is_ok()
        {
            let _ = rx.recv_timeout(PROBE_CLOSE_TIMEOUT);
        }
    }
    drop(stdin);
    signal_process_group(pid, SIGKILL);
    let _ = child.kill();
    let _ = child.wait();

    let Some(response) = response else {
        return AcpProbe::Unavailable("Antigravity ACP server did not respond in time".into());
    };
    if let Some(error) = response.get("error") {
        let message = error["message"]
            .as_str()
            .unwrap_or("Authentication required");
        if auth_required(error["code"].as_i64(), message) {
            return AcpProbe::AuthRequired;
        }
        return AcpProbe::Unavailable(message.into());
    }
    let result = &response["result"];
    let tier = result["_meta"]["antigravity"]["tier"]["name"]
        .as_str()
        .map(String::from);
    let (default_model, models) = parse_acp_models(result);
    let default_effort = result["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|opt| opt["id"].as_str() == Some("thought_level"))
        .and_then(|opt| opt["currentValue"].as_str())
        .filter(|v| !v.is_empty())
        .map(String::from);
    AcpProbe::SignedIn {
        tier,
        default_model,
        default_effort,
        models,
    }
}
