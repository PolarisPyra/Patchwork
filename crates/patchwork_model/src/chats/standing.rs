//! How a chat stands, read from its messages: whether its agent is at work, waiting on the
//! person, or done, the line it last wrote, and what it is waiting to be told.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::{ApprovalStatus, Chat, Message, MessageStatus, Role, StepStatus};

/// Whether a chat is working, failed or waiting for the person, as clients show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatStatus {
    /// Its agent's process is running a turn.
    Running,
    /// Its last message failed.
    Error,
    Idle,
}

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
    pub chat_id: String,
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

impl Message {
    /// Whether the message is still being written.
    #[must_use]
    pub fn is_streaming(&self) -> bool {
        self.status == MessageStatus::Streaming
    }
}

impl Chat {
    /// Where the chat's agent is.
    #[must_use]
    pub fn standing(&self) -> Standing {
        self.standing_while(false)
    }

    /// Where the chat's agent is, given whether its process is starting or running.
    ///
    /// A running process counts as working even before its reply is saved, but a question it
    /// asked still comes first.
    #[must_use]
    pub fn standing_while(&self, running: bool) -> Standing {
        if self.ask().is_some() {
            return Standing::Waiting;
        }
        if running || self.is_busy() {
            return Standing::Working;
        }
        match self.last_reply() {
            Some(reply) if reply.status == MessageStatus::Error => Standing::Failed,
            _ => Standing::Done,
        }
    }

    /// The status clients show: running, error or idle.
    ///
    /// Only the process counts as running; a reply left streaming without one reads idle. The
    /// last message of any kind decides an error.
    #[must_use]
    pub fn client_status(&self, running: bool) -> ChatStatus {
        if running {
            ChatStatus::Running
        } else if self
            .messages
            .last()
            .is_some_and(|m| m.status == MessageStatus::Error)
        {
            ChatStatus::Error
        } else {
            ChatStatus::Idle
        }
    }

    /// Whether a reply is being written.
    #[must_use]
    pub fn is_replying(&self) -> bool {
        self.messages.iter().any(|m| m.is_streaming())
    }

    /// Whether a reply is being written, or a task a reply started still runs.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.is_replying()
            || self
                .messages
                .iter()
                .any(|m| m.background.iter().any(|task| task.status.is_live()))
    }

    /// The newest reply still being written.
    #[must_use]
    pub fn live_reply(&self) -> Option<&Message> {
        self.messages
            .iter()
            .map(Arc::as_ref)
            .rfind(|m| m.role == Role::Assistant && m.is_streaming())
    }

    /// What the agent is waiting to be told, the oldest first when it asked more than once.
    #[must_use]
    pub fn ask(&self) -> Option<Ask> {
        let approval = self
            .messages
            .iter()
            .filter(|m| m.is_streaming())
            .flat_map(|m| m.approvals.iter())
            .find(|a| a.status == ApprovalStatus::Pending)?;
        let (title, kind) = match approval.questions.as_slice() {
            _ if approval.secret.is_some() => (approval.title.clone(), AskKind::InChat),
            [] => (approval.title.clone(), AskKind::Permission),
            [only] => (only.question.clone(), AskKind::Question(only.id.clone())),
            [first, ..] => (first.question.clone(), AskKind::InChat),
        };
        Some(Ask {
            chat_id: self.id.clone(),
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
        if !reply.is_streaming() {
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

    /// What a replying chat is doing now: its running tool step, else its running plan step,
    /// else writing.
    #[must_use]
    pub fn doing(&self) -> String {
        let Some(reply) = self.messages.iter().rfind(|m| m.role == Role::Assistant) else {
            return "starting".into();
        };
        reply
            .tools
            .iter()
            .rfind(|t| t.status == StepStatus::Running)
            .map(|t| t.summary.clone())
            .or_else(|| {
                reply
                    .plan
                    .iter()
                    .find(|s| s.status == StepStatus::Running)
                    .map(|s| s.text.clone())
            })
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "writing".into())
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
pub fn asks(chats: &[Arc<Chat>]) -> Vec<Ask> {
    chats
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
