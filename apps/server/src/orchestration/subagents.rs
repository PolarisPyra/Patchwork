//! Subagents a turn starts: their own threads, moving them to the background, and stopping one.
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::Duration;
use std::time::Instant;

use crate::provider;
use crate::types::AgentEvent;
use crate::types::AgentEventEnvelope;
use crate::types::AgentKind;
use crate::types::MessageStatus;
use crate::types::new_id;
use crate::types::now_ms;
use crate::types::Role;
use crate::types::Thread;
use crate::types::ThreadParent;
use crate::types::ToolCall;
use super::process::write_line;
use super::Runner;
use super::stream::publish;
use super::turn::assistant_message;
use super::turn::Turn;


/// Subagents the agent started: tool call id → the agent's own task id.
pub(super) type Tasks = Arc<Mutex<HashMap<String, String>>>;

/// Control requests waiting for the agent's answer, by request id.
pub(super) type Controls = Arc<Mutex<HashMap<String, mpsc::Sender<()>>>>;

/// How long a steer waits for Claude to move its foreground subagents to the background.
pub(super) const BACKGROUND_WAIT: Duration = Duration::from_secs(5);

impl Runner {
    /// Claude takes a steer by cancelling the calls it is running, and a subagent working in the
    /// foreground is one of them. Each is moved to the background first, where it keeps working
    /// and reports back when done; this waits for Claude to confirm, up to `BACKGROUND_WAIT`.
    pub(super) fn background_subagents(&self, thread_id: &str, subagents: &[String]) {
        let (stdin, controls) = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(turn) = turns.get(thread_id) else {
                return;
            };
            if turn.agent != AgentKind::Claude || turn.cancelled.load(Ordering::SeqCst) {
                return;
            }
            (turn.stdin.clone(), turn.controls.clone())
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
                write_line(
                    &stdin,
                    &provider::claude::background_task(tool_id, &request_id),
                )
                .then_some(answer)
            })
            .collect();
        let deadline = Instant::now() + BACKGROUND_WAIT;
        answers.into_iter().for_each(|answer| {
            let left = deadline.saturating_duration_since(Instant::now());
            let _ = answer.recv_timeout(left);
        });
    }

    /// Stops one subagent of the running turn, leaving the rest of the turn going.
    pub fn stop_subagent(&self, thread_id: &str, tool_id: &str) -> Result<(), String> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let turn = turns
            .get(thread_id)
            .ok_or("This turn is no longer running")?;
        let line = turn
            .tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(tool_id)
            .and_then(|task| provider::stop_subagent(turn.agent, task))
            .ok_or("This subagent can't be stopped")?;
        if write_line(&turn.stdin, &line) {
            Ok(())
        } else {
            Err("This turn no longer accepts input".into())
        }
    }
}

/// A subagent's own thread, which its frames stream into while it runs.
pub(super) struct SubagentThread {
    pub(super) thread_id: String,
    /// The reply its work streams into.
    pub(super) message_id: String,
    pub(super) has_text: bool,
}

/// Opens a read-only thread for the subagent `tool` started, with its instructions as the
/// first message and an empty reply for its work. None when the call starts no subagent.
pub(super) fn open_child(turn: &Turn, tool: &ToolCall) -> Option<SubagentThread> {
    let sub = tool.subagent.as_ref()?;
    let parent = turn.server.store.thread(&turn.thread_id)?;
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
        session_id: None,
        created_at,
        messages: vec![asked, reply],
        goal: None,
        position: 0,
        folder_id: None,
        archived_at: None,
        parent: Some(ThreadParent {
            thread_id: turn.thread_id.clone(),
            tool_id: tool.id.clone(),
        }),
        plan_mode: false,
        slash_commands: vec![],
        imported_from: None,
        context_pending: false,
    };
    if let Err(e) = turn.server.store.insert_threads(vec![thread.clone()]) {
        log::warn!("failed to save a subagent's thread: {e}");
        return None;
    }
    let thread_id = thread.id.clone();
    turn.server.emit(AgentEventEnvelope {
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
pub(super) fn finish_child(
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
        &turn.server,
        &child.thread_id,
        AgentEvent::TurnCompleted {
            message_id: child.message_id,
            status,
            error: None,
        },
    );
}
