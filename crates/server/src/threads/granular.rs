//! Granular chats: the model and effort of a chat's own agent, of the fallback that takes over
//! when that agent's provider is out of usage, and of the sub agents each of them starts.
//!
//! A chat's picks are its own; a project's are given to every chat in it, and to each new one.
//! They only act while granular chats are on in Settings.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use crate::persistence::Store;
use crate::provider::AgentKind;
use crate::settings::Settings;
use crate::threads::Thread;
use crate::{Error, Server};

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
    /// What the chat's agent runs its sub agents at. Naming the chat's agent sets the model and
    /// effort of the sub agents its CLI starts; naming another has Patchwork run them there.
    pub sub: Option<AgentPick>,
    /// What the fallback runs its sub agents at, as `sub` is for the chat's agent.
    pub sub_fallback: Option<AgentPick>,
}

/// Everything a granular chat runs with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Granular {
    pub main: AgentPick,
    #[serde(flatten)]
    pub reserves: Reserves,
}

/// Whether the sub agents `agent` starts on itself can be given a model and effort: by its CLI,
/// or, for Patchwork's own agent, by Patchwork, which runs them.
#[must_use]
pub fn runs_sub_agents_at(agent: AgentKind) -> bool {
    matches!(agent, AgentKind::Claude | AgentKind::Codex | AgentKind::Api)
}

/// Whether `agent` can hand its sub agents' work to another agent, which Patchwork runs for it:
/// the agents given Patchwork's own tools.
#[must_use]
pub fn delegates_sub_agents(agent: AgentKind) -> bool {
    matches!(
        agent,
        AgentKind::Claude | AgentKind::Codex | AgentKind::OpenCode | AgentKind::Api
    )
}

impl Granular {
    /// Drops the picks that could never act: a fallback that is the chat's own agent and model,
    /// and sub agents the agent that would start them can neither run nor hand over.
    fn tidy(&mut self) {
        let Reserves {
            fallback,
            sub,
            sub_fallback,
        } = &mut self.reserves;
        let main = &self.main;
        if fallback
            .as_ref()
            .is_some_and(|f| f.agent == main.agent && f.model == main.model)
        {
            *fallback = None;
        }
        let fits = |pick: &AgentPick, agent: AgentKind| {
            if pick.agent == agent {
                runs_sub_agents_at(agent)
            } else {
                delegates_sub_agents(agent)
            }
        };
        if sub.as_ref().is_some_and(|s| !fits(s, main.agent)) {
            *sub = None;
        }
        let behind = fallback.as_ref().map(|f| f.agent);
        if sub_fallback
            .as_ref()
            .is_some_and(|s| !behind.is_some_and(|agent| fits(s, agent)))
        {
            *sub_fallback = None;
        }
    }
}

/// The `app_state` key of a thread's reserves.
fn thread_key(thread_id: &str) -> String {
    format!("granular:{thread_id}")
}

/// The `app_state` key of a project's granular settings.
pub(crate) fn project_key(project_id: &str) -> String {
    format!("granular-project:{project_id}")
}

/// The thread's reserves, whether or not granular chats are on.
fn saved(store: &Store, thread_id: &str) -> Reserves {
    store
        .state(&thread_key(thread_id))
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn save(store: &Store, thread_id: &str, reserves: &Reserves) -> Result<(), Error> {
    if *reserves == Reserves::default() {
        return store.clear_state(&thread_key(thread_id));
    }
    let json = serde_json::to_string(reserves).map_err(|e| e.to_string())?;
    store.set_state(&thread_key(thread_id), &json)
}

/// The model and effort the thread's sub agents run at, when its `agent` can be told.
pub(crate) fn sub_pick(
    store: &Store,
    settings: &Settings,
    thread_id: &str,
    agent: AgentKind,
) -> Option<AgentPick> {
    if !settings.granular_chats || !runs_sub_agents_at(agent) {
        return None;
    }
    saved(store, thread_id).sub.filter(|s| s.agent == agent)
}

/// Where the thread's `run_sub_agent` calls run: on the other agent its chat names, else, for
/// Patchwork's own agent, which has no sub agents of its own, on itself at the chat's sub agent
/// pick or its own model.
pub(crate) fn sub_agent_of(
    store: &Store,
    settings: &Settings,
    thread: &Thread,
) -> Option<AgentPick> {
    if let Some(pick) = delegate_of(store, settings, thread) {
        return Some(pick);
    }
    if thread.agent != AgentKind::Api || thread.parent.is_some() {
        return None;
    }
    let own = AgentPick {
        agent: AgentKind::Api,
        model: thread.model.clone(),
        effort: thread.effort.clone(),
    };
    Some(sub_pick(store, settings, &thread.id, AgentKind::Api).unwrap_or(own))
}

/// The other agent the thread's sub agents run on, when its own can hand work to one.
fn delegate_of(store: &Store, settings: &Settings, thread: &Thread) -> Option<AgentPick> {
    if !settings.granular_chats || thread.parent.is_some() || !delegates_sub_agents(thread.agent) {
        return None;
    }
    saved(store, &thread.id)
        .sub
        .filter(|s| s.agent != thread.agent)
}

/// The agent that takes the thread over when its own is out of usage, if it has one.
pub(crate) fn fallback_of(
    store: &Store,
    settings: &Settings,
    thread: &Thread,
) -> Option<AgentPick> {
    if !settings.granular_chats || thread.parent.is_some() {
        return None;
    }
    saved(store, &thread.id)
        .fallback
        .filter(|f| f.agent != thread.agent || f.model != thread.model)
}

/// Moves the thread to its fallback, which is told the conversation on its next turn, and returns
/// the fallback. None when the thread has none to move to.
pub(crate) fn fall_back(store: &Store, settings: &Settings, thread: &Thread) -> Option<AgentPick> {
    let fallback = fallback_of(store, settings, thread)?;
    let reserves = saved(store, &thread.id);
    let to = fallback.clone();
    let moved = store.update_thread(&thread.id, |t| {
        t.agent = to.agent;
        t.model = to.model;
        t.effort = to.effort;
        // The session belongs to the agent that ran out.
        t.session_id = None;
        t.context_pending = !t.messages.is_empty();
    });
    if let Err(e) = moved {
        log::warn!(
            thread = thread.id.as_str(), error:% = e;
            "could not move {{thread}} to its fallback: {{error}}"
        );
        return None;
    }
    // A fallback is used once, and its sub agents are now the chat's. The chat has moved either
    // way: reserves left as they were name a fallback it is now on, which is no fallback, and
    // sub agents of the agent it left, which are not used.
    let left = Reserves {
        fallback: None,
        sub: reserves.sub_fallback,
        sub_fallback: None,
    };
    if let Err(e) = save(store, &thread.id, &left) {
        log::warn!(
            thread = thread.id.as_str(), error:% = e;
            "could not save what {{thread}} has left behind its fallback: {{error}}"
        );
    }
    Some(fallback)
}

impl Server {
    /// What the chat runs with: its own agent, and the reserves saved for it.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is unknown.
    pub fn thread_granular(&self, id: &str) -> Result<Granular, Error> {
        let thread = self.thread(id)?;
        Ok(Granular {
            main: AgentPick {
                agent: thread.agent,
                model: thread.model,
                effort: thread.effort,
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
    pub fn set_thread_granular(&self, id: &str, mut granular: Granular) -> Result<(), Error> {
        granular.tidy();
        let thread = self.thread(id)?;
        let Granular { main, reserves } = granular;
        can_switch(&thread, main.agent)?;
        let same_main = thread.agent == main.agent
            && thread.model == main.model
            && thread.effort == main.effort;
        let same_reserves = saved(&self.store, id) == reserves;
        if same_main && same_reserves {
            return Ok(());
        }
        // The reserves first. Should the chat's own agent then fail to save, they stand behind
        // the agent it kept: sub agents of another agent are not used, and it has a fallback.
        if !same_reserves {
            save(&self.store, id, &reserves)?;
        }
        if !same_main {
            self.switch_agent(&thread, main.agent, main.model, main.effort)?;
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
        let chats: Vec<String> = self.store.with_threads(|threads| {
            threads
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
            let Some(thread) = self.store.thread(id) else {
                continue;
            };
            let mut given = granular.clone();
            let started = !thread.messages.is_empty() && !thread.context_pending;
            if started && thread.agent != given.main.agent {
                given.main = AgentPick {
                    agent: thread.agent,
                    model: thread.model,
                    effort: thread.effort,
                };
            }
            if let Err(e) = self.set_thread_granular(id, given) {
                log::warn!(
                    thread = id.as_str(), error:% = e;
                    "could not give {{thread}} its project's granular settings: {{error}}"
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
    pub(super) fn give_project_granular(&self, thread: &mut Thread) {
        if !self.store.settings().granular_chats {
            return;
        }
        let Some(granular) = thread
            .project_id
            .as_deref()
            .and_then(|id| self.project_granular(id))
        else {
            return;
        };
        if let Err(e) = save(&self.store, &thread.id, &granular.reserves) {
            log::warn!(error:% = e; "could not save a new chat's granular settings: {{error}}");
        }
        thread.agent = granular.main.agent;
        thread.model = granular.main.model;
        thread.effort = granular.main.effort;
    }
}
