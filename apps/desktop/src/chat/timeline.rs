//! What a turn holds, derived from its message: the web UI's `response/model.ts` and
//! `response/tools.ts`. Plain functions of the message, so both UIs fold and word a turn alike.

use patchwork_server::threads::{Message, MessageStatus, Part, StepStatus, ToolCall};

use crate::ui::full_elapsed;

/// A call or a block of reasoning: the smallest thing a row shows.
pub enum Entry<'a> {
    Thinking {
        text: String,
        ended: bool,
        index: usize,
    },
    Tool(&'a ToolCall),
}

/// One line of a turn, in the order it happened.
pub enum Step<'a> {
    Text(String),
    Work(Vec<Entry<'a>>),
    Agents(Vec<&'a ToolCall>),
    Plan(String),
    Approvals(Vec<&'a str>),
    Compaction,
}

/// Calls that keep a plan the composer already shows, so the turn does not repeat it.
const HIDDEN_TOOLS: [&str; 5] = [
    "TodoWrite",
    "TaskCreate",
    "TaskUpdate",
    "TaskList",
    "TaskGet",
];

/// The reply's parts as steps. A reply saved before parts were kept shows its calls, then its text.
pub fn steps_of(message: &Message) -> Vec<Step<'_>> {
    // By id, as a long turn has hundreds of calls to look up.
    let mut tools = std::collections::HashMap::new();
    for t in message
        .tools
        .iter()
        .filter(|t| t.parent.is_none() && !HIDDEN_TOOLS.contains(&t.name.as_str()))
    {
        tools.entry(t.id.as_str()).or_insert(t);
    }
    let tool = |id: &str| tools.get(id).copied();
    let fallback: Vec<Part>;
    let parts = if message.parts.is_empty() {
        fallback = message
            .tools
            .iter()
            .map(|t| Part::Tool { id: t.id.clone() })
            .chain((!message.text.is_empty()).then(|| Part::Text {
                text: message.text.clone(),
            }))
            .collect();
        &fallback
    } else {
        &message.parts
    };
    let mut steps: Vec<Step> = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let entry = match part {
            Part::Text { text } => {
                if !text.trim().is_empty() {
                    steps.push(Step::Text(text.trim().to_string()));
                }
                continue;
            }
            Part::Plan { text } => {
                steps.push(Step::Plan(text.clone()));
                continue;
            }
            Part::Compaction { .. } => {
                steps.push(Step::Compaction);
                continue;
            }
            Part::Approval { id } => {
                let Some(approval) = message.approvals.iter().find(|a| &a.id == id) else {
                    continue;
                };
                match steps.last_mut() {
                    Some(Step::Approvals(ids)) => ids.push(&approval.id),
                    _ => steps.push(Step::Approvals(vec![&approval.id])),
                }
                continue;
            }
            Part::Thinking { text, ended_at, .. } => {
                if text.trim().is_empty() {
                    continue;
                }
                Entry::Thinking {
                    text: text.clone(),
                    ended: ended_at.is_some(),
                    index,
                }
            }
            Part::Tool { id } => match tool(id) {
                Some(tool) => Entry::Tool(tool),
                None => continue,
            },
        };
        // A subagent is its own card, however many of them ran in a row.
        if let Entry::Tool(tool) = entry
            && tool.subagent.is_some()
        {
            match steps.last_mut() {
                Some(Step::Agents(tools)) => tools.push(tool),
                _ => steps.push(Step::Agents(vec![tool])),
            }
            continue;
        }
        match steps.last_mut() {
            Some(Step::Work(entries)) => entries.push(entry),
            _ => steps.push(Step::Work(vec![entry])),
        }
    }
    steps
}

/// Whether the work this entry stands for is still going.
pub fn entry_active(entry: &Entry) -> bool {
    match entry {
        Entry::Thinking { ended, .. } => !ended,
        Entry::Tool(tool) => is_running(tool),
    }
}

/// A stable identity for a row, so its details stay open as the reply grows.
pub fn work_key(message: &Message, entry: &Entry) -> String {
    match entry {
        Entry::Thinking { index, .. } => format!("{}:t{index}", message.id),
        Entry::Tool(tool) => tool.id.clone(),
    }
}

/// "1 step", "7 steps".
pub fn count_of(n: usize) -> String {
    if n == 1 {
        "1 step".into()
    } else {
        format!("{n} steps")
    }
}

/// How many actions a step holds, which is what a fold's count reads out.
fn size_of(step: &Step) -> usize {
    match step {
        Step::Work(entries) => entries.len(),
        Step::Agents(tools) => tools.len(),
        Step::Approvals(ids) => ids.len(),
        Step::Text(_) | Step::Plan(_) => 1,
        Step::Compaction => 0,
    }
}

/// Whether a settled turn folds, how many steps it hides, and how many actions those hold.
pub struct FoldLayout {
    pub folds: bool,
    pub hidden: usize,
    pub actions: usize,
}

pub fn fold_layout_of(message: &Message, steps: &[Step], streaming: bool) -> FoldLayout {
    // The final text stays out, with any work after it. A failed reply stays open.
    let last_text = steps
        .iter()
        .rposition(|s| matches!(s, Step::Text(_) | Step::Plan(_)));
    let before_text = last_text.unwrap_or(steps.len());
    // A reply a steer cut short folds whole.
    let hidden = if message.cut_off {
        steps.len()
    } else {
        before_text
    };
    let cancelled = message.status == MessageStatus::Cancelled;
    let folds = !streaming
        && message.status != MessageStatus::Error
        && (hidden > 0 || cancelled || message.cut_off);
    FoldLayout {
        folds,
        hidden,
        actions: steps[..hidden].iter().map(size_of).sum(),
    }
}

/// The head line of a settled turn, and what opening it would do.
pub struct FoldSummary {
    pub label: String,
    pub action: Option<String>,
}

/// A turn under a second has no duration worth reading, so it says "Worked".
pub fn fold_head_of(message: &Message, actions: usize, open: bool) -> FoldSummary {
    let action = match (actions, open) {
        (0, _) => None,
        (_, true) => Some("Hide steps".into()),
        (n, false) => Some(count_of(n)),
    };
    if message.cut_off {
        return FoldSummary {
            label: "Superseded attempt".into(),
            action,
        };
    }
    let live: Vec<_> = message
        .background
        .iter()
        .filter(|t| t.status.is_live())
        .collect();
    if !live.is_empty() {
        let agents = live
            .iter()
            .all(|t| t.kind == "agent" || t.kind == "local_agent");
        let noun = if agents {
            "subagent"
        } else {
            "background task"
        };
        let plural = if live.len() == 1 { "" } else { "s" };
        return FoldSummary {
            label: format!("Waiting for {} {noun}{plural}", live.len()),
            action,
        };
    }
    let duration = message
        .finished_at
        .map(|end| end.saturating_sub(message.created_at))
        .filter(|took| *took >= 1000)
        .map(full_elapsed);
    let label = match (message.status == MessageStatus::Cancelled, duration) {
        (true, Some(d)) => format!("You stopped after {d}"),
        (true, None) => "You stopped the response".into(),
        (false, Some(d)) => format!("Worked for {d}"),
        (false, None) => "Worked".into(),
    };
    FoldSummary { label, action }
}

/// Which steps a turn shows. A closed fold keeps the steps after it, subagents still working,
/// questions the agent asked, and compactions.
pub fn shown(message: &Message, step: &Step, i: usize, hidden: usize) -> bool {
    i >= hidden
        || matches!(step, Step::Compaction)
        || matches!(step, Step::Agents(tools) if tools.iter().any(|t| is_running(t)))
        || matches!(step, Step::Approvals(ids) if ids.iter().any(|id| message
            .approvals
            .iter()
            .any(|a| a.id == *id && !a.questions.is_empty())))
}

/// The kinds of work a call can be, which decide both its icon and its wording.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Edit,
    Command,
    Read,
    Search,
    Web,
    Agent,
    Skill,
    Other,
}

impl Action {
    /// The glyph a row shows for it (`ToolIcon.tsx`).
    pub fn icon(self) -> &'static str {
        match self {
            Action::Edit => "note-pencil",
            Action::Command => "terminal",
            Action::Read => "eye",
            Action::Search => "magnifying-glass",
            Action::Web => "globe",
            Action::Agent => "robot",
            Action::Skill => "books",
            Action::Other => "wrench",
        }
    }
}

/// The action a call does, across Claude Code, Codex, OpenCode and Antigravity.
pub fn action_of(tool: &ToolCall) -> Action {
    if tool.subagent.is_some() {
        return Action::Agent;
    }
    let action = match tool.name.as_str() {
        "Bash" | "BashOutput" | "Shell" | "run_command" => Action::Command,
        "Read" | "NotebookRead" | "view_file" => Action::Read,
        "Edit"
        | "Write"
        | "MultiEdit"
        | "NotebookEdit"
        | "write_to_file"
        | "replace_file_content"
        | "multi_replace_file_content" => Action::Edit,
        "Grep" | "Glob" | "LS" | "grep_search" | "find_by_name" | "list_dir" => Action::Search,
        "WebSearch" | "WebFetch" | "search_web" | "read_url_content" => Action::Web,
        "Skill" => Action::Skill,
        _ => Action::Other,
    };
    // Agents without a skill tool load a skill by reading its SKILL.md.
    let skill_file = tool.summary == "SKILL.md" || tool.summary.ends_with("/SKILL.md");
    if action == Action::Read && skill_file {
        Action::Skill
    } else {
        action
    }
}

pub fn is_running(tool: &ToolCall) -> bool {
    matches!(tool.status, StepStatus::Running | StepStatus::Pending)
}

fn created(tool: &ToolCall) -> bool {
    tool.name == "Write"
        && tool
            .output
            .as_deref()
            .is_some_and(|o| o.starts_with("File created successfully"))
}

/// A row's label: what the call does while it runs, and what it did once done.
pub fn tool_label(tool: &ToolCall) -> String {
    let summary = tool.summary.as_str();
    let running = is_running(tool);
    let verb = |doing: &str, done: &str| if running { doing } else { done }.to_string();
    match action_of(tool) {
        Action::Command => {
            if running {
                let word = summary.split_whitespace().next().unwrap_or("");
                let program = word.rsplit('/').next().filter(|w| !w.is_empty());
                format!("Running {}", program.unwrap_or("command"))
            } else if tool.status == StepStatus::Error {
                if summary.is_empty() {
                    "Failed command".into()
                } else {
                    format!("Failed: {summary}")
                }
            } else if summary.is_empty() {
                "Ran command".into()
            } else {
                summary.into()
            }
        }
        Action::Read if summary.is_empty() => "Read file".into(),
        Action::Read => format!("{} {summary}", verb("Reading", "Read")),
        Action::Edit if summary.is_empty() => "Changed file".into(),
        Action::Edit => {
            let done = if created(tool) { "Created" } else { "Changed" };
            format!("{} {summary}", verb("Editing", done))
        }
        Action::Search if summary.is_empty() => "Searched files".into(),
        Action::Search => format!("{} {summary}", verb("Searching", "Searched")),
        Action::Web if tool.name == "WebFetch" => {
            format!("{} {summary}", verb("Fetching", "Fetched"))
        }
        Action::Web if summary.is_empty() => "Searched web".into(),
        Action::Web => format!("{} {summary}", verb("Searching", "Searched")),
        Action::Agent => tool
            .subagent
            .as_ref()
            .map(|s| {
                if s.description.is_empty() {
                    s.kind.clone()
                } else {
                    s.description.clone()
                }
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| tool.name.clone()),
        Action::Skill => {
            let name = if tool.name == "Skill" {
                summary
                    .strip_prefix("claude:")
                    .unwrap_or(summary)
                    .to_string()
            } else {
                let segments: Vec<&str> = summary.split('/').collect();
                segments
                    .len()
                    .checked_sub(2)
                    .and_then(|i| segments.get(i))
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            };
            if name.is_empty() {
                verb("Loading skill", "Used skill")
            } else {
                format!("{} the {name} skill", verb("Loading", "Used"))
            }
        }
        Action::Other => {
            let name = if tool.name.starts_with("mcp__") {
                tool.name.rsplit("__").next().unwrap_or(&tool.name)
            } else if tool.name == "MCP" {
                summary.rsplit('.').next().unwrap_or(summary)
            } else {
                &tool.name
            };
            format!("{} {name}", verb("Using", "Used"))
        }
    }
}

/// A row's label.
pub fn entry_label(entry: &Entry) -> String {
    match entry {
        Entry::Thinking { ended: false, .. } => "Thinking".into(),
        Entry::Thinking { .. } => "Thought".into(),
        Entry::Tool(tool) => tool_label(tool),
    }
}

/// What a row opens to show: a command and its output.
pub fn details_of(tool: &ToolCall) -> String {
    let input = if action_of(tool) == Action::Command {
        format!("$ {}", tool.summary)
    } else {
        tool.summary.clone()
    };
    let joined = [Some(input), tool.output.clone()]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if joined.is_empty() {
        "No output".into()
    } else {
        joined
    }
}

/// The lines an edit added and removed; a `Write` that replaced a file shows none.
pub fn lines_of(tool: &ToolCall) -> Option<(u32, u32)> {
    if tool.name == "Write" && !created(tool) {
        return None;
    }
    tool.lines.as_ref().map(|l| (l.added, l.removed))
}

/// What a command came to: the tests it ran, or else the exit code it failed with.
pub enum CommandResult {
    Tests { passed: u64, failed: u64 },
    Exit(u64),
}

/// The number before " passed" or " failed" in a line, summed.
fn count_before(line: &str, word: &str) -> u64 {
    let mut total = 0;
    let mut rest = line;
    while let Some(at) = rest.find(word) {
        let digits: String = rest[..at]
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let after = rest[at + word.len()..].chars().next();
        if !digits.is_empty() && !after.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            total += digits.parse::<u64>().unwrap_or(0);
        }
        rest = &rest[at + word.len()..];
    }
    total
}

pub fn command_result(tool: &ToolCall) -> Option<CommandResult> {
    let output = tool.output.as_deref().filter(|o| !o.is_empty())?;
    if action_of(tool) != Action::Command || is_running(tool) {
        return None;
    }
    let (mut passed, mut failed) = (0, 0);
    for line in output.lines() {
        if line.contains("Test Suites") || line.contains("Test Files") {
            continue;
        }
        passed += count_before(line, " passed");
        failed += count_before(line, " failed");
    }
    if passed + failed > 0 {
        return Some(CommandResult::Tests { passed, failed });
    }
    if tool.status != StepStatus::Error {
        return None;
    }
    let code: String = output
        .strip_prefix("Exit code ")?
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    code.parse().ok().map(CommandResult::Exit)
}

/// A reply that only says the goal was cleared reads as a notice, not a message.
pub fn is_goal_cleared_reply(text: &str) -> bool {
    text.trim_start().starts_with("Goal cleared")
}
