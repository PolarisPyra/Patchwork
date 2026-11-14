//! How a chat stands, read from its messages: whether its agent is at work, waiting on the
//! person, or done, the line it last wrote, and what it is waiting to be told.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use super::{ApprovalStatus, Message, MessageStatus, Role, StepStatus, Thread};

/// Where a chat's agent is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// A reply is streaming, or a task a reply started still runs.
    Working,
    /// It asked for permission or an answer and has not been given one.
    Waiting,
    /// Its last reply ended, or it has not been asked anything.
    Done,
    /// Its last reply ended with an error.
    Failed,
}

/// What an agent is waiting to be told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub thread_id: String,
    pub approval_id: String,
    /// What it wants to do, such as "Run command", or the question it asks.
    pub title: String,
    /// The command or paths it wants to act on. Empty for a question.
    pub detail: String,
    /// How it is answered.
    pub kind: AskKind,
}

/// How an [`Ask`] is answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskKind {
    /// Allowed or denied.
    Permission,
    /// Answered in words, under this question id.
    Question(String),
    /// Answered only in its chat: a secret, or several questions at once.
    InChat,
}

impl Thread {
    /// Where the chat's agent is.
    #[must_use]
    pub fn standing(&self) -> Standing {
        if self.ask().is_some() {
            return Standing::Waiting;
        }
        let streaming = self
            .messages
            .iter()
            .any(|m| m.status == MessageStatus::Streaming);
        let tasks = self
            .messages
            .iter()
            .any(|m| m.background.iter().any(|task| task.status.is_live()));
        if streaming || tasks {
            return Standing::Working;
        }
        match self.last_reply() {
            Some(reply) if reply.status == MessageStatus::Error => Standing::Failed,
            _ => Standing::Done,
        }
    }

    /// What the agent is waiting to be told, the oldest first when it asked more than once.
    #[must_use]
    pub fn ask(&self) -> Option<Ask> {
        let approval = self
            .messages
            .iter()
            .filter(|m| m.status == MessageStatus::Streaming)
            .flat_map(|m| m.approvals.iter())
            .find(|a| a.status == ApprovalStatus::Pending)?;
        let (title, kind) = match approval.questions.as_slice() {
            _ if approval.secret.is_some() => (approval.title.clone(), AskKind::InChat),
            [] => (approval.title.clone(), AskKind::Permission),
            [only] => (only.question.clone(), AskKind::Question(only.id.clone())),
            [first, ..] => (first.question.clone(), AskKind::InChat),
        };
        Some(Ask {
            thread_id: self.id.clone(),
            approval_id: approval.id.clone(),
            title,
            detail: one_line(&approval.detail).unwrap_or_default(),
            kind,
        })
    }

    /// One line on what the agent is doing or did: while it works, the step it is on or the last
    /// line it wrote; once it is done, the first line of its reply, or its error.
    #[must_use]
    pub fn latest_line(&self) -> Option<String> {
        let reply = self.last_reply()?;
        if reply.status != MessageStatus::Streaming {
            return reply
                .error
                .as_deref()
                .filter(|_| reply.status == MessageStatus::Error)
                .and_then(one_line)
                .or_else(|| reply.text.lines().find_map(one_line));
        }
        let step = reply
            .tools
            .iter()
            .rfind(|tool| tool.status == StepStatus::Running && tool.parent.is_none())
            .and_then(|tool| one_line(&format!("{} {}", tool.name, tool.summary)));
        step.or_else(|| reply.text.lines().rev().find_map(one_line))
    }

    /// The agent's last reply, leaving out messages still queued behind it.
    fn last_reply(&self) -> Option<&Message> {
        self.messages
            .iter()
            .map(Arc::as_ref)
            .rfind(|m| !m.queued && m.role == Role::Assistant)
    }
}

/// What every chat's agent is waiting to be told, in the order the chats are given.
#[must_use]
pub fn asks(threads: &[Arc<Thread>]) -> Vec<Ask> {
    threads
        .iter()
        .filter(|t| t.parent.is_none() && t.archived_at.is_none())
        .filter_map(|t| t.ask())
        .collect()
}

/// The longest line shown, in characters.
const MAX_LINE: usize = 160;

/// A line of Markdown as plain words, cut short. None when it holds no words.
fn one_line(text: &str) -> Option<String> {
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    let plain: String = line
        .trim()
        .trim_start_matches(['#', '>', '-', '*', ' '])
        .chars()
        .filter(|c| !matches!(c, '`' | '*'))
        .take(MAX_LINE)
        .collect();
    let plain = plain.trim();
    (!plain.is_empty()).then(|| plain.to_string())
}
