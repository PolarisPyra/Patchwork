//! Where Claude Code keeps its settings and login, and the models it offers.

use std::path::PathBuf;

use serde_json::Value;

use crate::provider::AgentKind;
use crate::provider::AgentStatus;
use crate::provider::ClaudeModel;
use crate::provider::ModelOption;
use crate::system::fs::home_dir;
use crate::provider::model_manifest;
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

/// Current Claude models, from the fetched model manifest, each with the thinking levels the
/// cached catalog says it takes. The manifest names the models; the catalog is the CLI's own
/// record of what each one can do, so a model that cannot think gets no level.
pub fn models() -> Vec<ModelOption> {
    let catalog = crate::provider::claude::cli::models();
    let mut models = model_manifest::models("claude");
    for model in &mut models {
        if let Some(id) = model.id.as_deref() {
            model.efforts = efforts_in(&catalog, id);
        }
    }
    models
}

/// The levels `id` takes, from the catalog. A catalog id matches exactly, or a dated snapshot of
/// it: `claude-haiku-4-5` matches `claude-haiku-4-5-20251001`. Only a date counts as a snapshot,
/// so `claude-opus-5` never takes the levels of `claude-opus-5-5`.
pub(super) fn efforts_in(catalog: &[ClaudeModel], id: &str) -> Option<Vec<String>> {
    // A pinned context window, as in `claude-fable-5-1[1m]`, is not part of the catalog's id.
    let id = id.split('[').next().unwrap_or(id);
    let exact = catalog.iter().find(|model| model.id == id);
    let model = exact.or_else(|| {
        catalog.iter().find(|model| {
            model
                .id
                .strip_prefix(id)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(is_snapshot_date)
        })
    })?;
    Some(model.efforts.clone())
}

/// A catalog id's `YYYYMMDD` snapshot date, which marks a dated copy of a model.
fn is_snapshot_date(text: &str) -> bool {
    text.len() >= 6 && text.chars().all(|c| c.is_ascii_digit())
}
