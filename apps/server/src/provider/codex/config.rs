//! Where Codex keeps its settings, login and sessions, and the models it offers.

use std::path::{Path, PathBuf};

use crate::provider::AgentKind;
use crate::provider::AgentStatus;
use crate::provider::ModelOption;
use crate::system::fs::home_dir;
use crate::provider::model_manifest;
use crate::provider::probe;

/// `$CODEX_HOME`, else `~/.codex`.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".codex")))
}

pub fn auth(status: &mut AgentStatus) {
    let login = probe(AgentKind::Codex, &["login", "status"])
        .filter(|p| p.success)
        .and_then(|p| {
            p.stdout
                .lines()
                .chain(p.stderr.lines())
                .find(|l| l.contains("Logged in"))
                .map(|l| l.trim().to_string())
        });
    status.signed_in = login.is_some();
    status.detail = login.unwrap_or_else(|| "Not signed in — run `codex login`".into());
}

/// Read only model settings, without launching an inference request.
pub fn configured_model() -> Option<String> {
    let settings: toml::Value =
        toml::from_str(&std::fs::read_to_string(home()?.join("config.toml")).ok()?).ok()?;
    let profile_model = settings
        .get("profile")
        .and_then(toml::Value::as_str)
        .and_then(|profile| settings.get("profiles")?.get(profile)?.get("model"));
    Some(
        profile_model
            .or_else(|| settings.get("model"))?
            .as_str()?
            .to_owned(),
    )
}

/// Current Codex models, from the fetched model manifest, each with the reasoning levels the
/// CLI's own catalog gives it: `extra` and `ultra` are on newer models only.
pub fn models() -> Vec<ModelOption> {
    let catalog = crate::provider::codex::cli::models();
    let mut models = model_manifest::models("codex");
    for model in &mut models {
        if let Some(known) = model
            .id
            .as_deref()
            .and_then(|id| catalog.iter().find(|known| known.slug == id))
        {
            model.efforts = Some(known.efforts.clone());
            model.context_window = known.context_window;
        }
    }
    models
}

/// `(modified time, path)` of every rollout file under `dir`.
pub fn rollout_files(dir: &Path) -> Vec<(std::time::SystemTime, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "jsonl") {
                let modified = entry.metadata().and_then(|m| m.modified());
                out.push((modified.unwrap_or(std::time::UNIX_EPOCH), path));
            }
        }
    }
    out
}
