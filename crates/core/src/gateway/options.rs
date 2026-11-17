//! What a chat runs with, as it is changed from a chat app: its agent, model, access and plan
//! mode.

// Rust guideline compliant 2026-10-09

use serde_json::json;

use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::provider::AgentKind;
use crate::threads::{AccessMode, Thread};
use crate::{Core, Error};

/// The access modes, by the names a chat app gives them.
pub(crate) const ACCESS: [(&str, AccessMode); 4] = [
    ("supervised", AccessMode::Supervised),
    ("auto-edits", AccessMode::AutoAcceptEdits),
    ("auto", AccessMode::Auto),
    ("full", AccessMode::FullAccess),
];

impl Core {
    /// Sets what `thread` runs with: its `model`, `agent` (with a model after its name), `plan`
    /// mode (on or off) or `access`, as `word` says.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is not one the option takes, or the change cannot be saved.
    pub(crate) fn set_chat_option(
        &self,
        thread: &Thread,
        word: &str,
        value: &str,
    ) -> Result<(), Error> {
        match word {
            "model" => {
                let model = Some(value.to_string()).filter(|m| !m.eq_ignore_ascii_case("default"));
                self.set_thread_model(&thread.id, thread.agent, model)?;
            }
            "agent" => self.switch_agent_named(thread, value)?,
            "plan" => match value.to_lowercase().as_str() {
                "on" => self.set_thread_plan_mode(&thread.id, true)?,
                "off" => self.set_thread_plan_mode(&thread.id, false)?,
                _ => return Err("Plan mode is on or off".into()),
            },
            "access" => {
                let lower = value.to_lowercase();
                let Some((_, mode)) = ACCESS.iter().find(|(name, _)| *name == lower) else {
                    let names: Vec<&str> = ACCESS.iter().map(|(name, _)| *name).collect();
                    return Err(format!("Access is one of: {}", names.join(", ")).into());
                };
                self.set_thread_access_mode(&thread.id, *mode)?;
            }
            _ => return Err(format!("A chat has no option {word}").into()),
        }
        self.emit(AgentEventEnvelope {
            thread_id: thread.id.clone(),
            event: AgentEvent::OptionsChanged,
        });
        Ok(())
    }

    /// Moves `thread` to the agent `rest` names, with the model after its name or the agent's
    /// default model.
    fn switch_agent_named(&self, thread: &Thread, rest: &str) -> Result<(), Error> {
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
        self.set_thread_model(&thread.id, agent, model)
    }
}

/// What `thread` runs with, as a chat app is shown it.
pub(crate) fn options(thread: &Thread) -> String {
    let access = ACCESS
        .iter()
        .find(|(_, mode)| *mode == thread.access_mode)
        .map_or("full", |(name, _)| *name);
    let plan = if thread.plan_mode { "on" } else { "off" };
    format!(
        "{}: {:?}, model {}, access {access}, plan mode {plan}.",
        thread.title,
        thread.agent,
        thread.model.as_deref().unwrap_or("default"),
    )
}
