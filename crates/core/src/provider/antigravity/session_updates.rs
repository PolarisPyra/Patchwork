//! `session/update` notifications: text, thoughts, plans, tool calls, and session state.

// Rust guideline compliant 2026-11-23

use serde_json::Value;

use crate::chats::{PlanStep, TokenUsage};
use crate::events::AgentEvent;
use crate::provider::{
    AgentKind, Parsed, ProviderSlashCommand, acp, ToolStart, TurnSpec, file_touched, relative,
    step_status, text_delta, thinking, tool_finished, tool_started,
};

use super::edits::edit_lines;
use super::errors::{limit_notice, limit_window};
use super::questions::asked;
use super::swarm::{format_swarm_tool, invoke_subagent_descriptor};
use super::tool_fields::{tool_args, tool_output, tool_paths, tool_summary};

/// How the request id of a context report (`usage_update`) starts.
pub const CONTEXT_REQUEST: &str = "acp-context-";

pub(super) fn parse_session_update(
    update: &Value,
    spec: &TurnSpec,
    message_id: &str,
    out: &mut Vec<Parsed>,
) {
    match update["sessionUpdate"].as_str() {
        Some("available_commands_update") => {
            let commands: Vec<ProviderSlashCommand> = update["availableCommands"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|cmd| {
                    let name = cmd["name"].as_str()?.trim();
                    if name.is_empty() || acp::hides_command(spec.agent, name) {
                        return None;
                    }
                    Some(ProviderSlashCommand {
                        name: name.into(),
                        description: cmd["description"]
                            .as_str()
                            .map(str::trim)
                            .filter(|d| !d.is_empty())
                            .map(String::from),
                    })
                })
                .collect();
            if !commands.is_empty() {
                out.push(Parsed::SlashCommands(commands));
            }
        }
        Some("agent_message_chunk") => {
            if update["content"]["type"].as_str() == Some("text")
                && let Some(text) = update["content"]["text"].as_str().filter(|t| !t.is_empty())
            {
                match limit_notice(text).filter(|_| spec.agent == AgentKind::Antigravity) {
                    Some(notice) => {
                        out.push(Parsed::Error(notice.into()));
                        out.extend(limit_window(notice));
                    }
                    None => out.push(text_delta(message_id, text)),
                }
            }
        }
        Some("agent_thought_chunk") => {
            if update["content"]["type"].as_str() == Some("text")
                && let Some(text) = update["content"]["text"].as_str().filter(|t| !t.is_empty())
            {
                out.push(thinking(message_id, text));
            }
        }
        Some("plan") => {
            let steps: Vec<PlanStep> = update["entries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    let text = entry["content"].as_str()?.trim();
                    if text.is_empty() {
                        return None;
                    }
                    Some(PlanStep {
                        text: text.into(),
                        status: step_status(entry["status"].as_str()),
                    })
                })
                .collect();
            if !steps.is_empty() {
                out.push(Parsed::Event(AgentEvent::PlanUpdated {
                    message_id: message_id.into(),
                    steps,
                }));
            }
        }
        Some("tool_call" | "tool_call_update") => {
            let Some(id) = update["toolCallId"].as_str().filter(|s| !s.is_empty()) else {
                return;
            };
            let name = update["name"]
                .as_str()
                .or_else(|| update["_meta"]["toolName"].as_str())
                .or_else(|| update["kind"].as_str())
                .unwrap_or("tool");
            let args = tool_args(update);
            let start = if let Some((swarm_name, swarm_summary)) = format_swarm_tool(name, args) {
                ToolStart::new(id, swarm_name, swarm_summary)
            } else if is_subagent_call(update, name)
                && let Some(subagent) = invoke_subagent_descriptor(args)
            {
                let summary = subagent.description.clone();
                ToolStart {
                    id,
                    name: "Agent",
                    summary,
                    subagent: Some(subagent),
                    parent: None,
                }
            } else {
                ToolStart::new(id, name, tool_summary(update, args, spec.root))
            };
            let mut started = tool_started(message_id, start);
            if let Parsed::Event(AgentEvent::ToolStarted { tool, .. }) = &mut started {
                tool.lines = edit_lines(update);
            }
            out.push(started);
            out.extend(asked(update, name));

            let status = update["status"].as_str();
            if matches!(status, Some("completed" | "failed")) {
                let failed = status == Some("failed")
                    || update["rawOutput"]["exit_code"]
                        .as_i64()
                        .is_some_and(|c| c != 0)
                    || update["rawOutput"]["exitCode"]
                        .as_i64()
                        .is_some_and(|c| c != 0);
                if !failed && update["kind"].as_str() == Some("edit") {
                    for path in tool_paths(update, args) {
                        out.push(file_touched(message_id, relative(spec.root, &path)));
                    }
                }
                let output = tool_output(update);
                out.push(tool_finished(message_id, id, failed, &output));
            }
        }
        // agy 1.5 compacts without saying so; a later one may report it.
        Some("compaction_update") => match update["status"].as_str() {
            Some("in_progress") => out.push(Parsed::Compacting),
            Some("completed") => {
                let summary: Vec<&str> = update["summary"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|block| block["text"].as_str())
                    .filter(|text| !text.trim().is_empty())
                    .collect();
                out.push(Parsed::Compacted(
                    (!summary.is_empty()).then(|| summary.join("\n")),
                ));
            }
            // A compaction that failed ends with whatever agy says next.
            _ => {}
        },
        Some("current_mode_update") => {
            if let Some(mode) = update["currentModeId"].as_str().filter(|m| !m.is_empty()) {
                out.push(Parsed::AgentMode(mode.into()));
            }
        }
        Some("config_option_update") => {
            if update["configOptions"].is_array() {
                out.push(Parsed::AgentConfig(update["configOptions"].clone()));
            }
        }
        Some("usage_update") => {
            if let Some(used) = update["used"].as_u64().filter(|&n| n > 0) {
                out.push(Parsed::RequestUsage {
                    request_id: format!("{CONTEXT_REQUEST}{message_id}"),
                    usage: TokenUsage {
                        context: used,
                        context_window: update["size"].as_u64().filter(|&n| n > 0),
                        ..Default::default()
                    },
                    top_level: true,
                    model: None,
                });
            }
        }
        _ => {}
    }
}

/// Whether the call starts one of agy's sub agents. agy may name it only in its title, which an
/// MCP tool of the same name shares but marks as MCP.
fn is_subagent_call(update: &Value, name: &str) -> bool {
    const NAMES: [&str; 2] = ["invoke_subagent", "start_subagent"];
    let titled = update["title"]
        .as_str()
        .and_then(|t| {
            t.strip_prefix("Running ")
                .or_else(|| t.strip_prefix("Run "))
        })
        .map(|t| t.trim_end_matches('?'))
        .is_some_and(|t| NAMES.contains(&t));
    (NAMES.contains(&name) || titled) && update["_meta"]["is_mcp_tool_call"] != true
}
