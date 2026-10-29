//! What an agent CLI's output is read into, and the helpers each CLI's parser builds it with.

use serde_json::Value;
use crate::orchestration::AgentEvent;
use crate::system::now_ms;
use crate::threads::{
    StepStatus, ToolCall, tool_output, SubAgent, Question, TokenUsage, Goal, BackgroundTask,
    TaskStatus,
};
use crate::usage::UsageWindow;
use super::ProviderSlashCommand;

/// One change to a task the agent started, as the agent reports it. Its `task_id` is the agent's
/// own id for the task, which is how later changes name it.
#[derive(Debug)]
pub enum TaskEvent {
    /// A task began. `top_level` is false for one a subagent started, which is listed under that
    /// subagent's thread instead. `backgrounded` is set when it was started to run in the background.
    Started {
        task: BackgroundTask,
        task_id: String,
        top_level: bool,
        backgrounded: bool,
    },
    /// The task's status or description changed, or it was moved to the background once started.
    /// A status that has ended is reported as `Ended` instead.
    Updated {
        task_id: String,
        status: Option<TaskStatus>,
        description: Option<String>,
        backgrounded: bool,
    },
    /// The one-line progress the task reported.
    Progress { task_id: String, summary: String },
    /// The task ended. The agent may report this twice, once as its status and once as its
    /// notification with the output, in either order, so each part is merged in.
    Ended {
        task_id: String,
        status: TaskStatus,
        summary: Option<String>,
        output: Option<String>,
    },
    /// Every task the agent still has running, by its id. This is the agent's level signal, sent
    /// whenever the set changes rather than once per task: a task missing from it has ended, even
    /// if the report of its end never arrived. Claude sends it; the others report only edges.
    Level { live: Vec<String> },
}

/// What one JSONL line means for the turn.
#[derive(Debug)]
// Each event is built once and moved straight into the turn, so boxing the large variant would
// only add an allocation per event.
#[allow(clippy::large_enum_variant)]
pub enum Parsed {
    Event(AgentEvent),
    SessionId(String),
    SlashCommands(Vec<ProviderSlashCommand>),
    /// A task the agent reports on: started, moved to the background, progressing, ended, or the
    /// set of those still running.
    Task(TaskEvent),
    /// Subscription usage the agent reported along the way.
    Usage(Vec<UsageWindow>),
    /// Tokens one model request used. `top_level` is false for a subagent's request.
    RequestUsage {
        request_id: String,
        usage: TokenUsage,
        top_level: bool,
    },
    /// The agent's id for the turn now running, which a steering message must name.
    TurnStarted(String),
    /// The agent is retrying a model request that failed or stalled.
    Retrying {
        reason: String,
        attempt: u32,
        max_attempts: u32,
    },
    /// The turn failed with this message.
    Error(String),
    /// The agent has no conversation under the session id the turn resumed.
    SessionLost,
    /// Write this line to the agent's stdin.
    Send(String),
    /// The API agent calls a tool of an MCP server, which Patchwork keeps running; the result
    /// goes back to its stdin as `api::mcp_reply`.
    McpCall {
        id: Value,
        server: String,
        tool: String,
        arguments: Value,
    },
    /// The agent waits for the user; `reply` is what `approval_reply` needs to answer it.
    Approval {
        title: String,
        detail: String,
        reply: Value,
        /// Set when the agent asks the user questions instead of asking permission.
        questions: Vec<Question>,
        /// What it asks to do, when a permission rule can answer it.
        target: Option<crate::permissions::Target>,
    },
    /// The agent waits for an answer to a call the access mode runs without asking, which only
    /// the permission rules may stop; answered like an approval.
    Check {
        title: String,
        detail: String,
        reply: Value,
        target: crate::permissions::Target,
    },
    /// The agent answered the control request with this id.
    ControlResponse(String),
    /// The agent took a steering message (Codex acknowledges each one).
    Steered,
    /// The agent refused a steering message, so it waits for the turn to end.
    SteerRejected(String),
    Goal(GoalUpdate),
    /// The turn is over; stdin is closed so the agent exits.
    Finished,
}

/// A change to the thread's `/goal`, as the agent reports it.
#[derive(Debug)]
pub enum GoalUpdate {
    /// Claude set the goal, or restated it with the checks made so far.
    Set {
        objective: String,
        checks: Option<u32>,
    },
    /// Claude's evaluator found the goal not met yet, so the agent keeps going.
    Checked,
    /// Claude ended a reply, so its evaluator checks the goal, if one is set.
    Checking,
    Cleared,
    /// Codex reports the whole goal.
    Reported(Goal),
}

/// Collapses whitespace and caps the length, for tool summaries.
pub(crate) fn one_line(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}

/// Strips the project root so paths are project-relative; other paths pass through.
pub(super) fn relative(root: &str, path: &str) -> String {
    let root = root.trim_end_matches('/');
    let path = path
        .strip_prefix(root)
        .and_then(|p| p.strip_prefix('/'))
        .unwrap_or(path);
    path.strip_prefix("./").unwrap_or(path).to_string()
}

/// A tool call the agent has begun, as the provider's events describe it.
pub(super) struct ToolStart<'a> {
    pub(super) id: &'a str,
    pub(super) name: &'a str,
    pub(super) summary: String,
    pub(super) subagent: Option<SubAgent>,
    pub(super) parent: Option<&'a str>,
}

pub(super) fn tool_started(message_id: &str, start: ToolStart<'_>) -> Parsed {
    Parsed::Event(AgentEvent::ToolStarted {
        message_id: message_id.into(),
        tool: ToolCall {
            id: start.id.into(),
            name: start.name.into(),
            summary: start.summary,
            status: StepStatus::Running,
            subagent: start.subagent,
            parent: start.parent.map(String::from),
            started_at: Some(now_ms()),
            ended_at: None,
            output: None,
            child_thread_id: None,
        },
    })
}

pub(super) fn tool_finished(
    message_id: &str,
    tool_id: &str,
    is_error: bool,
    output: &str,
) -> Parsed {
    Parsed::Event(AgentEvent::ToolFinished {
        message_id: message_id.into(),
        tool_id: tool_id.into(),
        is_error,
        output: tool_output(output),
    })
}

pub(super) fn thinking(message_id: &str, text: &str) -> Parsed {
    Parsed::Event(AgentEvent::Thinking {
        message_id: message_id.into(),
        text: text.into(),
        started_at: None,
        ended_at: None,
    })
}

pub(super) fn file_touched(message_id: &str, path: String) -> Parsed {
    Parsed::Event(AgentEvent::FileTouched {
        message_id: message_id.into(),
        path,
    })
}

pub(super) fn text_delta(message_id: &str, text: &str) -> Parsed {
    Parsed::Event(AgentEvent::TextDelta {
        message_id: message_id.into(),
        text: text.into(),
    })
}

/// One-line summary of a tool call's input: the first useful field that is present.
pub(super) fn summarize(input: &Value, root: &str) -> String {
    const KEYS: [&str; 7] = [
        "command",
        // The skill a `Skill` call loads.
        "skill",
        "file_path",
        "pattern",
        "url",
        "description",
        "query",
    ];
    KEYS.iter()
        .find_map(|key| {
            input[*key].as_str().filter(|s| !s.is_empty()).map(|s| {
                if *key == "file_path" {
                    relative(root, s)
                } else {
                    s.to_string()
                }
            })
        })
        .map(|s| one_line(&s))
        .unwrap_or_default()
}
