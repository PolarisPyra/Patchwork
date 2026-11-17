//! The commands that change what the chat a conversation talks to runs with: its agent, model,
//! access and plan mode.

// Rust guideline compliant 2026-10-09

use serde_json::json;

use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::provider::AgentKind;
use crate::threads::{AccessMode, Thread};
use crate::{Core, Error};

/// The access modes, by the names a chat app gives them.
const ACCESS: [(&str, AccessMode); 4] = [
    ("supervised", AccessMode::Supervised),
    ("auto-edits", AccessMode::AutoAcceptEdits),
    ("auto", AccessMode::Auto),
    ("full", AccessMode::FullAccess),
];

impl Core {
    /// Runs the option command `word` with `rest` on the chat `thread_id`, and returns what to
    /// tell the conversation. With nothing after the command, it shows the current options.
    pub(super) fn thread_option(&self, word: &str, rest: &str, thread_id: Option<&str>) -> String {
        let Some(thread) = thread_id.and_then(|id| self.store.thread(id)) else {
            return "No chat yet. Your next message starts one.".to_string();
        };
        if rest.is_empty() {
            return options(&thread);
        }
        let changed = match word {
            "model" => {
                let model = Some(rest.to_string()).filter(|m| !m.eq_ignore_ascii_case("default"));
                self.set_thread_model(&thread.id, thread.agent, model)
            }
            "agent" => self.switch_agent_named(&thread, rest),
            "plan" => match rest.to_lowercase().as_str() {
                "on" => self.set_thread_plan_mode(&thread.id, true),
                "off" => self.set_thread_plan_mode(&thread.id, false),
                _ => return "Send /plan on or /plan off.".to_string(),
            },
            "access" => {
                let lower = rest.to_lowercase();
                let Some((_, mode)) = ACCESS.iter().find(|(name, _)| *name == lower) else {
                    let names: Vec<&str> = ACCESS.iter().map(|(name, _)| *name).collect();
                    return format!("Send /access with one of: {}.", names.join(", "));
                };
                self.set_thread_access_mode(&thread.id, *mode)
            }
            _ => return String::new(),
        };
        if let Err(error) = changed {
            return format!("Patchwork could not change that: {error}");
        }
        self.emit(AgentEventEnvelope {
            thread_id: thread.id.clone(),
            event: AgentEvent::OptionsChanged,
        });
        match self.store.thread(&thread.id) {
            Some(thread) => options(&thread),
            None => "Changed.".to_string(),
        }
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
fn options(thread: &Thread) -> String {
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
