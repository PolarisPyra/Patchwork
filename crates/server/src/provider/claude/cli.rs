//! What the installed Claude Code offers: version, feature gates, and models.
//!
//! What the installed Claude Code offers, read from the CLI itself: its version, the feature gates
//! and model catalog it has cached, and the read-only diagnostics commands. Claude Code has no
//! command that lists its gates or models, so Patchwork reads the files the CLI itself keeps
//! rather than a copy of either list, and a newer Claude Code shows up as it is.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::provider::cli_info::{self, InfoCommand};
use crate::provider::{find_binary, located_or};
use crate::provider::{AgentKind, ClaudeFeature, ClaudeInfo, ClaudeModel};

/// The Claude Code in use: the one the user located, else `claude` on PATH.
#[must_use]
pub fn binary() -> Option<PathBuf> {
    located_or(AgentKind::Claude, || find_binary("claude"))
}

/// The CLI's version line, e.g. `2.1.310 (Claude Code)`.
fn version(binary: &Path) -> Result<String, Error> {
    let text = cli_info::run(binary, &["--version"])?;
    Ok(text.lines().next().unwrap_or_default().trim().to_string())
}

/// Longest a gate's value may be on its line in the pane.
const MAX_VALUE: usize = 40;

/// The feature gates Claude Code has resolved and cached in its settings file.
///
/// The CLI asks GrowthBook for them and writes the answer to `.claude.json`, so this is the set the
/// CLI is running with, not a list of names compiled into it.
///
/// # Errors
///
/// Returns an error when Claude Code's settings file cannot be read or is not valid JSON.
pub fn features() -> Result<Vec<ClaudeFeature>, Error> {
    let file = crate::provider::claude::settings_file();
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("Could not read {}: {e}", file.display()))?;
    let config: Value =
        serde_json::from_str(&text).map_err(|e| format!("{} is not JSON: {e}", file.display()))?;
    Ok(parse_features(&config))
}

/// Parses the `cachedGrowthBookFeatures` map of Claude Code's settings file: each gate's name,
/// the value it holds, and whether that value is a plain switch that is on.
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

/// A gate's value as one short line: the JSON when it fits, else a count of what it holds.
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

/// The first `chars` characters of `text`.
fn truncate(text: &str, chars: usize) -> &str {
    match text.char_indices().nth(chars) {
        Some((end, _)) => &text[..end],
        None => text,
    }
}

/// The models in the newest catalog Claude Code has cached.
///
/// The models in the newest catalog Claude Code has cached, with the terminal's catalog preferred
/// when the CLI and the desktop app have each fetched one. The CLI fetches the catalog from the
/// API and keeps it in `cache/model-catalog`.
#[must_use]
pub fn models() -> Vec<ClaudeModel> {
    let Some(dir) =
        crate::provider::claude::home().map(|home| home.join("cache").join("model-catalog"))
    else {
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
///
/// The catalog lists the newest model of each line first. A model outside the main section is
/// legacy only when one listed before it has the same short name, such as an older Opus: a line
/// the main section leaves out, such as an early-access model, stays with the current ones.
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
        version: version(&path).unwrap_or_else(|_| "unknown".into()),
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
    use super::*;

    #[test]
    fn parses_the_cached_gates() {
        let config: Value = serde_json::from_str(
            r#"{
                "numStartups": 40,
                "cachedGrowthBookFeatures": {
                    "tengu_streaming_tool_execution2": true,
                    "tengu_quiet_basalt_echo": false,
                    "tengu_cozy_teapot": "strict",
                    "tengu_bridge_poll_interval_ms": 0,
                    "tengu_nifty_lemur": {"enabled": false, "version": 0},
                    "tengu_porch_bell": null
                }
            }"#,
        )
        .expect("the sample is JSON");
        let features = parse_features(&config);
        assert_eq!(features.len(), 6);
        // Sorted by name, whatever order the file holds.
        assert_eq!(features[0].name, "tengu_bridge_poll_interval_ms");
        assert_eq!(features[0].value, "0");
        assert_eq!(features[0].enabled, None);
        let streaming = features
            .iter()
            .find(|feature| feature.name == "tengu_streaming_tool_execution2")
            .expect("the gate is listed");
        assert_eq!(streaming.enabled, Some(true));
        assert_eq!(streaming.value, "true");
        // A gate that carries a setting has no on or off, and shows what it holds.
        let teapot = features
            .iter()
            .find(|feature| feature.name == "tengu_cozy_teapot")
            .expect("the gate is listed");
        assert_eq!(teapot.enabled, None);
        assert_eq!(teapot.value, "\"strict\"");
        let lemur = features
            .iter()
            .find(|feature| feature.name == "tengu_nifty_lemur")
            .expect("the gate is listed");
        assert_eq!(lemur.value, r#"{"enabled":false,"version":0}"#);
        assert_eq!(lemur.enabled, None);
    }

    #[test]
    fn a_long_value_is_summarised() {
        let long_object = format!(r#"{{"a":"{}"}}"#, "x".repeat(80));
        let long_list = format!(r#"["{}","{}"]"#, "y".repeat(40), "z".repeat(40));
        let config: Value = serde_json::from_str(&format!(
            r#"{{"cachedGrowthBookFeatures":{{"tengu_a":{long_object},"tengu_b":{long_list}}}}}"#
        ))
        .expect("the sample is JSON");
        let features = parse_features(&config);
        assert_eq!(features[0].value, "{1 keys}");
        assert_eq!(features[1].value, "[2 items]");
    }

    #[test]
    fn a_settings_file_without_gates_lists_none() {
        let config: Value = serde_json::from_str(r#"{"numStartups":3}"#).expect("JSON");
        assert!(parse_features(&config).is_empty());
    }

    #[test]
    fn parses_the_model_catalog() {
        let catalog: Value = serde_json::from_str(
            r#"{"catalog":{"surface":"cc","config":{"models":[
                {"id":"claude-opus-5-5","name":"Opus 5.5","section":"main",
                 "thinking":{"type":"effort","effort_options":[{"id":"low"},{"id":"high"}]},
                 "runtime":{"max_input_tokens":1000000,"max_output_tokens":128000},
                 "fast_mode":{"type":"toggle"}},
                {"id":"claude-haiku-4-5","thinking":{"type":"none"},
                 "runtime":{"max_input_tokens":200000}}
            ]}}}"#,
        )
        .expect("the sample is JSON");
        let models = parse_models(&catalog);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "Opus 5.5");
        assert_eq!(models[0].context_window, Some(1_000_000));
        assert_eq!(models[0].efforts, vec!["low", "high"]);
        assert!(models[0].fast_mode);
        // A model without a name lists under its id, and one that does not think has no efforts.
        assert_eq!(models[1].name, "claude-haiku-4-5");
        assert_eq!(models[1].context_window, Some(200_000));
        assert!(models[1].efforts.is_empty());
        assert!(!models[1].fast_mode);
    }

    #[test]
    fn rubbish_is_not_a_catalog() {
        assert!(parse_models(&Value::Null).is_empty());
        assert!(parse_models(&serde_json::json!({ "catalog": {} })).is_empty());
    }

    /// Ignored by default: it reads the Claude Code this machine has, if it has one.
    /// `cargo test -p patchwork-server -- --ignored reads_the_installed_claude`.
    #[test]
    #[ignore = "needs Claude Code"]
    fn reads_the_installed_claude() {
        let info = info().expect("Claude Code answers");
        assert!(info.version.contains("Claude Code"), "{}", info.version);
        assert!(
            info.features
                .iter()
                .any(|feature| feature.enabled == Some(true)),
            "the gates parse: {} gates",
            info.features.len()
        );
        assert!(
            info.features
                .iter()
                .all(|feature| !feature.value.is_empty()),
            "every gate shows a value"
        );
        assert!(!info.models.is_empty(), "the model catalog parses");
        assert!(
            info.models
                .iter()
                .any(|model| model.context_window.is_some())
        );
        let doctor = command(InfoCommand::Doctor).expect("claude doctor answers");
        assert_ne!(doctor.trim(), "");
        eprintln!(
            "{}: {} gates ({} on), {} models",
            info.version,
            info.features.len(),
            info.features
                .iter()
                .filter(|feature| feature.enabled == Some(true))
                .count(),
            info.models.len()
        );
    }
}
