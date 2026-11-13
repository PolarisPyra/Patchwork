//! A second opinion on each reply that changed files: the agent a granular chat picks as its
//! reviewer reads the turn's diff in a chat of its own, and its verdict shows on the reply.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::Duration;

use super::wait::Unanswered;
use crate::Server;
use crate::events::AgentEvent;
use crate::orchestration::changes::checkpoint::turn_file_diff;
use crate::orchestration::publish;
use crate::system::{new_id, now_ms};
use crate::threads::{
    AgentPick, MessageStatus, ReviewStatus, Role, Thread, ThreadParent, TurnReview, reviewer_of,
};

/// The longest a review runs before it is stopped.
const MAX_REVIEW: Duration = Duration::from_secs(20 * 60);

/// The most of the diff put in the prompt. The reviewer is told how to read the rest.
const MAX_DIFF: usize = 60_000;

/// The line a reviewer starts its reply with when it finds nothing to change.
const APPROVED: &str = "APPROVED";

/// The line a reviewer starts its reply with when it has something to say.
const CONCERNS: &str = "CONCERNS";

impl Server {
    /// Has the chat's reviewer look at what the reply `message_id` changed, on a thread of its
    /// own. Does nothing for a chat without a reviewer. Called once the turn's reply is saved.
    pub(crate) fn review_after_turn(self: &Arc<Self>, thread_id: &str, message_id: &str) {
        let Some(thread) = self.store.thread(thread_id) else {
            return;
        };
        let Some(pick) = reviewer_of(&self.store, &self.store.settings(), &thread) else {
            return;
        };
        let (server, message) = (Arc::clone(self), message_id.to_string());
        std::thread::spawn(move || review(&server, &thread, &pick, &message));
    }
}

/// Runs the review of the reply `message_id` in `parent` on `pick`, publishing it as it starts
/// and again when it ends.
fn review(server: &Arc<Server>, parent: &Thread, pick: &AgentPick, message_id: &str) {
    let Some(prompt) = prompt(server, parent, message_id) else {
        return;
    };
    let child = reviewer_thread(parent, pick);
    let child_id = child.id.clone();
    let at = now_ms();
    let emit = |status: ReviewStatus, text: String| {
        publish(
            server,
            &parent.id,
            AgentEvent::ReviewRecorded {
                message_id: message_id.to_string(),
                review: TurnReview {
                    status,
                    agent: pick.agent,
                    model: pick.model.clone(),
                    thread_id: child_id.clone(),
                    text,
                    at,
                },
            },
        );
    };
    if let Err(e) = server.store.insert_threads(vec![child.clone()]) {
        log::warn!(error:% = e; "could not save the reviewer's chat: {{error}}");
        return;
    }
    server.emit(crate::events::AgentEventEnvelope {
        thread_id: child_id.clone(),
        event: AgentEvent::ThreadCreated { thread: child },
    });
    emit(ReviewStatus::Running, String::new());
    let sent = match server.send_message(child_id.clone(), prompt, Vec::new(), false) {
        Ok(sent) => sent,
        Err(e) => {
            emit(ReviewStatus::Failed, e.to_string());
            return;
        }
    };
    let reply =
        server
            .runner
            .wait_for_reply(server, &child_id, Some(&sent.id), &|| false, MAX_REVIEW);
    server.runner.retire_idle(&child_id);
    match reply {
        Ok(reply) if reply.status == MessageStatus::Complete => {
            let (status, text) = verdict(&reply.text);
            emit(status, text);
        }
        Ok(reply) => emit(
            ReviewStatus::Failed,
            reply
                .error
                .unwrap_or_else(|| "The review was stopped before it finished.".into()),
        ),
        Err(why) => {
            if why == Unanswered::TimedOut {
                server.stop_turn(&child_id, false);
            }
            let text = match why {
                Unanswered::TimedOut => "The review ran for 20 minutes and was stopped.",
                Unanswered::NotStarted | Unanswered::Stopped => "The reviewer did not start.",
            };
            emit(ReviewStatus::Failed, text.into());
        }
    }
}

/// The reviewer's own chat, under the one whose reply it reviews.
fn reviewer_thread(parent: &Thread, pick: &AgentPick) -> Thread {
    Thread {
        id: new_id(),
        project_id: parent.project_id.clone(),
        title: "Review".into(),
        agent: pick.agent,
        model: pick.model.clone(),
        access_mode: parent.access_mode,
        effort: pick.effort.clone(),
        compact_window: None,
        background: parent.background.clone(),
        session_id: None,
        created_at: now_ms(),
        messages: Vec::new(),
        goal: None,
        position: 0,
        folder_id: None,
        archived_at: None,
        parent: Some(ThreadParent {
            thread_id: parent.id.clone(),
            tool_id: String::new(),
        }),
        plan_mode: false,
        slash_commands: vec![],
        imported_from: None,
        context_pending: false,
        role: None,
        role_pending: false,
        favorite: false,
        pinned: false,
    }
}

/// What the reviewer is asked: the request the reply answered and the diff of what it changed.
/// None when the reply has no diff to review.
fn prompt(server: &Server, parent: &Thread, message_id: &str) -> Option<String> {
    let at = parent.messages.iter().position(|m| m.id == message_id)?;
    let reply = &parent.messages[at];
    let turn = reply.turn.as_ref()?;
    let root = server.project_root(parent.project_id.as_deref()?).ok()?;
    let request = parent.messages[..at]
        .iter()
        .rfind(|m| m.role == Role::User && !m.queued)
        .map_or("", |m| m.text.trim());
    let mut diff = String::new();
    let mut left = Vec::new();
    for path in &reply.files {
        match turn_file_diff(&root, turn, path) {
            Ok(file) if diff.len() + file.len() <= MAX_DIFF => diff.push_str(&file),
            Ok(_) => left.push(path.as_str()),
            Err(e) => log::warn!(error:% = e; "could not diff a file for review: {{error}}"),
        }
    }
    if diff.trim().is_empty() && left.is_empty() {
        return None;
    }
    let rest = if left.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nThe diff of these files was too long to include; read it with \
             `git diff {} {} -- <path>`:\n{}",
            turn.before,
            turn.after,
            left.join("\n")
        )
    };
    Some(format!(
        "You are reviewing another agent's work in this repository as a second opinion. Do not \
         change any files: read, run read-only commands, and report.\n\n\
         Start your reply with a line that is exactly {APPROVED} if the change does what was \
         asked and you found no real problems, or {CONCERNS} if you found bugs, missed \
         requirements or risks. Then list each problem with its file and line, most serious \
         first. Skip style nits.\n\n\
         The request:\n{request}\n\n\
         The diff of the files the agent changed:\n```diff\n{diff}```{rest}"
    ))
}

/// The reviewer's verdict, from the line its reply starts with, and the rest of its reply.
fn verdict(text: &str) -> (ReviewStatus, String) {
    let text = text.trim();
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let word = first
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_uppercase();
    if word.starts_with(APPROVED) {
        (ReviewStatus::Approved, rest.trim().to_string())
    } else if word.starts_with(CONCERNS) {
        (ReviewStatus::Concerns, rest.trim().to_string())
    } else {
        // A reviewer that skipped the verdict line said something, which is worth reading.
        (ReviewStatus::Concerns, text.to_string())
    }
}
