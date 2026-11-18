//! Subagents a turn starts: their own threads, and moving them to the background.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;
use std::time::Instant;

use crate::provider;
use crate::events::AgentEvent;
use crate::events::AgentEventEnvelope;
use crate::provider::AgentKind;
use crate::threads::MessageStatus;
use crate::system::new_id;
use crate::system::now_ms;
use crate::threads::Role;
use crate::threads::Thread;
use crate::threads::ThreadParent;
use crate::threads::ToolCall;
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
        thread_id: &str,
        subagents: &[String],
    ) {
        let (stdin, controls) = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(turn) = turns.get(thread_id) else {
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
                controls
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(request_id.clone(), sent);
                let line = provider::claude::background_task(tool_id, &request_id);
                if write_line(&stdin, &line) {
                    return Some(answer);
                }
                // Nothing will answer a request that was never written.
                controls
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&request_id);
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
pub(in crate::orchestration) struct SubagentThread {
    pub(in crate::orchestration) thread_id: String,
    /// The reply its work streams into.
    pub(in crate::orchestration) message_id: String,
    pub(in crate::orchestration) has_text: bool,
}

/// Opens a read-only thread for the subagent `tool` started, with its instructions as the
/// first message and an empty reply for its work. None when the call starts no subagent.
pub(in crate::orchestration) fn open_child(turn: &Turn, tool: &ToolCall) -> Option<SubagentThread> {
    let sub = tool.subagent.as_ref()?;
    let parent = turn.core.store.thread(&turn.thread_id)?;
    let created_at = now_ms();
    let mut asked = assistant_message();
    asked.role = Role::User;
    asked.text = sub
        .prompt
        .clone()
        .unwrap_or_else(|| sub.description.clone());
    asked.status = MessageStatus::Complete;
    asked.finished_at = Some(created_at);
    let reply = assistant_message();
    let message_id = reply.id.clone();
    let title = [&sub.description, &sub.kind]
        .into_iter()
        .find(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| "Subagent".into());
    let thread = Thread {
        id: new_id(),
        project_id: parent.project_id,
        title,
        agent: parent.agent,
        model: sub.model.clone().or(parent.model),
        access_mode: parent.access_mode,
        effort: None,
        compact_window: None,
        background: parent.background,
        session_id: None,
        created_at,
        messages: vec![Arc::new(asked), Arc::new(reply)],
        goal: None,
        position: 0,
        folder_id: None,
        archived_at: None,
        parent: Some(ThreadParent {
            thread_id: turn.thread_id.clone(),
            tool_id: tool.id.clone(),
        }),
        slash_commands: vec![],
        imported_from: None,
        context_pending: false,
        role: None,
        role_pending: false,
        favorite: false,
        pinned: false,
        bot_id: None,
    };
    if let Err(e) = turn.core.store.insert_threads(vec![thread.clone()]) {
        log::warn!(error:% = e; "failed to save a subagent's thread: {{error}}");
        return None;
    }
    let thread_id = thread.id.clone();
    turn.core.emit(AgentEventEnvelope {
        thread_id: thread_id.clone(),
        event: AgentEvent::ThreadCreated { thread },
    });
    Some(SubagentThread {
        thread_id,
        message_id,
        has_text: false,
    })
}

/// Ends the reply in the thread of the subagent `tool_id` started, once that subagent is done.
pub(in crate::orchestration) fn finish_child(
    turn: &Turn,
    children: &mut HashMap<String, SubagentThread>,
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
        &child.thread_id,
        AgentEvent::TurnCompleted {
            message_id: child.message_id,
            status,
            error: None,
        },
    );
}
