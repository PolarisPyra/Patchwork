//! OpenCode 1.x: a turn is one `opencode run --format json`, whose JSON lines are read into agent events.

use serde_json::Value;
use crate::orchestration::AgentEvent;
use crate::threads::{AccessMode, AttachmentKind, PlanStep, TokenUsage};
use crate::provider::Parsed;
use crate::provider::ToolStart;
use crate::provider::TurnSpec;
use crate::provider::file_touched;
use crate::provider::one_line;
use crate::provider::prompt;
use crate::provider::relative;
use crate::provider::summarize;
use crate::provider::text_delta;
use crate::provider::thinking;
use crate::provider::tool_finished;
use crate::provider::tool_started;
use super::{parse_serve, uses_serve};

pub fn turn_args(spec: &TurnSpec) -> Vec<String> {
    // The bridge takes a 2.x turn from its stdin.
    if uses_serve() {
        return vec![crate::provider::opencode::bridge::BRIDGE_ARG.into()];
    }
    let mut args: Vec<String> = ["run", "--format", "json", "--thinking"]
        .map(String::from)
        .into();
    if spec.mode == AccessMode::FullAccess {
        args.push("--auto".into());
    }
    if spec.plan {
        args.extend(["--agent".into(), "plan".into()]);
    }
    if let Some(id) = spec.session_id {
        args.extend(["--session".into(), id.into()]);
    }
    if let Some(model) = spec.model {
        args.extend(["--model".into(), model.into()]);
    }
    if let Some(effort) = spec.effort {
        args.extend(["--variant".into(), effort.into()]);
    }
    // Images go with the message as files; every attachment is also named in the prompt.
    for image in spec
        .attachments
        .iter()
        .filter(|a| a.kind == AttachmentKind::Image)
    {
        args.extend(["--file".into(), image.path.clone()]);
    }
    // After `--`, a prompt that starts with "-" is not read as a flag.
    args.extend(["--".into(), prompt(spec)]);
    args
}

/// Maps one line of `opencode run --format json`: a `step_start`, then the step's `reasoning`,
/// `text` and finished `tool_use` parts, then a `step_finish` with its token counts.
pub fn parse(v: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
    if uses_serve() {
        return parse_serve(v, spec, message_id);
    }
    let part = &v["part"];
    match v["type"].as_str() {
        Some("step_start") => v["sessionID"]
            .as_str()
            .filter(|id| spec.session_id != Some(*id))
            .map(|id| Parsed::SessionId(id.into()))
            .into_iter()
            .collect(),
        Some("text") => text_of(part)
            .map(|t| text_delta(message_id, t))
            .into_iter()
            .collect(),
        Some("reasoning") => text_of(part)
            .map(|t| thinking(message_id, t))
            .into_iter()
            .collect(),
        Some("tool_use") => tool(part, spec.root, message_id),
        Some("step_finish") => step_usage(part).into_iter().collect(),
        Some("error") => {
            let error = &v["error"];
            // A stopped turn reports its own abort; that is not a failure.
            if error["name"] == "MessageAbortedError" {
                return Vec::new();
            }
            // 1.x puts the message under `data`, 2.x on the error itself.
            let message = error["data"]["message"]
                .as_str()
                .or_else(|| error["message"].as_str())
                .or_else(|| error["name"].as_str())
                .unwrap_or("OpenCode reported an error");
            vec![Parsed::Error(message.into())]
        }
        _ => Vec::new(),
    }
}

fn text_of(part: &Value) -> Option<&str> {
    part["text"].as_str().filter(|t| !t.trim().is_empty())
}

/// A tool part. OpenCode prints a tool once it has finished, so it starts and ends here.
pub(super) fn tool(part: &Value, root: &str, message_id: &str) -> Vec<Parsed> {
    let state = &part["state"];
    // 1.x names the call with `callID`; 2.x has only the part's own `id`.
    let id = part["callID"].as_str().or_else(|| part["id"].as_str());
    let (Some(id), Some(name)) = (id, part["tool"].as_str()) else {
        return Vec::new();
    };
    let input = &state["input"];
    let path = input["filePath"]
        .as_str()
        .or_else(|| input["path"].as_str());
    let summary = match path {
        Some(path) => relative(root, path),
        None => summarize(input, root),
    };
    let mut started = tool_started(
        message_id,
        ToolStart {
            id,
            name: tool_name(name),
            summary: one_line(&summary),
            subagent: None,
            parent: None,
        },
    );
    // Reported only once done, the call keeps when it really started.
    if let (Parsed::Event(AgentEvent::ToolStarted { tool, .. }), Some(start)) =
        (&mut started, state["time"]["start"].as_u64())
    {
        tool.started_at = Some(start);
    }
    let mut out = vec![started];
    let failed = state["status"] == "error";
    let output = if failed {
        state["error"].as_str()
    } else {
        state["output"].as_str()
    };
    out.push(tool_finished(
        message_id,
        id,
        failed,
        output.unwrap_or_default(),
    ));
    if !failed && matches!(name, "write" | "edit" | "patch") {
        out.extend(path.map(|p| file_touched(message_id, relative(root, p))));
    }
    if !failed {
        out.extend(todo_plan(name, input, message_id));
    }
    out
}

/// The task list a `todowrite` call keeps, as the message's plan. Its todos have the same
/// `content` and `status` as Claude's `TodoWrite`.
pub(super) fn todo_plan(name: &str, input: &Value, message_id: &str) -> Option<Parsed> {
    if name != "todowrite" {
        return None;
    }
    let steps = input["todos"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|todo| PlanStep {
            text: todo["content"].as_str().unwrap_or_default().into(),
            status: crate::provider::step_status(todo["status"].as_str()),
        })
        .collect();
    Some(Parsed::Event(AgentEvent::PlanUpdated {
        message_id: message_id.into(),
        steps,
    }))
}

/// OpenCode's tool names, shown the way the other agents name theirs.
pub(super) fn tool_name(name: &str) -> &str {
    match name {
        "read" => "Read",
        "write" => "Write",
        "edit" | "patch" => "Edit",
        "bash" | "shell" => "Bash",
        "glob" => "Glob",
        "grep" => "Grep",
        "list" => "LS",
        "webfetch" => "WebFetch",
        "websearch" => "WebSearch",
        "todowrite" => "TodoWrite",
        "skill" => "Skill",
        "task" | "subagent" => "Task",
        "execute" => "Execute",
        other => other,
    }
}

/// The token counts of one model request, from its `step_finish` part.
fn step_usage(part: &Value) -> Option<Parsed> {
    let tokens = part.get("tokens")?;
    let count = |v: &Value| v.as_u64().unwrap_or(0);
    let input = count(&tokens["input"]);
    let cache_read = count(&tokens["cache"]["read"]);
    let cache_write = count(&tokens["cache"]["write"]);
    Some(Parsed::RequestUsage {
        request_id: part["id"].as_str()?.into(),
        usage: TokenUsage {
            input,
            cache_read,
            cache_write,
            output: count(&tokens["output"]) + count(&tokens["reasoning"]),
            context: input + cache_read + cache_write,
        },
        top_level: true,
    })
}
