//! Where Claude Code keeps its settings and login, and the models it offers.

use std::path::PathBuf;

use serde_json::Value;

use crate::provider::AgentKind;
use crate::provider::AgentStatus;
use crate::provider::ModelOption;
use crate::system::fs::home_dir;
use crate::provider::probe;

/// `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".claude")))
}

/// Claude Code's settings file: `.claude.json` in `CLAUDE_CONFIG_DIR`, or in the home folder.
pub fn settings_file() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir).join(".claude.json"),
        None => home_dir().unwrap_or_default().join(".claude.json"),
    }
}

pub fn auth(status: &mut AgentStatus) {
    let json = probe(AgentKind::Claude, &["auth", "status"])
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

/// Current Claude models, from the catalog the CLI caches, each with the thinking levels and
/// context window it gives them, so a model that cannot think gets no level.
pub fn models() -> Vec<ModelOption> {
    crate::provider::claude::cli::models()
        .into_iter()
        .map(|model| ModelOption {
            id: Some(model.id),
            label: model.name,
            default_model: None,
            is_legacy: model.legacy,
            efforts: Some(model.efforts),
            context_window: model.context_window,
        })
        .collect()
}
