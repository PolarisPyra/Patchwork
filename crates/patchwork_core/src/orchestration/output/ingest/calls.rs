//! The reply's own events: its text, and the calls it starts and finishes.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;

use super::read::Reading;
use crate::chats::ToolCall;
use crate::events::ChatEvent;
use crate::orchestration::output::stream::Unsaved;
use crate::orchestration::tasks::delegate::mark_call;
use crate::orchestration::tasks::subagents::{
    SubagentChat, finish_child, follow_agy_children, open_child,
};
use crate::orchestration::turn::process::terminate;
use crate::orchestration::turn::start::Turn;
use crate::orchestration::turn::stuck::LOOP_LIMIT;
use crate::provider::{ProviderKind, antigravity::SWARM_TASK_PREFIX};

impl Reading {
    /// Shows one event of the reply.
    pub(super) fn event(&mut self, mut event: ChatEvent) {
        self.steer_reply_empty = false;
        self.unprompted.track(&event);
        if let ChatEvent::Thinking { .. } = &event {
            self.turn.stream_thinking(event, &mut self.unsaved);
            return;
        }
        match &mut event {
            // Separate successive text blocks so they do not run together. An agent that
            // streams a block as chunks keeps the chunks after the first together.
            ChatEvent::TextDelta { text, .. } => {
                let continues = self.caps.streams_text && self.last_was_text;
                if self.has_text && !continues {
                    text.insert_str(0, "\n\n");
                }
                self.has_text = true;
            }
            ChatEvent::ToolStarted { tool, message_id } => {
                if !self.tool_started(tool, message_id) {
                    return;
                }
            }
            ChatEvent::ToolFinished {
                tool_id,
                message_id,
                is_error,
                output,
            } => {
                if !self.tool_finished(tool_id, message_id, *is_error, output.as_deref()) {
                    return;
                }
            }
            ChatEvent::FileTouched { .. } => self.repeats.changed_file(),
            _ => {}
        }
        self.last_was_text = matches!(event, ChatEvent::TextDelta { .. });
        if self.last_was_text {
            self.turn.stream_text(event, &mut self.unsaved);
            return;
        }
        let finished = match &event {
            ChatEvent::ToolFinished {
                tool_id, is_error, ..
            } if self.caps.lists_open_calls => Some((tool_id.clone(), *is_error)),
            _ => None,
        };
        self.publish_event(event);
        if let Some((tool_id, failed)) = finished {
            for event in self.background.finish_unreported(&tool_id, failed) {
                self.publish_event(event);
            }
        }
    }

    /// Notes a call the agent started; false when it is shown already.
    ///
    /// Some agents report a call when it starts and again when it completes; only the first
    /// start for an id is kept. An agent that restates its calls can name or describe one better
    /// than its start, so one that does is shown again in place; a subagent's call keeps its chat.
    fn tool_started(&mut self, tool: &mut ToolCall, message_id: &mut String) -> bool {
        self.repeats.started(tool);
        // A swarm worker's call reads finished while its worker is parked and running again
        // once it is woken, so every start of it is shown, as a worker that came back to work
        // is working again.
        let worker = tool.id.starts_with(SWARM_TASK_PREFIX);
        let shown = (self.caps.restates_calls && tool.subagent.is_none())
            .then(|| (tool.name.clone(), tool.summary.clone()));
        if let Some(owner) = self.tool_owner.get(&tool.id) {
            let shown = shown.filter(|shown| self.tool_shown.get(&tool.id) != Some(shown));
            if !worker && shown.is_none() {
                return false;
            }
            message_id.clone_from(owner);
            if let Some(shown) = shown {
                self.tool_shown.insert(tool.id.clone(), shown);
            }
            if worker {
                self.open_calls.insert(tool.id.clone());
            }
            return true;
        }
        if let Some(shown) = shown {
            self.tool_shown.insert(tool.id.clone(), shown);
        }
        self.tool_owner.insert(tool.id.clone(), message_id.clone());
        if tool.parent.is_none() {
            self.open_calls.insert(tool.id.clone());
        }
        if let Some(task) = self.turn.agent.provider().background_task(tool) {
            self.background.start_unreported(task);
        }
        if self.caps.subagent_chats
            && tool.parent.is_none()
            && let Some(child) = open_child(&self.turn, tool)
        {
            tool.child_chat_id = Some(child.chat_id.clone());
            // A swarm worker's chat follows its transcript from the start.
            if let Some(transcript) = self.reader.transcript(&tool.id) {
                follow_agy_children(&self.turn, child, vec![transcript]);
            } else {
                self.children.insert(tool.id.clone(), child);
            }
        }
        mark_call(&self.turn, tool);
        true
    }

    /// Notes a call the agent finished; false when its finish waits for its task to end.
    fn tool_finished(
        &mut self,
        tool_id: &str,
        message_id: &mut String,
        is_error: bool,
        output: Option<&str>,
    ) -> bool {
        self.call_ended |= self.open_calls.remove(tool_id);
        // A swarm worker parks and wakes again many times, which is the swarm working, not a
        // call repeated without getting anywhere.
        if !tool_id.starts_with(SWARM_TASK_PREFIX)
            && let Some(name) = self.repeats.finished(tool_id, is_error, output)
            && self.stream_error.is_none()
        {
            self.stream_error = Some(format!(
                "Stopped: the agent called {name} {LOOP_LIMIT} times in a row without getting anywhere."
            ));
            terminate(self.pid, Arc::clone(&self.turn.exited));
        }
        if let Some(owner) = self.tool_owner.get(tool_id) {
            message_id.clone_from(owner);
        }
        // A call whose task still runs is shown finished when the task ends.
        !self.background.call_returned(tool_id, message_id, is_error)
    }

    /// Publishes one event of the turn, as [`publish_event`] does.
    pub(super) fn publish_event(&mut self, event: ChatEvent) {
        publish_event(&self.turn, &mut self.children, &mut self.unsaved, event);
    }
}

/// Publishes one event of the turn. A subagent whose call finishes also closes its chat.
pub(super) fn publish_event(
    turn: &Turn,
    children: &mut HashMap<String, SubagentChat>,
    unsaved: &mut Unsaved,
    event: ChatEvent,
) {
    turn.save_text(unsaved);
    let finished = match &event {
        ChatEvent::ToolFinished {
            tool_id, is_error, ..
        } => Some((tool_id.clone(), *is_error)),
        _ => None,
    };
    // Some agents' subagents work on after the call that starts them returns, so their chat
    // follows their transcripts instead of ending with the call.
    let transcripts = match &event {
        ChatEvent::ToolFinished {
            tool_id,
            is_error: false,
            output: Some(output),
            ..
        } if children.contains_key(tool_id) && crate::system::ssh::split(&turn.root).is_none() => {
            turn.agent.provider().child_transcripts(output)
        }
        _ => Vec::new(),
    };
    turn.publish(event);
    if let Some((tool_id, failed)) = finished {
        if !transcripts.is_empty()
            && let Some(child) = children.remove(&tool_id)
        {
            follow_agy_children(turn, child, transcripts);
        } else {
            finish_child(turn, children, &tool_id, failed);
        }
    }
}
