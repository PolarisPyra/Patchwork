//! Granular chats: the model and effort of a chat's own agent, of the fallback that takes over
//! when that agent's provider is out of usage, and of the agent that reviews what a reply changed.
//!
//! A chat's picks are its own; a project's are given to every chat in it, and to each new one.
//! They only act while granular chats are on in Settings.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use crate::persistence::Store;
use crate::provider::AgentKind;
use crate::settings::Settings;
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
    /// Takes the chat over when its agent's provider is out of usage.
    pub fallback: Option<AgentPick>,
    /// Reviews the files each reply changed, as a second opinion from another model.
    pub review: Option<AgentPick>,
}

/// Everything a granular chat runs with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Granular {
    pub main: AgentPick,
    #[serde(flatten)]
    pub reserves: Reserves,
}

impl Granular {
    /// Drops the picks that could never act: a fallback or reviewer that is the chat's own agent
    /// and model.
    fn tidy(&mut self) {
        let Reserves { fallback, review } = &mut self.reserves;
        let main = &self.main;
        let same = |pick: &AgentPick| pick.agent == main.agent && pick.model == main.model;
        if fallback.as_ref().is_some_and(same) {
            *fallback = None;
        }
        // A review by the model that wrote the changes is no second opinion.
        if review.as_ref().is_some_and(same) {
            *review = None;
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

/// Where the chat's `run_sub_agent` calls run: Patchwork's own agent, which has no sub agents
/// of its own, runs them on itself at its own model.
pub(crate) fn sub_agent_of(chat: &Chat) -> Option<AgentPick> {
    if chat.agent != AgentKind::Api || chat.parent.is_some() {
        return None;
    }
    Some(AgentPick {
        agent: AgentKind::Api,
        model: chat.model.clone(),
        effort: chat.effort.clone(),
    })
}

/// The agent that reviews the files the chat's replies change, if it has one other than the
/// model that changed them and review is on.
pub(crate) fn reviewer_of(store: &Store, settings: &Settings, chat: &Chat) -> Option<AgentPick> {
    if !settings.granular_chats || settings.review_off || chat.parent.is_some() {
        return None;
    }
    saved(store, &chat.id)
        .review
        .filter(|r| r.agent != chat.agent || r.model != chat.model)
}

/// The agent that takes the chat over when its own is out of usage, if it has one.
pub(crate) fn fallback_of(store: &Store, settings: &Settings, chat: &Chat) -> Option<AgentPick> {
    if !settings.granular_chats || chat.parent.is_some() {
        return None;
    }
    saved(store, &chat.id)
        .fallback
        .filter(|f| f.agent != chat.agent || f.model != chat.model)
}

/// Moves the chat to its fallback, which is told the conversation on its next turn, and returns
/// the fallback. None when the chat has none to move to.
pub(crate) fn fall_back(store: &Store, settings: &Settings, chat: &Chat) -> Option<AgentPick> {
    let fallback = fallback_of(store, settings, chat)?;
    let reserves = saved(store, &chat.id);
    let to = fallback.clone();
    let moved = store.update_chat(&chat.id, |t| {
        t.agent = to.agent;
        t.model = to.model;
        t.effort = to.effort;
        // The session belongs to the agent that ran out.
        t.session_id = None;
        t.context_pending = !t.messages.is_empty();
    });
    if let Err(e) = moved {
        log::warn!(
            chat = chat.id.as_str(), error:% = e;
            "could not move {{chat}} to its fallback: {{error}}"
        );
        return None;
    }
    // A fallback is used once. The chat has moved either way: reserves left as they were name a
    // fallback it is now on, which is no fallback.
    let left = Reserves {
        fallback: None,
        review: reserves.review,
    };
    if let Err(e) = save(store, &chat.id, &left) {
        log::warn!(
            chat = chat.id.as_str(), error:% = e;
            "could not save what {{chat}} has left behind its fallback: {{error}}"
        );
    }
    Some(fallback)
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
