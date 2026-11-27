//! What the installed Claude Code offers: version, feature gates, and models.
//!
//! Reads the CLI's version, cached GrowthBook feature gates, cached model catalog, and read-only
//! diagnostics commands.

// Rust guideline compliant 2026-10-09

use std::collections::HashSet;
use std::path::PathBuf;

use patchwork_provider_core::cli_info::{self, InfoCommand};
use patchwork_provider_core::{
    AgentKind, ClaudeFeature, ClaudeInfo, ClaudeModel, find_binary, located_or,
};
use patchwork_platform::Error;
use serde_json::Value;

/// The Claude Code in use: the one the user located, else `claude` on PATH.
#[must_use]
pub fn binary() -> Option<PathBuf> {
    located_or(AgentKind::Claude, || find_binary("claude"))
}

/// Longest a gate's value may be on its line in the pane.
const MAX_VALUE: usize = 40;

/// The feature gates Claude Code has resolved and cached in its settings file.
///
/// # Errors
///
/// Returns an error when Claude Code's settings file cannot be read or is not valid JSON.
pub fn features() -> Result<Vec<ClaudeFeature>, Error> {
    let file = crate::settings_file();
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("Could not read {}: {e}", file.display()))?;
    let config: Value =
        serde_json::from_str(&text).map_err(|e| format!("{} is not JSON: {e}", file.display()))?;
    Ok(parse_features(&config))
}

/// Parses the `cachedGrowthBookFeatures` map of Claude Code's settings file.
#[must_use]
pub fn parse_features(config: &Value) -> Vec<ClaudeFeature> {
    let Some(gates) = config
        .get("cachedGrowthBookFeatures")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    let mut features: Vec<ClaudeFeature> = gates
        .iter()
        .map(|(name, value)| ClaudeFeature {
            name: name.clone(),
            value: short_value(value),
            enabled: value.as_bool(),
        })
        .collect();
    features.sort_by(|a, b| a.name.cmp(&b.name));
    features
}

fn short_value(value: &Value) -> String {
    let json = value.to_string();
    if json.len() <= MAX_VALUE {
        return json;
    }
    match value {
        Value::Array(items) => format!("[{} items]", items.len()),
        Value::Object(map) => format!("{{{} keys}}", map.len()),
        Value::String(_) => format!("{}…", truncate(&json, MAX_VALUE)),
        _ => json,
    }
}

fn truncate(text: &str, chars: usize) -> &str {
    match text.char_indices().nth(chars) {
        Some((end, _)) => &text[..end],
        None => text,
    }
}

/// The models in the newest catalog Claude Code has cached.
///
/// Prefers the terminal's catalog when the CLI and desktop app have each fetched one.
#[must_use]
pub fn models() -> Vec<ClaudeModel> {
    let Some(dir) = crate::home().map(|home| home.join("cache").join("model-catalog")) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let text = std::fs::read_to_string(entry.path()).ok()?;
            let catalog: Value = serde_json::from_str(&text).ok()?;
            let terminal =
                catalog.pointer("/catalog/surface").and_then(Value::as_str) == Some("cc");
            let fetched = catalog
                .get("fetchedAt")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            Some(((terminal, fetched), catalog))
        })
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, catalog)| parse_models(&catalog))
        .unwrap_or_default()
}

/// Parses a cached catalog: `{ "catalog": { "config": { "models": [ … ] } } }`.
#[must_use]
pub fn parse_models(catalog: &Value) -> Vec<ClaudeModel> {
    let mut lines = HashSet::new();
    catalog
        .pointer("/catalog/config/models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| {
                    let mut parsed = parse_model(model)?;
                    let line = model.get("short_name").and_then(Value::as_str);
                    let superseded = line.is_none_or(|line| !lines.insert(line));
                    parsed.legacy &= superseded;
                    Some(parsed)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_model(model: &Value) -> Option<ClaudeModel> {
    let id = model.get("id")?.as_str()?.to_string();
    Some(ClaudeModel {
        name: model
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&id)
            .to_string(),
        context_window: model
            .pointer("/runtime/max_input_tokens")
            .and_then(Value::as_u64),
        efforts: model
            .pointer("/thinking/effort_options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| option.get("id")?.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        fast_mode: model.get("fast_mode").is_some_and(|mode| !mode.is_null()),
        legacy: model
            .get("section")
            .and_then(Value::as_str)
            .is_some_and(|section| section != "main"),
        id,
    })
}

/// The CLI's path and version, its gates and models, for the Settings pane.
///
/// # Errors
///
/// Returns an error when Claude Code is not installed.
pub fn info() -> Result<ClaudeInfo, Error> {
    let path = binary().ok_or("Claude Code is not installed")?;
    Ok(ClaudeInfo {
        version: cli_info::version(&path),
        path: path.to_string_lossy().into_owned(),
        features: features().unwrap_or_default(),
        models: models(),
    })
}

/// Runs one of the read-only diagnostics commands. See `cli_info::command`.
///
/// # Errors
///
/// Returns an error when Claude Code does not offer that command, is not installed, or the command
/// fails, times out, or prints nothing.
pub fn command(kind: InfoCommand) -> Result<String, Error> {
    cli_info::check(
        "Claude Code",
        kind,
        &[
            InfoCommand::Doctor,
            InfoCommand::McpList,
            InfoCommand::PluginList,
        ],
    )?;
    let binary = binary().ok_or("Claude Code is not installed")?;
    cli_info::command(&binary, kind)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_features_and_models() {
        let cfg = json!({
            "cachedGrowthBookFeatures": {
                "beta_flag": true,
                "poll_ms": 500
            }
        });
        let feats = parse_features(&cfg);
        assert_eq!(feats.len(), 2);
        assert_eq!(feats[0].name, "beta_flag");
        assert_eq!(feats[0].enabled, Some(true));
        assert_eq!(feats[1].enabled, None);

        let catalog = json!({
            "catalog": {
                "config": {
                    "models": [
                        {
                            "id": "claude-sonnet-5",
                            "name": "Sonnet 5",
                            "short_name": "sonnet",
                            "section": "main",
                            "runtime": { "max_input_tokens": 200_000 },
                            "thinking": { "effort_options": [{ "id": "high" }] }
                        },
                        {
                            "id": "claude-sonnet-4",
                            "name": "Sonnet 4",
                            "short_name": "sonnet",
                            "section": "legacy"
                        }
                    ]
                }
            }
        });
        let models = parse_models(&catalog);
        assert_eq!(models.len(), 2);
        assert!(!models[0].legacy);
        assert!(models[1].legacy);
        assert_eq!(models[0].efforts, vec!["high"]);
    }
}
