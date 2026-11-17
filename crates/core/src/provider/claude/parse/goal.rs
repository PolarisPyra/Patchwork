//! Claude Code's `/goal`: reading a goal command, and how the evaluator judged a reply.

// Rust guideline compliant 2026-10-09

use serde_json::Value;
use crate::threads::GoalStatus;
use crate::provider::GoalUpdate;
use crate::provider::without_attachments;
use super::super::home;

/// Reads Claude's `/goal` command output: "Goal set: <objective>",
/// "Goal active: <objective> (3 turns)", or that the goal was cleared.
pub(super) fn goal_from_command(text: &str) -> Option<GoalUpdate> {
    if let Some(objective) = text.strip_prefix("Goal set: ") {
        return Some(GoalUpdate::Set {
            objective: without_attachments(objective).trim().into(),
            checks: Some(0),
        });
    }
    if text.starts_with("Goal cleared") || text.starts_with("No goal set") {
        return Some(GoalUpdate::Cleared);
    }
    let rest = text.strip_prefix("Goal active: ")?;
    let line = rest.lines().next().unwrap_or(rest);
    // A goal that names attachments spans several lines, with the turn count after the list.
    let (objective, count) = match line.rsplit_once(" (") {
        Some(split) => split,
        None => (without_attachments(rest), rest.rsplit_once(" (")?.1),
    };
    let checks = count
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    Some(GoalUpdate::Set {
        objective: objective.into(),
        checks: Some(checks),
    })
}

/// Where Claude's goal stands once its run has ended.
///
/// The stream does not say whether the evaluator found the goal met, judged it impossible, or gave
/// up on a loop that made no progress, so this reads the evaluator's last verdict from the session
/// transcript.
#[must_use]
pub fn goal_outcome(session_id: &str, objective: &str) -> GoalStatus {
    let file = format!("{session_id}.jsonl");
    let transcript = home()
        .and_then(|h| std::fs::read_dir(h.join("projects")).ok())
        .into_iter()
        .flatten()
        .flatten()
        .map(|d| d.path().join(&file))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    // Setting the goal writes a `sentinel` entry; each verdict after it writes another.
    let verdict = transcript
        .lines()
        .rev()
        .filter(|l| l.contains("\"goal_status\""))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|v| v["attachment"].clone())
        .find(|a| {
            a["type"] == "goal_status"
                && a["condition"].as_str().map(without_attachments) == Some(objective)
        });
    match verdict {
        Some(a) if a["met"] == true => GoalStatus::Complete,
        Some(a) if a["failed"] == true => GoalStatus::Blocked,
        // Unmet or not yet judged: the goal is still set in the session and goes on
        // with the next message.
        _ => GoalStatus::Paused,
    }
}
