//! What a chat runs with, as it is changed from a chat app: its agent, model and access.

// Rust guideline compliant 2026-10-09

use serde_json::json;

use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::provider::AgentKind;
use crate::chats::{AccessMode, Chat};
use crate::{Core, Error};
use std::sync::Arc;

/// The access modes, by the names a chat app gives them.
pub(crate) const ACCESS: [(&str, AccessMode); 4] = [
    ("supervised", AccessMode::Supervised),
    ("auto-edits", AccessMode::AutoAcceptEdits),
    ("auto", AccessMode::Auto),
    ("full", AccessMode::FullAccess),
];

impl Core {
    /// Sets what `chat` runs with: its `model`, `agent` (with a model after its name) or
    /// `access`, as `word` says.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not one the option takes, or the change cannot be saved.
    pub(crate) fn set_chat_option(
        self: &Arc<Self>,
        chat: &Chat,
        word: &str,
        value: &str,
    ) -> Result<(), Error> {
        match word {
            "model" => {
                let model = Some(value.to_string()).filter(|m| !m.eq_ignore_ascii_case("default"));
                self.set_chat_model(&chat.id, chat.agent, model)?;
            }
            "agent" => self.switch_agent_named(chat, value)?,
            "access" => {
                let lower = value.to_lowercase();
                let Some((_, mode)) = ACCESS.iter().find(|(name, _)| *name == lower) else {
                    let names: Vec<&str> = ACCESS.iter().map(|(name, _)| *name).collect();
                    return Err(format!("Access is one of: {}", names.join(", ")).into());
                };
                self.set_chat_access_mode(&chat.id, *mode)?;
            }
            _ => return Err(format!("A chat has no option {word}").into()),
        }
        self.emit(AgentEventEnvelope {
            chat_id: chat.id.clone(),
            event: AgentEvent::OptionsChanged,
        });
        Ok(())
    }

    /// Moves `chat` to the agent `rest` names, with the model after its name or the agent's
    /// default model.
    fn switch_agent_named(&self, chat: &Chat, rest: &str) -> Result<(), Error> {
        let (name, model) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let agent = serde_json::from_value::<AgentKind>(json!(name.to_lowercase()))
            .map_err(|_unknown| format!("Patchwork has no agent {name}"))?;
        let model = match model.trim() {
            "" => self
                .store
                .settings()
                .tokens
                .default_models
                .get(&agent)
                .cloned(),
            model => Some(model.to_string()),
        };
        self.set_chat_model(&chat.id, agent, model)
    }
}

/// What `chat` runs with, as a chat app is shown it.
pub(crate) fn options(chat: &Chat) -> String {
    let access = ACCESS
        .iter()
        .find(|(_, mode)| *mode == chat.access_mode)
        .map_or("full", |(name, _)| *name);
    format!(
        "{}: {:?}, model {}, access {access}.",
        chat.title,
        chat.agent,
        chat.model.as_deref().unwrap_or("default"),
    )
}
