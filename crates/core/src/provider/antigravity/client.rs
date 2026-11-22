//! Locating `agy`, probing auth and models over ACP (`agy acp`), and running diagnostics.

// Rust guideline compliant 2026-11-22

use std::io::{BufReader, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::Error;
use crate::provider::cli_info::{self, InfoCommand};
use crate::provider::{
    AgentKind, AgentStatus, CliInfo, ModelOption, agent_command, find_binary, located_or, probe,
};
use crate::system::Locked;
use crate::system::files::paths::home_dir;
use crate::system::processes::running::{SIGKILL, signal_process_group};

/// Upper bound for the one-shot ACP `initialize` + `session/new` probe.
const ACP_PROBE_TIMEOUT: Duration = Duration::from_secs(15);

/// How long `detect_agent` and `list_models` reuse one ACP session probe.
const PROBE_CACHE_TTL: Duration = Duration::from_secs(5);

/// JSON-RPC error code `agy acp` returns when the user is not signed in.
pub(super) const AUTH_REQUIRED_CODE: i64 = -32000;

/// First `agy` release that exposes all account models to every ACP client regardless of
/// `clientInfo.name`. Older runtimes only offer non-Gemini models to allowlisted clients like Zed.
const OPEN_MODEL_ACCESS_VERSION: (u32, u32, u32) = (1, 5, 1);

/// ACP `clientInfo` for `initialize`: identifies as Patchwork on `agy >= 1.5.1`, and falls back
/// to Zed on older or unparseable versions so non-Gemini models remain available.
pub(super) fn client_info() -> Value {
    static INFO: OnceLock<Value> = OnceLock::new();
    INFO.get_or_init(|| {
        let parsed = antigravity_bin()
            .map(|bin| cli_info::version(&bin))
            .and_then(|v| parse_semver(&v));
        if parsed.is_some_and(|v| v >= OPEN_MODEL_ACCESS_VERSION) {
            json!({
                "name": "patchwork",
                "title": "Patchwork",
                "version": crate::VERSION,
            })
        } else {
            json!({
                "name": "zed",
                "version": "0.0.0",
            })
        }
    })
    .clone()
}

fn parse_semver(raw: &str) -> Option<(u32, u32, u32)> {
    let word = raw
        .split_whitespace()
        .map(|w| w.trim_start_matches('v'))
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))?;
    let mut parts = word.split(['.', '-', '+']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// Suffix on a model id and the reasoning effort it maps to.
pub(super) const EFFORT_SUFFIXES: &[(&str, &str)] = &[
    ("-extra-high", "xhigh"),
    ("-xhigh", "xhigh"),
    ("-high", "high"),
    ("-medium", "medium"),
    ("-low", "low"),
];

/// Outcome of a one-shot ACP probe (`initialize` + `session/new`).
#[derive(Debug, Clone)]
enum AcpProbe {
    SignedIn {
        tier: Option<String>,
        default_model: Option<String>,
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

/// The Antigravity CLI: the one the user located, else `agy` on PATH or in the usual install
/// folders, else where Antigravity's Windows installer puts it by default.
#[must_use]
pub fn antigravity_bin() -> Option<PathBuf> {
    located_or(AgentKind::Antigravity, || {
        find_binary("agy").or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .filter(|_| cfg!(windows))
                .map(|d| PathBuf::from(d).join(r"agy\bin\agy.exe"))
                .filter(|p| p.is_file())
        })
    })
}

/// `~/.gemini`, where Antigravity and the earlier Gemini CLI save sessions and config.
#[must_use]
pub fn home() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".gemini"))
}

/// Strips a reasoning effort suffix from a model id if present.
pub(super) fn strip_effort_suffix(id: &str) -> &str {
    for (suffix, _) in EFFORT_SUFFIXES {
        if let Some(base) = id.strip_suffix(suffix) {
            return base;
        }
    }
    id
}

/// Strips a trailing effort label from a display name.
fn strip_effort_label(name: &str) -> &str {
    for label in [
        " (Extra High)",
        " (XHigh)",
        " (High)",
        " (Medium)",
        " (Low)",
    ] {
        if let Some(base) = name.strip_suffix(label) {
            return base;
        }
    }
    name
}

/// Detects reasoning effort from a model id.
fn effort_from_id(id: &str) -> Option<&'static str> {
    for (suffix, effort) in EFFORT_SUFFIXES {
        if id.ends_with(suffix) {
            return Some(effort);
        }
    }
    None
}

fn sort_efforts(efforts: &mut [String]) {
    efforts.sort_by_key(|e| match e.as_str() {
        "low" => 1,
        "medium" => 2,
        "high" => 3,
        "xhigh" => 4,
        _ => 5,
    });
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

/// The CLI's default base model, as reported by `agy acp`.
#[must_use]
pub fn configured_model() -> Option<String> {
    match probe_acp_cached() {
        AcpProbe::SignedIn { default_model, .. } => default_model,
        AcpProbe::AuthRequired | AcpProbe::Unavailable(_) => None,
    }
}

/// The models the CLI offers, from `agy acp` (falling back to `agy models`), grouped by base
/// model with their supported reasoning efforts.
#[must_use]
pub fn models() -> Vec<ModelOption> {
    if let AcpProbe::SignedIn { models, .. } = probe_acp_cached()
        && !models.is_empty()
    {
        return models;
    }
    let Some(listed) = probe(AgentKind::Antigravity, &["models"]).filter(|p| p.success) else {
        return Vec::new();
    };
    parse_models(&listed.stdout)
}

/// Reads the cached ACP probe or runs a fresh one.
fn probe_acp_cached() -> AcpProbe {
    let cache = probe_cache();
    if let Some((at, cached)) = cache.locked().as_ref()
        && at.elapsed() < PROBE_CACHE_TTL
    {
        return cached.clone();
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
            if v["id"] == 2 {
                let _ = tx.send(v);
                return;
            }
        }
    });
    let response = rx.recv_timeout(ACP_PROBE_TIMEOUT).ok();
    drop(stdin);
    signal_process_group(pid, SIGKILL);
    let _ = child.kill();
    let _ = child.wait();

    let Some(response) = response else {
        return AcpProbe::Unavailable("Antigravity ACP server did not respond in time".into());
    };
    if let Some(error) = response.get("error") {
        let code = error["code"].as_i64().unwrap_or_default();
        let message = error["message"]
            .as_str()
            .unwrap_or("Authentication required");
        if code == AUTH_REQUIRED_CODE || message.to_lowercase().contains("authentication") {
            return AcpProbe::AuthRequired;
        }
        return AcpProbe::Unavailable(message.into());
    }
    let result = &response["result"];
    let tier = result["_meta"]["antigravity"]["tier"]["name"]
        .as_str()
        .map(String::from);
    let (default_model, models) = parse_acp_models(result);
    AcpProbe::SignedIn {
        tier,
        default_model,
        models,
    }
}

/// Parses base models and their per-model effort options from an ACP `session/new` result.
#[must_use]
pub fn parse_acp_models(session_result: &Value) -> (Option<String>, Vec<ModelOption>) {
    let available: Vec<(&str, &str)> = session_result["models"]["availableModels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["modelId"].as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            let name = m["name"].as_str().unwrap_or(id).trim();
            Some((id, name))
        })
        .collect();

    let model_config = session_result["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|opt| opt["id"].as_str() == Some("model"));

    let Some(config) = model_config else {
        let default_model = session_result["models"]["currentModelId"]
            .as_str()
            .map(|id| strip_effort_suffix(id).to_string());
        return (default_model, group_model_pairs(&available));
    };

    let default_model = config["currentValue"]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(String::from);

    let mut out = Vec::new();
    for entry in config["options"].as_array().into_iter().flatten() {
        let Some(base_id) = entry["value"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let label = entry["name"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(base_id)
            .to_string();

        let mut efforts = Vec::new();
        for &(variant_id, _) in &available {
            if let Some(rest) = variant_id.strip_prefix(base_id)
                && let Some(effort) = EFFORT_SUFFIXES
                    .iter()
                    .find_map(|(suffix, effort)| (*suffix == rest).then_some(*effort))
                && !efforts.iter().any(|e| e == effort)
            {
                efforts.push(effort.to_string());
            }
        }
        sort_efforts(&mut efforts);
        out.push(ModelOption {
            id: Some(base_id.to_string()),
            label,
            default_model: None,
            is_legacy: false,
            efforts: Some(efforts),
            context_window: None,
        });
    }
    if out.is_empty() {
        return (default_model, group_model_pairs(&available));
    }
    (default_model, out)
}

/// Parses the tab-separated model list from `agy models`, consolidating effort variants into
/// base models with their supported effort levels.
#[must_use]
pub fn parse_models(stdout: &str) -> Vec<ModelOption> {
    let pairs: Vec<(&str, &str)> = stdout
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(id, name)| (id.trim(), name.trim()))
        .filter(|(id, _)| !id.is_empty())
        .collect();
    group_model_pairs(&pairs)
}

fn group_model_pairs(pairs: &[(&str, &str)]) -> Vec<ModelOption> {
    let mut models: Vec<(String, String, Vec<String>, bool)> = Vec::new();
    for &(id, name) in pairs {
        let base_id = strip_effort_suffix(id);
        let base_name = strip_effort_label(name);
        let effort = effort_from_id(id);

        if let Some(existing) = models.iter_mut().find(|(m_id, _, _, _)| m_id == base_id) {
            if let Some(eff) = effort
                && !existing.2.iter().any(|e| e == eff)
            {
                existing.2.push(eff.to_string());
            }
        } else {
            let mut efforts = Vec::new();
            let had_effort = effort.is_some();
            if let Some(eff) = effort {
                efforts.push(eff.to_string());
            }
            models.push((
                base_id.to_string(),
                base_name.to_string(),
                efforts,
                had_effort,
            ));
        }
    }

    models
        .into_iter()
        .map(|(id, name, mut efforts, had_effort)| {
            sort_efforts(&mut efforts);
            ModelOption {
                id: Some(id),
                label: name,
                default_model: None,
                is_legacy: false,
                efforts: Some(if had_effort { efforts } else { Vec::new() }),
                context_window: None,
            }
        })
        .collect()
}

/// The commands the Antigravity pane offers. It has no flags and no `doctor`.
const INFO_COMMANDS: &[InfoCommand] = &[
    InfoCommand::McpList,
    InfoCommand::PluginList,
    InfoCommand::Changelog,
];

/// The CLI's path, version and models, for the Settings pane.
///
/// # Errors
///
/// Returns an error when the Antigravity CLI is not installed.
pub fn info() -> Result<CliInfo, Error> {
    let path = antigravity_bin().ok_or("The Antigravity CLI is not installed")?;
    let version = cli_info::version(&path);
    Ok(CliInfo {
        path: path.to_string_lossy().into_owned(),
        version,
        settings: Vec::new(),
        models: models(),
    })
}

/// Runs one of the read-only diagnostics commands. See `cli_info::command`.
///
/// # Errors
///
/// Returns an error when Antigravity has no such command, when its CLI is not installed, or when
/// the command fails or takes too long.
pub fn command(kind: InfoCommand) -> Result<String, Error> {
    cli_info::check("Antigravity", kind, INFO_COMMANDS)?;
    let binary = antigravity_bin().ok_or("The Antigravity CLI is not installed")?;
    cli_info::command(&binary, kind)
}
