//! Requests agy sends Patchwork: permission prompts and questions, mapped to approvals.

// Rust guideline compliant 2026-11-23

use std::path::Path;

use serde_json::{Value, json};

use crate::permissions::{PermissionKind, Target};
use crate::provider::{Parsed, TurnSpec, one_line, relative};

use super::edits::edit_lines;
use super::questions::question;
use super::tool_fields::{arg_str, tool_args, tool_paths, tool_summary};

pub(super) fn handle_server_request(v: &Value, spec: &TurnSpec) -> Parsed {
    // Qwen Code asks for prompts queued during its reply, which Patchwork sends as steers, and
    // whether it may start a turn of its own.
    let answer = match v["method"].as_str() {
        Some("craft/drainMidTurnQueue") => Some(json!({"messages": [], "hasQueuedPrompt": false})),
        Some("_qwencode/start_turn") => Some(json!({"accepted": true})),
        _ => None,
    };
    if let Some(result) = answer {
        return Parsed::Send(
            json!({"jsonrpc": "2.0", "id": v["id"], "result": result}).to_string(),
        );
    }
    if v["method"].as_str() != Some("session/request_permission") {
        return Parsed::Send(
            json!({
                "jsonrpc": "2.0",
                "id": v["id"],
                "error": {"code": -32601, "message": "Method not supported by Patchwork"},
            })
            .to_string(),
        );
    }

    if let Some(question) = question(v) {
        return question;
    }

    let params = &v["params"];
    let tool = &params["toolCall"];
    let options = params["options"].as_array().map_or(&[][..], Vec::as_slice);

    // Patchwork asks about each call, so a one-off answer is preferred; a lasting one only
    // when agy offers nothing else.
    let kind = |kind: &str| {
        options
            .iter()
            .find(move |o| o["kind"].as_str() == Some(kind))
    };
    let allow_option = kind("allow_once")
        .or_else(|| kind("allow_always"))
        .or_else(|| {
            options
                .iter()
                .find(|o| o["optionId"].as_str() == Some("allow"))
        })
        .and_then(|o| o["optionId"].as_str());
    let deny_option = kind("reject_once")
        .or_else(|| kind("reject_always"))
        .or_else(|| {
            options
                .iter()
                .find(|o| o["optionId"].as_str() == Some("deny"))
        })
        .and_then(|o| o["optionId"].as_str());

    let (title, detail, target) = permission_call(tool, spec.root);
    Parsed::Approval {
        title,
        detail: one_line(&detail),
        reply: json!({
            "id": v["id"],
            "allow_option": allow_option,
            "deny_option": deny_option,
        }),
        questions: Vec::new(),
        target,
    }
}

/// Maps an ACP `session/request_permission` tool call to a Patchwork approval title, detail, and
/// permission rule target.
fn permission_call(tool: &Value, root: &str) -> (String, String, Option<Target>) {
    let name = tool["name"].as_str().unwrap_or_default();
    let kind = tool["kind"].as_str().unwrap_or_default();
    let args = tool_args(tool);

    if kind == "execute" || matches!(name, "run_command" | "execute_command" | "shell") {
        let command = arg_str(args, &["CommandLine", "command_line", "command", "cmd"])
            .or_else(|| tool["title"].as_str())
            .unwrap_or_default();
        let target = (!command.is_empty())
            .then(|| Target::new(PermissionKind::Command, [command.to_string()]));
        return ("Run command".into(), command.into(), target);
    }

    if kind == "edit"
        || matches!(
            name,
            "write_file"
                | "write_to_file"
                | "edit_file"
                | "replace_file_content"
                | "multi_replace_file_content"
                | "create_file"
                | "delete_file"
        )
    {
        let paths = tool_paths(tool, args);
        let detail = if paths.is_empty() {
            tool["title"]
                .as_str()
                .unwrap_or("Edit workspace files")
                .to_string()
        } else {
            paths
                .iter()
                .map(|p| relative(root, p))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let detail = match edit_lines(tool) {
            Some(lines) => format!("{detail} (+{} −{})", lines.added, lines.removed),
            None => detail,
        };
        let target = (!paths.is_empty()).then(|| Target::new(PermissionKind::Edit, paths));
        return ("Edit files".into(), detail, target);
    }

    if kind == "read"
        || matches!(
            name,
            "read_file" | "view_file" | "list_dir" | "grep_search" | "find_by_name"
        )
    {
        let paths = tool_paths(tool, args);
        if let Some(first) = paths.first() {
            let folder = if name.contains("file") {
                Path::new(first)
                    .parent()
                    .map_or_else(|| first.clone(), |p| p.to_string_lossy().into_owned())
            } else {
                first.clone()
            };
            return (
                "Read outside the project".into(),
                relative(root, first),
                Some(Target::new(PermissionKind::Folder, [folder])),
            );
        }
    }

    if (kind == "fetch"
        || matches!(
            name,
            "read_url" | "read_url_content" | "execute_url" | "fetch_url"
        ))
        && let Some(url) = arg_str(args, &["Url", "url", "URL"])
    {
        return (
            "Fetch a web page".into(),
            url.into(),
            Some(Target::new(PermissionKind::Web, [url.to_string()])),
        );
    }

    if let Some((server, tool_name)) = name.strip_prefix("mcp__").and_then(|s| s.split_once("__")) {
        return (
            format!("Use {tool_name} from {server}"),
            one_line(&args.to_string()),
            Some(Target::new(
                PermissionKind::Mcp,
                [format!("{server}/{tool_name}")],
            )),
        );
    }

    let title = if name.is_empty() {
        "Allow action".into()
    } else {
        format!("Run {name}")
    };
    let detail = tool_summary(tool, args, root);
    (title, detail, None)
}
