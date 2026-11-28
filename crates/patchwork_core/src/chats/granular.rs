//! Granular chats: the agent, model and effort of a chat's orchestrator, its own agent that
//! plans, decides and reviews, and of the executor its sub agents run on to carry the plan out.
//!
//! A chat's picks are its own; a project's are given to every chat in it, and to each new one.
//! They only act while granular chats are on in Settings.

// Rust guideline compliant 2026-11-23

use serde::{Deserialize, Serialize};

use crate::persistence::Store;
use crate::provider::AgentKind;
use crate::chats::Chat;
use crate::{Error, Core};

use super::options::can_switch;

/// An agent with the model and effort it runs at. None leaves either to the agent's CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPick {
    pub agent: AgentKind,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// The agents behind a chat's own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Reserves {
    /// Runs the chat's sub agents, which search, edit, build and test.
    pub executor: Option<AgentPick>,
}

/// Everything a granular chat runs with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Granular {
    /// The chat's own agent, which plans the work and reviews the result.
    pub main: AgentPick,
    #[serde(flatten)]
    pub reserves: Reserves,
}

impl Granular {
    /// Drops an executor that is the orchestrator itself, which its sub agents run on anyway.
    fn tidy(&mut self) {
        if self.reserves.executor.as_ref() == Some(&self.main) {
            self.reserves.executor = None;
        }
    }
}

/// The `app_state` key of a chat's reserves.
fn chat_key(chat_id: &str) -> String {
    format!("granular:{chat_id}")
}

/// The `app_state` key of a project's granular settings.
pub(crate) fn project_key(project_id: &str) -> String {
    format!("granular-project:{project_id}")
}

/// The chat's reserves, whether or not granular chats are on.
fn saved(store: &Store, chat_id: &str) -> Reserves {
    store
        .state(&chat_key(chat_id))
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn save(store: &Store, chat_id: &str, reserves: &Reserves) -> Result<(), Error> {
    if *reserves == Reserves::default() {
        return store.clear_state(&chat_key(chat_id));
    }
    let json = serde_json::to_string(reserves).map_err(|e| e.to_string())?;
    store.set_state(&chat_key(chat_id), &json)
}

/// Where the chat's `run_sub_agent` calls run: the executor picked for it while granular chats
/// are on, else, for Patchwork's own agent, which has no sub agents of its own, on itself at its
/// own model. None leaves a CLI to run its sub agents itself.
pub(crate) fn sub_agent_of(store: &Store, chat: &Chat) -> Option<AgentPick> {
    if chat.parent.is_some() {
        return None;
    }
    if store.settings().granular_chats
        && let Some(executor) = saved(store, &chat.id).executor
    {
        return Some(executor);
    }
    (chat.agent == AgentKind::Api).then(|| AgentPick {
        agent: AgentKind::Api,
        model: chat.model.clone(),
        effort: chat.effort.clone(),
    })
}

impl Core {
    /// What the chat runs with: its own agent, and the reserves saved for it.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown.
    pub fn chat_granular(&self, id: &str) -> Result<Granular, Error> {
        let chat = self.chat(id)?;
        Ok(Granular {
            main: AgentPick {
                agent: chat.agent,
                model: chat.model,
                effort: chat.effort,
            },
            reserves: saved(&self.store, id),
        })
    }

    /// Sets what the chat runs with, from its next turn.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown, its agent would change after the conversation
    /// has started, or the change cannot be saved.
    pub fn set_chat_granular(&self, id: &str, mut granular: Granular) -> Result<(), Error> {
        granular.tidy();
        let chat = self.chat(id)?;
        let Granular { main, reserves } = granular;
        can_switch(&chat, main.agent)?;
        let same_main =
            chat.agent == main.agent && chat.model == main.model && chat.effort == main.effort;
        let same_reserves = saved(&self.store, id) == reserves;
        if same_main && same_reserves {
            return Ok(());
        }
        // The reserves first. Should the chat's own agent then fail to save, they stand behind
        // the agent it kept.
        if !same_reserves {
            save(&self.store, id, &reserves)?;
        }
        if !same_main {
            self.switch_agent(&chat, main.agent, main.model, main.effort)?;
        }
        // An idle agent was started with what the chat ran with before.
        self.runner.retire_idle(id);
        Ok(())
    }

    /// The granular settings every chat in the project is given, if any were set.
    #[must_use]
    pub fn project_granular(&self, project_id: &str) -> Option<Granular> {
        let json = self.store.state(&project_key(project_id))?;
        serde_json::from_str(&json).ok()
    }

    /// Sets what each new chat in the project starts with, and leaves the chats already in it as
    /// they are.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown or the settings cannot be saved.
    pub fn set_project_granular_default(
        &self,
        project_id: &str,
        mut granular: Granular,
    ) -> Result<(), Error> {
        self.project_root(project_id)?;
        granular.tidy();
        let json = serde_json::to_string(&granular).map_err(|e| e.to_string())?;
        self.store.set_state(&project_key(project_id), &json)
    }

    /// Forgets what each new chat in the project starts with, so a new chat there is asked.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be removed.
    pub fn clear_project_granular_default(&self, project_id: &str) -> Result<(), Error> {
        self.store.clear_state(&project_key(project_id))
    }

    /// Gives `granular` to every chat in the project, and to each new one. A chat that has
    /// started on another agent keeps its agent, since a conversation belongs to one, and takes
    /// the rest.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown or a change cannot be saved.
    pub fn set_project_granular(
        &self,
        project_id: &str,
        mut granular: Granular,
    ) -> Result<(), Error> {
        granular.tidy();
        self.set_project_granular_default(project_id, granular.clone())?;
        let chats: Vec<String> = self.store.with_chats(|chats| {
            chats
                .iter()
                .filter(|t| t.project_id.as_deref() == Some(project_id))
                .filter(|t| t.parent.is_none() && t.archived_at.is_none())
                .map(|t| t.id.clone())
                .collect()
        });
        // Every chat is tried, so one that fails does not leave the ones after it unchanged.
        let mut failed = Vec::new();
        for id in &chats {
            // Read as each is reached: a chat may have started since the list was made.
            let Some(chat) = self.store.chat(id) else {
                continue;
            };
            let mut given = granular.clone();
            let started = !chat.messages.is_empty() && !chat.context_pending;
            if started && chat.agent != given.main.agent {
                given.main = AgentPick {
                    agent: chat.agent,
                    model: chat.model,
                    effort: chat.effort,
                };
            }
            if let Err(e) = self.set_chat_granular(id, given) {
                log::warn!(
                    chat = id.as_str(), error:% = e;
                    "could not give {{chat}} its project's granular settings: {{error}}"
                );
                failed.push(e);
            }
        }
        match failed.len() {
            0 => Ok(()),
            n => Err(format!(
                "{n} of this project's {} chats could not be changed: {}",
                chats.len(),
                failed[0]
            )
            .into()),
        }
    }

    /// Whether a new chat in `project_id` is asked what it runs with: granular chats are on, and
    /// its project has no settings to give it.
    #[must_use]
    pub fn asks_granular(&self, project_id: Option<&str>) -> bool {
        self.store.settings().granular_chats
            && project_id.is_none_or(|id| self.project_granular(id).is_none())
    }

    /// Gives a chat just made in a project the project's granular settings.
    pub(super) fn give_project_granular(&self, chat: &mut Chat) {
        if !self.store.settings().granular_chats {
            return;
        }
        let Some(granular) = chat
            .project_id
            .as_deref()
            .and_then(|id| self.project_granular(id))
        else {
            return;
        };
        if let Err(e) = save(&self.store, &chat.id, &granular.reserves) {
            log::warn!(error:% = e; "could not save a new chat's granular settings: {{error}}");
        }
        chat.agent = granular.main.agent;
        chat.model = granular.main.model;
        chat.effort = granular.main.effort;
    }
}
