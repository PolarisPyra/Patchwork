//! Imports conversations that the agent CLIs saved on disk, so work started
//! outside Patchwork shows up as threads. Each imported thread keeps the CLI's
//! session id, so follow-up messages resume the original conversation.

mod antigravity;
mod claude;
mod codex;
mod opencode;
mod t3;

use std::path::Path;

use crate::provider;
use crate::types::{AgentKind, Role, MessageStatus, AccessMode, Message, Thread, ImportSource, new_id};

/// The longest transcript sent to an agent as context. The newest messages are kept.
const MAX_CONTEXT_BYTES: usize = 200_000;

/// A conversation read from an agent CLI's session files.
#[derive(Debug, Clone)]
pub struct Session {
    /// The agent that runs the conversation: the one that resumes it, or for an imported
    /// transcript, the agent the next turn is sent to.
    pub agent: AgentKind,
    pub session_id: String,
    /// Read from another app's history. No agent can resume it, so the transcript is sent
    /// as context instead.
    pub imported: bool,
    /// Working directory the conversation ran in.
    pub cwd: String,
    pub title: Option<String>,
    /// Model the conversation last used, if the source records it.
    pub model: Option<String>,
    pub created_at: u64,
    pub messages: Vec<SessionMessage>,
    /// A helper run inside another conversation (Claude sub-agents, Codex guardian
    /// reviews). Counted, but never offered for import.
    pub subagent: bool,
}

#[derive(Debug, Clone)]
pub struct SessionMessage {
    pub role: Role,
    pub text: String,
    pub created_at: u64,
}

/// Every conversation `source` has saved on disk, in no particular order.
/// Unreadable files and conversations without a user message are skipped.
/// `default_agent` continues the imported transcripts, which no agent can resume.
pub fn sessions(source: ImportSource, default_agent: AgentKind) -> Vec<Session> {
    let all = match source {
        ImportSource::Claude => claude::sessions(),
        ImportSource::Codex => codex::sessions(),
        ImportSource::Antigravity => antigravity::sessions(),
        ImportSource::T3 => t3::sessions(),
        ImportSource::OpenCode => opencode::sessions(default_agent),
    };
    all.into_iter()
        .filter(|s| s.messages.iter().any(|m| m.role == Role::User))
        .collect()
}

impl SessionMessage {
    fn new(role: Role, text: String, created_at: u64) -> Self {
        Self {
            role,
            text,
            created_at,
        }
    }
}

/// Appends assistant text to the previous assistant message, or starts a new one.
fn push_merged(messages: &mut Vec<SessionMessage>, role: Role, text: String, at: u64) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    match messages.last_mut() {
        Some(last) if role == Role::Assistant && last.role == Role::Assistant => {
            last.text.push_str("\n\n");
            last.text.push_str(&text);
        }
        _ => messages.push(SessionMessage::new(role, text, at)),
    }
}

/// A conversation becomes a chat rather than a project when its folder is gone
/// or is one of T3 Code's chat scratch folders (`~/.t3/scratch`).
pub fn is_chat_folder(cwd: &Path) -> bool {
    !cwd.is_dir()
        || provider::home_dir().is_some_and(|home| cwd.starts_with(home.join(".t3/scratch")))
}

/// Converts an imported session into a stored thread.
pub fn to_thread(session: Session, project_id: Option<String>) -> Thread {
    let title = session
        .title
        .filter(|t| !t.trim().is_empty())
        .or_else(|| {
            session
                .messages
                .iter()
                .find(|m| m.role == Role::User)
                .map(|m| crate::title_from(&m.text))
        })
        .unwrap_or_else(|| "Imported conversation".into());
    // An imported transcript has no agent session to resume; its first turn sends the transcript.
    let (session_id, imported_from) = if session.imported {
        (None, Some(session.session_id))
    } else {
        (Some(session.session_id), None)
    };
    Thread {
        id: new_id(),
        project_id,
        title,
        agent: session.agent,
        model: session.model,
        access_mode: AccessMode::default(),
        session_id,
        imported_from,
        context_pending: session.imported,
        created_at: session.created_at,
        position: -(session.created_at as i64),
        folder_id: None,
        effort: None,
        archived_at: None,
        parent: None,
        plan_mode: false,
        slash_commands: vec![],
        messages: session
            .messages
            .into_iter()
            .map(|m| Message {
                id: new_id(),
                role: m.role,
                text: m.text,
                created_at: m.created_at,
                status: MessageStatus::Complete,
                tools: vec![],
                plan: vec![],
                files: vec![],
                error: None,
                approvals: vec![],
                attachments: vec![],
                steer: false,
                queued: false,
                background: Vec::new(),
                cut_off: false,
                refusal: None,
                parts: Vec::new(),
                finished_at: None,
                usage: None,
            })
            .collect(),
        goal: None,
    }
}

/// The imported conversation as text for the agent's first turn: the newest messages that fit
/// in `MAX_CONTEXT_BYTES`, each labelled with who wrote it.
pub fn transcript(messages: &[Message]) -> String {
    let mut turns = Vec::new();
    let mut size = 0;
    for message in messages.iter().rev() {
        let who = if message.role == Role::User { "User" } else { "Assistant" };
        let turn = format!("{who}:\n{}", message.text);
        size += turn.len();
        if size > MAX_CONTEXT_BYTES {
            break;
        }
        turns.push(turn);
    }
    turns.reverse();
    format!(
        "Here is the conversation so far, from an earlier session. Continue it from where it stopped.\n\n{}\n\n---\n\n",
        turns.join("\n\n")
    )
}
