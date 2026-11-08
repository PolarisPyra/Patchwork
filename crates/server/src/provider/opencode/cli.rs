//! What the installed OpenCode offers, read from the CLI itself: its version, its models, the
//! settings in the config files it reads, and its read-only diagnostics commands. OpenCode has no
//! feature flags and no `doctor`; `debug paths` and `debug agents` stand in for it.

use serde_json::Value;

use super::{models, opencode_bin};
use crate::provider::cli_info::{self, InfoCommand};
use crate::provider::{CliInfo, CliSetting};

/// The commands the OpenCode pane offers.
const COMMANDS: &[InfoCommand] = &[
    InfoCommand::McpList,
    InfoCommand::PluginList,
    InfoCommand::Paths,
    InfoCommand::Agents,
];

/// Longest a plain setting is shown before it is cut.
const MAX_VALUE: usize = 80;

/// Runs OpenCode with `args`. See `cli_info::run`.
fn run(args: &[&str]) -> Result<String, String> {
    let binary = opencode_bin().ok_or("OpenCode is not installed")?;
    cli_info::run(&binary, args)
}

/// Parses `opencode debug config`, a JSON list of the config sources OpenCode read. Each file's
/// top-level keys become one setting each, in the order OpenCode reads the files; folders are
/// left out, since `debug paths` shows them.
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

/// A setting's value in short. See `CliSetting::value`.
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

/// Parses `opencode debug agents` into one line per agent the user can see: its name, whether it
/// is a primary agent or a subagent, and its description.
pub fn parse_agents(text: &str) -> Result<String, String> {
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
pub fn info() -> Result<CliInfo, String> {
    let path = opencode_bin().ok_or("OpenCode is not installed")?;
    let version = run(&["--version"])
        .map(|text| text.lines().next().unwrap_or_default().trim().to_string())
        .unwrap_or_else(|_| "unknown".into());
    Ok(CliInfo {
        path: path.to_string_lossy().into_owned(),
        version,
        settings: run(&["debug", "config"])
            .map(|text| parse_settings(&text))
            .unwrap_or_default(),
        models: models(),
    })
}

/// Runs one of the read-only diagnostics commands. See `cli_info::command`. The agents come as
/// JSON with each agent's whole prompt, so they are read into a list instead.
pub fn command(kind: InfoCommand) -> Result<String, String> {
    cli_info::check("OpenCode", kind, COMMANDS)?;
    if kind == InfoCommand::Agents {
        return parse_agents(&run(&["debug", "agents"])?);
    }
    let binary = opencode_bin().ok_or("OpenCode is not installed")?;
    cli_info::command(&binary, kind)
}
