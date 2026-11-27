//! What the installed Codex CLI offers: version, feature flags, and models.
//!
//! Queries the installed binary directly so newer Codex releases show up without updating a
//! static catalog.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use patchwork_provider_core::cli_info::{self, InfoCommand};
use patchwork_provider_core::{AgentKind, CodexFeature, CodexInfo, CodexModel, find_binary, located_or};
use patchwork_platform::Error;

/// The Codex CLI in use: the one the user located, else `codex` on PATH.
#[must_use]
pub fn binary() -> Option<PathBuf> {
    located_or(AgentKind::Codex, || find_binary("codex"))
}

fn run(args: &[&str]) -> Result<String, Error> {
    let binary = binary().ok_or("The Codex CLI is not installed")?;
    cli_info::run(&binary, args)
}

/// Parses `codex features list`.
///
/// Each line is a flag name, its stage (which may be two words, as in `under development`), and
/// whether it is on.
#[must_use]
pub fn parse_features(text: &str) -> Vec<CodexFeature> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            if name.starts_with("Usage") || name.eq_ignore_ascii_case("name") {
                return None;
            }
            let rest: Vec<&str> = parts.collect();
            let (enabled, stage_words) = rest.split_last()?;
            let enabled = match *enabled {
                "true" => true,
                "false" => false,
                _ => return None,
            };
            Some(CodexFeature {
                name: name.to_string(),
                stage: stage_words.join(" "),
                enabled,
            })
        })
        .collect()
}

/// Reads the CLI's feature flags, sorted by name.
///
/// # Errors
///
/// Returns an error when the Codex CLI is not installed or `codex features list` fails.
pub fn features() -> Result<Vec<CodexFeature>, Error> {
    let mut features = parse_features(&run(&["features", "list"])?);
    features.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(features)
}

/// Parses `codex debug models`, which prints `{ "models": [ … ] }`.
#[must_use]
pub fn parse_models(text: &str) -> Vec<CodexModel> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let models = value
        .get("models")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    models
        .iter()
        .filter_map(|model| {
            let slug = model.get("slug")?.as_str()?.to_string();
            let display = model
                .get("display_name")
                .and_then(|v| v.as_str())
                .unwrap_or(&slug)
                .to_string();
            let efforts = model
                .get("supported_reasoning_levels")
                .and_then(|v| v.as_array())
                .map(|levels| {
                    levels
                        .iter()
                        .filter_map(|level| level.get("effort")?.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let speed_tiers = model
                .get("service_tiers")
                .and_then(|v| v.as_array())
                .map(|tiers| {
                    tiers
                        .iter()
                        .filter_map(|tier| {
                            Some(format!(
                                "{} ({})",
                                tier.get("name")?.as_str()?,
                                tier.get("description")?.as_str().unwrap_or_default()
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(CodexModel {
                slug,
                display_name: display,
                context_window: model
                    .get("context_window")
                    .and_then(serde_json::Value::as_u64),
                efforts,
                speed_tiers,
                multi_agent: model
                    .get("multi_agent_version")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                listed: model.get("visibility").and_then(|v| v.as_str()) != Some("hide"),
            })
        })
        .collect()
}

/// The models in the CLI's catalog, from `codex debug models`.
#[must_use]
pub fn models() -> Vec<CodexModel> {
    run(&["debug", "models"])
        .map(|text| parse_models(&text))
        .unwrap_or_default()
}

/// The CLI's version, flags, and models, for the Settings pane.
///
/// # Errors
///
/// Returns an error when the Codex CLI is not installed.
pub fn info() -> Result<CodexInfo, Error> {
    let path = binary().ok_or("The Codex CLI is not installed")?;
    Ok(CodexInfo {
        path: path.to_string_lossy().into_owned(),
        version: cli_info::version(&path),
        features: features().unwrap_or_default(),
        models: models(),
    })
}

/// Runs one of the read-only diagnostics commands. See `cli_info::command`.
///
/// # Errors
///
/// Returns an error when Codex does not offer that command, is not installed, or the command fails,
/// times out, or prints nothing.
pub fn command(kind: InfoCommand) -> Result<String, Error> {
    cli_info::check(
        "Codex",
        kind,
        &[
            InfoCommand::Doctor,
            InfoCommand::McpList,
            InfoCommand::PluginList,
        ],
    )?;
    let binary = binary().ok_or("The Codex CLI is not installed")?;
    cli_info::command(&binary, kind)
}
