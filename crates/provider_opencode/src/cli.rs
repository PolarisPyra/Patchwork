//! What the installed OpenCode offers: version, models, settings, and diagnostics.

// Rust guideline compliant 2026-10-09

use patchwork_provider_core::cli_info::{self, InfoCommand};
use patchwork_provider_core::{CliInfo, CliSetting};
use patchwork_platform::Error;
use serde_json::Value;

use crate::{models, opencode_bin};

const COMMANDS: &[InfoCommand] = &[
    InfoCommand::McpList,
    InfoCommand::PluginList,
    InfoCommand::Paths,
    InfoCommand::Agents,
];

const MAX_VALUE: usize = 80;

fn run(args: &[&str]) -> Result<String, Error> {
    let binary = opencode_bin().ok_or("OpenCode is not installed")?;
    cli_info::run(&binary, args)
}

/// Parses `opencode debug config`, a JSON list of the config sources OpenCode read.
#[must_use]
pub fn parse_settings(text: &str) -> Vec<CliSetting> {
    let Ok(Value::Array(sources)) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    sources
        .iter()
        .filter(|source| source.get("type").and_then(Value::as_str) == Some("document"))
        .filter_map(|source| {
            let path = source.get("path").and_then(Value::as_str)?;
            let info = source.get("info").and_then(Value::as_object)?;
            Some(
                info.iter()
                    .filter(|(name, _)| !name.starts_with('$'))
                    .map(|(name, value)| CliSetting {
                        name: name.clone(),
                        value: summarize(value),
                        source: path.to_string(),
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .flatten()
        .collect()
}

fn summarize(value: &Value) -> String {
    match value {
        Value::String(s) if s.chars().count() > MAX_VALUE => {
            format!("{}…", s.chars().take(MAX_VALUE).collect::<String>())
        }
        Value::String(s) => s.clone(),
        Value::Object(map) if map.is_empty() => "{}".into(),
        Value::Object(map) => map.keys().cloned().collect::<Vec<_>>().join(", "),
        Value::Array(list) if list.len() == 1 => "1 entry".into(),
        Value::Array(list) => format!("{} entries", list.len()),
        other => other.to_string(),
    }
}

/// Parses `opencode debug agents` into one line per visible agent.
///
/// # Errors
///
/// Returns an error when `text` is not a valid JSON array.
pub fn parse_agents(text: &str) -> Result<String, Error> {
    let agents: Vec<Value> =
        serde_json::from_str(text).map_err(|e| format!("Could not read OpenCode's agents: {e}"))?;
    let lines: Vec<String> = agents
        .iter()
        .filter(|agent| agent.get("hidden").and_then(Value::as_bool) != Some(true))
        .filter_map(|agent| {
            let name = agent
                .get("name")
                .or_else(|| agent.get("id"))
                .and_then(Value::as_str)?;
            let mode = agent.get("mode").and_then(Value::as_str).unwrap_or("agent");
            let line = match agent.get("description").and_then(Value::as_str) {
                Some(description) if !description.trim().is_empty() => {
                    format!("{name} ({mode}): {}", description.trim())
                }
                _ => format!("{name} ({mode})"),
            };
            Some(line)
        })
        .collect();
    Ok(lines.join("\n"))
}

/// The CLI's path, version, settings and models, for the Settings pane.
///
/// # Errors
///
/// Returns an error when OpenCode is not installed.
pub fn info() -> Result<CliInfo, Error> {
    let path = opencode_bin().ok_or("OpenCode is not installed")?;
    let version = cli_info::version(&path);
    Ok(CliInfo {
        path: path.to_string_lossy().into_owned(),
        version,
        settings: run(&["debug", "config"])
            .map(|text| parse_settings(&text))
            .unwrap_or_default(),
        models: models(),
    })
}

/// Runs one of the read-only diagnostics commands. See `cli_info::command`.
///
/// # Errors
///
/// Returns an error when OpenCode does not offer that command, is not installed, or the command
/// fails, times out, or prints nothing useful.
pub fn command(kind: InfoCommand) -> Result<String, Error> {
    cli_info::check("OpenCode", kind, COMMANDS)?;
    if kind == InfoCommand::Agents {
        return parse_agents(&run(&["debug", "agents"])?);
    }
    let binary = opencode_bin().ok_or("OpenCode is not installed")?;
    cli_info::command(&binary, kind)
}
