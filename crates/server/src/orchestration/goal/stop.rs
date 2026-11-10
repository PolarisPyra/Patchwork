//! Stopping a goal: the message that has the agent drop it, and clearing it from the thread.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::sync::Arc;

use super::state::{held_key, signature};
use crate::Server;
use crate::orchestration::{AgentEvent, AgentEventEnvelope};
use crate::provider::AgentKind;

/// Whether `text` is upkeep the app also sends on its own rather than work for the agent: an idle
/// `/compact`, or the message that has the agent drop its goal.
#[must_use]
pub fn is_upkeep(text: &str) -> bool {
    let text = text.trim();
    text == "/compact"
        || [AgentKind::Claude, AgentKind::Antigravity]
            .into_iter()
            .any(|agent| text == clear_command(agent))
}

/// The message that has the agent drop its goal. Antigravity has no command to clear a goal;
/// its docs say to ask the agent.
pub(crate) fn clear_command(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Antigravity | AgentKind::OpenCode | AgentKind::Api => "Stop the current goal.",
        AgentKind::Claude | AgentKind::Codex => "/goal clear",
    }
}

impl Server {
    /// Clears the goal, in the agent's session too, so the agent does not pick it up again.
    /// A goal's final status remains in the thread until the user clears it. The agent's own
    /// instructions are still cleared when pursuit ends (see `settle`).
    ///
    /// # Errors
    ///
    /// Returns an error when a turn is running, or when the goal's dismissal or held state cannot
    /// be saved or the stop message cannot be sent.
    pub fn stop_goal(self: &Arc<Self>, thread_id: &str) -> Result<(), Error> {
        let Some(thread) = self.store.thread(thread_id) else {
            return Ok(());
        };
        // A `/goal` command runs as a turn of its own, so a running turn must be stopped first.
        if self.runner.is_running(thread_id) {
            return Err("Stop the running turn before clearing the goal".into());
        }
        let Some(goal) = thread.goal.as_ref() else {
            return Ok(());
        };
        self.store.dismiss_goal(thread_id, &signature(goal))?;
        self.store.set_state(&held_key(thread_id), "")?;
        // Claude and Codex report the clear back, so the banner stays until their reply, as it
        // does while the goal runs. Other agents report nothing, so the banner clears now.
        let reports_clear = matches!(thread.agent, AgentKind::Claude | AgentKind::Codex);
        if thread.session_id.is_none() || !reports_clear {
            self.emit(AgentEventEnvelope {
                thread_id: thread_id.into(),
                event: AgentEvent::GoalUpdated { goal: None },
            });
        }
        // Explicitly closing the banner must clear the CLI's session state too, including a
        // goal marked complete: some CLIs can report that cached goal again on resume.
        if thread.session_id.is_some() {
            let stop = clear_command(thread.agent);
            self.send_message(thread_id.into(), stop.into(), vec![], false)?;
        }
        Ok(())
    }
}
