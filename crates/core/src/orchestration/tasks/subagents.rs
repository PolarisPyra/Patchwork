//! Subagents a turn starts: their own threads, and moving them to the background.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use crate::provider;
use crate::events::AgentEvent;
use crate::events::AgentEventEnvelope;
use crate::provider::AgentKind;
use crate::chats::{Message, MessageStatus};
use crate::system::new_id;
use crate::system::now_ms;
use crate::chats::Role;
use crate::chats::Chat;
use crate::chats::ChatParent;
use crate::chats::ToolCall;
use crate::orchestration::turn::process::write_line;
use crate::orchestration::Runner;
use crate::orchestration::output::stream::publish;
use crate::orchestration::turn::start::assistant_message;
use crate::orchestration::turn::start::Turn;

/// Control requests waiting for the agent's answer, by request id.
pub(in crate::orchestration) type Controls = Arc<Mutex<HashMap<String, mpsc::Sender<()>>>>;

/// How long a steer waits for Claude to move its foreground subagents to the background.
pub(in crate::orchestration) const BACKGROUND_WAIT: Duration = Duration::from_secs(5);

impl Runner {
    /// Claude takes a steer by cancelling the calls it is running, and a subagent working in the
    /// foreground is one of them. Each is moved to the background first, where it keeps working
    /// and reports back when done; this waits for Claude to confirm, up to `BACKGROUND_WAIT`.
    pub(in crate::orchestration) fn background_subagents(
        &self,
        chat_id: &str,
        subagents: &[String],
    ) {
        let (stdin, controls) = {
            let turns = self.turns.locked();
            let Some(turn) = turns.get(chat_id) else {
                return;
            };
            if turn.agent != AgentKind::Claude || turn.cancelled.load(Ordering::SeqCst) {
                return;
            }
            (Arc::clone(&turn.stdin), Arc::clone(&turn.controls))
        };
        let answers: Vec<_> = subagents
            .iter()
            .filter_map(|tool_id| {
                let request_id = new_id();
                let (sent, answer) = mpsc::channel();
                controls.locked().insert(request_id.clone(), sent);
                let line = provider::claude::background_task(tool_id, &request_id);
                if write_line(&stdin, &line) {
                    return Some(answer);
                }
                // Nothing will answer a request that was never written.
                controls.locked().remove(&request_id);
                None
            })
            .collect();
        let deadline = Instant::now() + BACKGROUND_WAIT;
        for answer in answers {
            let left = deadline.saturating_duration_since(Instant::now());
            let _ = answer.recv_timeout(left);
        }
    }
}

/// A subagent's own thread, which its frames stream into while it runs.
pub(in crate::orchestration) struct SubagentChat {
    pub(in crate::orchestration) chat_id: String,
    /// The reply its work streams into.
    pub(in crate::orchestration) message_id: String,
    pub(in crate::orchestration) has_text: bool,
}

/// Opens a read-only chat for the subagent `tool` started, with its instructions as the
/// first message and an empty reply for its work. None when the call starts no subagent.
pub(in crate::orchestration) fn open_child(turn: &Turn, tool: &ToolCall) -> Option<SubagentChat> {
    let sub = tool.subagent.as_ref()?;
    let parent = turn.core.store.chat(&turn.chat_id)?;
    let created_at = now_ms();
    let asked = Message {
        finished_at: Some(created_at),
        ..Message::new(
            Role::User,
            sub.prompt
                .clone()
                .unwrap_or_else(|| sub.description.clone()),
            MessageStatus::Complete,
        )
    };
    let reply = assistant_message();
    let message_id = reply.id.clone();
    let title = [&sub.description, &sub.kind]
        .into_iter()
        .find(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| "Subagent".into());
    let chat = Chat {
        project_id: parent.project_id,
        model: sub.model.clone().or(parent.model),
        background: parent.background,
        created_at,
        messages: vec![Arc::new(asked), Arc::new(reply)],
        parent: Some(ChatParent {
            chat_id: turn.chat_id.clone(),
            tool_id: tool.id.clone(),
        }),
        ..Chat::new(title, parent.agent, parent.access_mode)
    };
    if let Err(e) = turn.core.store.insert_chats(vec![chat.clone()]) {
        log::warn!(error:% = e; "failed to save a subagent's chat: {{error}}");
        return None;
    }
    let chat_id = chat.id.clone();
    turn.core.emit(AgentEventEnvelope {
        chat_id: chat_id.clone(),
        event: AgentEvent::ChatCreated { chat },
    });
    Some(SubagentChat {
        chat_id,
        message_id,
        has_text: false,
    })
}

/// Ends the reply in the chat of the subagent `tool_id` started, once that subagent is done.
pub(in crate::orchestration) fn finish_child(
    turn: &Turn,
    children: &mut HashMap<String, SubagentChat>,
    tool_id: &str,
    failed: bool,
) {
    let Some(child) = children.remove(tool_id) else {
        return;
    };
    let status = if failed {
        MessageStatus::Error
    } else {
        MessageStatus::Complete
    };
    publish(
        &turn.core,
        &child.chat_id,
        AgentEvent::TurnCompleted {
            message_id: child.message_id,
            status,
            error: None,
        },
    );
}
