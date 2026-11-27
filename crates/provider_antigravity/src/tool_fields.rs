//! Reading a tool call's arguments, paths, summary, and output, however agy names them.

// Rust guideline compliant 2026-11-23

use patchwork_provider_core::{one_line, relative, summarize};
use serde_json::Value;

/// Extracts the argument object from a tool call payload.
#[must_use]
pub fn tool_args(tool: &Value) -> &Value {
    if tool["rawInput"].is_object() {
        &tool["rawInput"]
    } else {
        &tool["args"]
    }
}

/// Returns the first non-empty string field matching one of `keys`.
#[must_use]
pub fn arg_str<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| args[*k].as_str())
        .filter(|s| !s.is_empty())
}

/// Produces a concise one-line summary for a tool call.
#[must_use]
pub fn tool_summary(tool: &Value, args: &Value, root: &str) -> String {
    let path = arg_str(
        args,
        &[
            "Path",
            "path",
            "TargetFile",
            "file_path",
            "filePath",
            "AbsolutePath",
            "DirectoryPath",
            "SearchPath",
        ],
    )
    .or_else(|| tool["locations"].as_array()?.first()?.get("path")?.as_str());
    let command = arg_str(args, &["CommandLine", "command_line", "command", "cmd"]);
    match (path, command) {
        (Some(p), _) => relative(root, p),
        (None, Some(c)) => one_line(c),
        (None, None) => tool["title"]
            .as_str()
            .filter(|t| !t.is_empty())
            .map_or_else(|| summarize(args, root), one_line),
    }
}

/// Collects file paths referenced by a tool call.
#[must_use]
pub fn tool_paths(tool: &Value, args: &Value) -> Vec<String> {
    let mut paths: Vec<String> = tool["locations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|loc| loc["path"].as_str().map(String::from))
        .collect();
    if paths.is_empty()
        && let Some(p) = arg_str(
            args,
            &[
                "Path",
                "path",
                "TargetFile",
                "file_path",
                "filePath",
                "AbsolutePath",
            ],
        )
    {
        paths.push(p.to_string());
    }
    paths
}

/// Extracts textual output from a tool call update.
#[must_use]
pub fn tool_output(update: &Value) -> String {
    let blocks: Vec<&str> = update["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item["content"]["text"]
                .as_str()
                .or_else(|| item["text"].as_str())
        })
        .collect();
    if !blocks.is_empty() {
        return blocks.join("\n");
    }
    let raw = &update["rawOutput"];
    raw["output"]
        .as_str()
        .or_else(|| raw["combinedOutput"].as_str())
        .or_else(|| raw["combined_output"].as_str())
        .or_else(|| raw["error"].as_str())
        .or_else(|| raw.as_str())
        .unwrap_or_default()
        .to_string()
}
