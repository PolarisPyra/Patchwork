//! What a chat runs with: its model, access, effort and compact window.

// Rust guideline compliant 2026-10-09

use crate::Error;
use crate::provider::AgentKind;
use crate::chats::{AccessMode, Chat};
use crate::Core;
use std::sync::Arc;

use super::lifecycle::note;

impl Core {
    /// Sets the chat's model. The agent can only change before the first message,
    /// since the conversation session belongs to one CLI. An imported chat has no session yet,
    /// so it can take any agent until its first turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown, its agent would change after the conversation
    /// has started, or the change cannot be saved.
    pub(crate) fn set_chat_model(
        &self,
        id: &str,
        agent: AgentKind,
        model: Option<String>,
    ) -> Result<(), Error> {
        let chat = self.chat(id)?;
        can_switch(&chat, agent)?;
        if chat.agent == agent && chat.model == model {
            return Ok(());
        }
        let effort = chat.effort.clone();
        let live = self
            .runner
            .set_choice(id, agent, model.as_deref(), effort.as_deref());
        self.switch_agent(&chat, agent, model, effort)?;
        // An idle agent that cannot change models was started with the old one.
        if !live {
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Saves `agent`, `model` and, when given, `effort` as what `chat` runs with, and notes it.
    ///
    /// An undone or forked Claude chat kept its Claude session to resume; another agent is given
    /// the conversation as a transcript instead, so a new agent drops the session.
    ///
    /// # Errors
    ///
    /// Returns an error when the change cannot be saved.
    pub(super) fn switch_agent(
        &self,
        chat: &Chat,
        agent: AgentKind,
        model: Option<String>,
        effort: Option<String>,
    ) -> Result<(), Error> {
        let shown = format!(
            "agent {agent:?}, model {}, effort {}",
            model.as_deref().unwrap_or("default"),
            effort.as_deref().unwrap_or("default")
        );
        let switched = chat.agent != agent;
        if switched {
            self.set_resume_point(&chat.id, None)?;
        }
        self.store.update_chat(&chat.id, |t| {
            t.agent = agent;
            t.model = model;
            t.effort = effort;
            if switched {
                t.session_id = None;
            }
        })?;
        note(&chat.id, shown);
        Ok(())
    }

    /// Sets the chat's access mode, which its running reply follows from now on.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown or the change cannot be saved.
    pub(crate) fn set_chat_access_mode(
        self: &Arc<Self>,
        id: &str,
        mode: AccessMode,
    ) -> Result<(), Error> {
        let chat = self.chat(id)?;
        self.store.update_chat(id, |t| t.access_mode = mode)?;
        // An idle agent that cannot change modes was started with the old one.
        if chat.access_mode != mode {
            note(id, format!("{mode:?} access"));
            if !self.runner.set_access_mode(self, id, mode) {
                self.runner.retire_idle(id);
            }
        }
        Ok(())
    }

    /// Takes effect from the chat's next turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown or the change cannot be saved.
    pub(crate) fn set_chat_effort(&self, id: &str, effort: Option<&str>) -> Result<(), Error> {
        let chat = self.chat(id)?;
        self.store
            .update_chat(id, |t| t.effort = effort.map(String::from))?;
        // An idle agent that cannot change effort was started with the old one.
        if chat.effort.as_deref() != effort {
            note(id, format!("effort {}", effort.unwrap_or("default")));
            let live = self
                .runner
                .set_choice(id, chat.agent, chat.model.as_deref(), effort);
            if !live {
                self.runner.retire_idle(id);
            }
        }
        Ok(())
    }

    /// Takes effect from the chat's next turn, or the API agent's next step. None follows
    /// Settings → Tokens.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown or the change cannot be saved.
    pub fn set_chat_compact_window(&self, id: &str, window: Option<u64>) -> Result<(), Error> {
        let chat = self.chat(id)?;
        self.store.update_chat(id, |t| t.compact_window = window)?;
        // An idle agent was started with the old setting.
        if chat.compact_window != window {
            note(id, format!("compact window {window:?}"));
            self.runner.retire_idle(id);
            let tokens = self.store.settings().tokens;
            self.runner.set_compact_window(
                id,
                window.unwrap_or_else(|| tokens.compact_window(chat.agent)),
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
/// Returns an error when `agent` differs from the chat's and the conversation has started.
pub(super) fn can_switch(chat: &Chat, agent: AgentKind) -> Result<(), Error> {
    if chat.agent != agent && !chat.messages.is_empty() && !chat.context_pending {
        return Err("The agent can't change after the conversation has started".into());
    }
    Ok(())
}
