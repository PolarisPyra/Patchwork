//! What a thread runs with: its model, access, plan mode, effort and compact window.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::provider::AgentKind;
use crate::threads::{AccessMode, Thread};
use crate::Server;

use super::lifecycle::note;

impl Server {
    /// Sets the thread's model. The agent can only change before the first message,
    /// since the conversation session belongs to one CLI. An imported chat has no session yet,
    /// so it can take any agent until its first turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown, its agent would change after the conversation
    /// has started, or the change cannot be saved.
    pub fn set_thread_model(
        &self,
        id: &str,
        agent: AgentKind,
        model: Option<String>,
    ) -> Result<(), Error> {
        let thread = self.thread(id)?;
        can_switch(&thread, agent)?;
        if thread.agent == agent && thread.model == model {
            return Ok(());
        }
        let effort = thread.effort.clone();
        self.switch_agent(&thread, agent, model, effort)?;
        // An idle agent was started with the old model.
        self.runner.retire_idle(id);
        Ok(())
    }

    /// Saves `agent`, `model` and, when given, `effort` as what `thread` runs with, and notes it.
    ///
    /// An undone or forked Claude chat kept its Claude session to resume; another agent is given
    /// the conversation as a transcript instead, so a new agent drops the session.
    ///
    /// # Errors
    ///
    /// Returns an error when the change cannot be saved.
    pub(super) fn switch_agent(
        &self,
        thread: &Thread,
        agent: AgentKind,
        model: Option<String>,
        effort: Option<String>,
    ) -> Result<(), Error> {
        let shown = format!(
            "agent {agent:?}, model {}, effort {}",
            model.as_deref().unwrap_or("default"),
            effort.as_deref().unwrap_or("default")
        );
        let switched = thread.agent != agent;
        if switched {
            self.set_resume_point(&thread.id, None)?;
        }
        self.store.update_thread(&thread.id, |t| {
            t.agent = agent;
            t.model = model;
            t.effort = effort;
            if switched {
                t.session_id = None;
            }
        })?;
        note(&thread.id, shown);
        Ok(())
    }

    /// Takes effect from the thread's next turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown or the change cannot be saved.
    pub fn set_thread_access_mode(&self, id: &str, mode: AccessMode) -> Result<(), Error> {
        let thread = self.thread(id)?;
        self.store.update_thread(id, |t| t.access_mode = mode)?;
        // An idle agent was started with the old setting.
        if thread.access_mode != mode {
            note(id, format!("{mode:?} access"));
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Takes effect from the thread's next turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown or the change cannot be saved.
    pub fn set_thread_plan_mode(&self, id: &str, on: bool) -> Result<(), Error> {
        let thread = self.thread(id)?;
        self.store.update_thread(id, |t| t.plan_mode = on)?;
        // An idle agent was started with the old setting.
        if thread.plan_mode != on {
            note(id, format!("plan mode {on}"));
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Takes effect from the thread's next turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown or the change cannot be saved.
    pub fn set_thread_effort(&self, id: &str, effort: Option<&str>) -> Result<(), Error> {
        let thread = self.thread(id)?;
        self.store
            .update_thread(id, |t| t.effort = effort.map(String::from))?;
        // An idle agent was started with the old setting.
        if thread.effort.as_deref() != effort {
            note(id, format!("effort {}", effort.unwrap_or("default")));
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Takes effect from the thread's next turn, or the API agent's next step. None follows
    /// Settings → Tokens.
    ///
    /// # Errors
    ///
    /// Returns an error when the thread is unknown or the change cannot be saved.
    pub fn set_thread_compact_window(&self, id: &str, window: Option<u64>) -> Result<(), Error> {
        let thread = self.thread(id)?;
        self.store
            .update_thread(id, |t| t.compact_window = window)?;
        // An idle agent was started with the old setting.
        if thread.compact_window != window {
            note(id, format!("compact window {window:?}"));
            self.runner.retire_idle(id);
            let tokens = self.store.settings().tokens;
            self.runner.set_compact_window(
                id,
                window.unwrap_or_else(|| tokens.compact_window(thread.agent)),
            );
        }
        Ok(())
    }
}

/// Refuses a change of agent once the conversation has started, since its session belongs to
/// one CLI. An imported chat has no session yet, so it can take any agent until its first turn.
///
/// # Errors
///
/// Returns an error when `agent` differs from the thread's and the conversation has started.
pub(super) fn can_switch(thread: &Thread, agent: AgentKind) -> Result<(), Error> {
    if thread.agent != agent && !thread.messages.is_empty() && !thread.context_pending {
        return Err("The agent can't change after the conversation has started".into());
    }
    Ok(())
}
