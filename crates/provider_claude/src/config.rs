//! Where Claude Code keeps its settings and login, and the models it offers.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use patchwork_provider_core::{AgentStatus, ModelOption, PROBE_TIMEOUT, command_at, probe_command};
use patchwork_platform::files::paths::home_dir;
use serde_json::Value;

/// `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
#[must_use]
pub fn home() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".claude")))
}

/// Claude Code's settings file: `.claude.json` in `CLAUDE_CONFIG_DIR`, or in the home folder.
#[must_use]
pub fn settings_file() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir).join(".claude.json"),
        None => home_dir().unwrap_or_default().join(".claude.json"),
    }
}

/// Claude Code's stored credentials: a file on Linux/Windows, the Keychain on macOS.
#[must_use]
pub fn credentials() -> Option<Value> {
    let file = home().and_then(|h| std::fs::read_to_string(h.join(".credentials.json")).ok());
    let raw = file.or_else(|| {
        if !cfg!(target_os = "macos") {
            return None;
        }
        let out = patchwork_platform::host_command("security")
            .args([
                "find-generic-password",
                "-s",
                "Claude Code-credentials",
                "-w",
            ])
            .output()
            .ok()
            .filter(|o| o.status.success())?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    })?;
    serde_json::from_str(raw.trim()).ok()
}

/// Whether `credentials` hold Claude Code's sign-in to the MCP server `name` at `url`.
#[must_use]
pub fn mcp_signed_in(credentials: &Value, name: &str, url: &str) -> bool {
    credentials["mcpOAuth"]
        .as_object()
        .into_iter()
        .flat_map(|entries| entries.values())
        .any(|e| {
            e["serverName"] == name
                && e["serverUrl"] == url
                && e["accessToken"].as_str().is_some_and(|t| !t.is_empty())
        })
}

/// Checks Claude Code sign-in state via `claude auth status`.
pub fn auth(status: &mut AgentStatus) {
    let json = crate::cli::binary()
        .and_then(|bin| {
            let mut cmd = command_at(&bin);
            cmd.env("CLAUDE_CODE_ENTRYPOINT", entrypoint());
            probe_command(cmd, &["auth", "status"], PROBE_TIMEOUT, |_| false)
        })
        .and_then(|p| serde_json::from_str::<Value>(p.stdout.trim()).ok());
    let logged_in = json
        .as_ref()
        .and_then(|v| v["loggedIn"].as_bool())
        .unwrap_or(false);
    if logged_in {
        let method = json
            .as_ref()
            .and_then(|v| v["authMethod"].as_str())
            .unwrap_or("claude.ai");
        status.signed_in = true;
        status.detail = format!("Signed in with {method}");
    } else {
        status.detail = "Not signed in — run `claude` to log in".into();
    }
}

/// Read only model settings, without launching an inference request.
#[must_use]
pub fn configured_model() -> Option<String> {
    if let Ok(model) = std::env::var("ANTHROPIC_MODEL")
        && !model.trim().is_empty()
    {
        return Some(model);
    }
    let settings: Value =
        serde_json::from_str(&std::fs::read_to_string(home()?.join("settings.json")).ok()?).ok()?;
    Some(settings.get("model")?.as_str()?.to_owned())
}

/// The `CLAUDE_CODE_ENTRYPOINT` for the Claude processes Patchwork runs.
///
/// Defaults to `patchwork` unless Claude's `settings.json` `env` sets one.
#[must_use]
pub fn entrypoint() -> String {
    home()
        .and_then(|home| std::fs::read_to_string(home.join("settings.json")).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|settings| {
            Some(
                settings["env"]["CLAUDE_CODE_ENTRYPOINT"]
                    .as_str()?
                    .to_owned(),
            )
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "patchwork".to_owned())
}

/// Current Claude models from the catalog the CLI caches.
#[must_use]
pub fn models() -> Vec<ModelOption> {
    crate::cli::models()
        .into_iter()
        .map(|model| ModelOption {
            id: Some(model.id),
            label: model.name,
            default_model: None,
            is_legacy: model.legacy,
            efforts: Some(model.efforts),
            context_window: model.context_window,
            needs_cli: None,
        })
        .collect()
}
