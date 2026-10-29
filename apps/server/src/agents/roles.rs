//! The custom agents a chat can summon, and the role it takes on.

use std::path::Path;

use crate::orchestration::{AgentEventEnvelope, AgentEvent};
use crate::{agents, provider};
use crate::Server;

impl Server {
    /// The custom agents a chat in the project can summon: the agents folder's and the
    /// project's own. A chat without a project has only the folder's.
    pub fn project_agents(&self, project_id: Option<&str>) -> Vec<agents::Agent> {
        match project_id.and_then(|id| self.project_root(id).ok()) {
            Some(root) => self.agents.available(Path::new(&root)),
            None => self.agents.list().to_vec(),
        }
    }

    pub(crate) fn thread_agents(&self, thread_id: &str) -> Vec<agents::Agent> {
        let project_id = self.store.with_threads(|threads| {
            threads
                .iter()
                .find(|t| t.id == thread_id)
                .and_then(|t| t.project_id.clone())
        });
        self.project_agents(project_id.as_deref())
    }

    /// Makes `role` the chat's custom agent, or none; the agent is told with the next message.
    pub fn set_thread_role(&self, id: &str, role: Option<String>) -> Result<(), String> {
        let mut changed = false;
        self.store.update_thread(id, |t| {
            if t.role != role {
                t.role = role.clone();
                t.role_pending = true;
                changed = true;
            }
        })?;
        if changed {
            self.emit(AgentEventEnvelope {
                thread_id: id.into(),
                event: AgentEvent::RoleChanged { role },
            });
        }
        Ok(())
    }

    /// Switches the chat to the model of the last custom agent `text` summons that names one,
    /// when the chat's CLI offers it, as `provider/model` or as the model alone. An idle agent
    /// is replaced; a running turn keeps its model until it ends.
    pub(crate) fn switch_to_agent_model(&self, thread_id: &str, text: &str) {
        if !text.contains('&') {
            return;
        }
        let agents = self.thread_agents(thread_id);
        let Some(wanted) = agents::mentioned(text)
            .filter_map(|name| agents.iter().find(|a| a.name == name)?.model.as_deref())
            .last()
        else {
            return;
        };
        let Some((agent, model)) = self.store.with_threads(|threads| {
            threads
                .iter()
                .find(|t| t.id == thread_id)
                .map(|t| (t.agent, t.model.clone()))
        }) else {
            return;
        };
        let models = provider::list_models(agent);
        let Some(offered) = agents::offered(wanted, models.iter().filter_map(|m| m.id.as_deref()))
            .map(str::to_string)
        else {
            return;
        };
        if model.as_ref() == Some(&offered) {
            return;
        }
        match self.set_thread_model(thread_id, agent, Some(offered.clone())) {
            Ok(()) => self.emit(AgentEventEnvelope {
                thread_id: thread_id.into(),
                event: AgentEvent::ModelChanged {
                    model: Some(offered),
                },
            }),
            Err(e) => log::warn!("failed to switch to the agent's model: {e}"),
        }
    }

    /// `text` with the roles of the custom agents it summons (see `agents::summon`), and of the
    /// chat's own agent when its agent was not told of it yet: the role was changed by hand, or
    /// `fresh`, a new agent session, knows of none. Also the primary agent it makes the chat's
    /// agent, which `told_role` keeps once the message is sent.
    pub(crate) fn summon_agents(
        &self,
        thread_id: &str,
        text: String,
        fresh: bool,
    ) -> (String, Option<String>) {
        let Some((role, pending)) = self.store.with_threads(|threads| {
            threads
                .iter()
                .find(|t| t.id == thread_id)
                .map(|t| (t.role.clone(), t.role_pending))
        }) else {
            return (text, None);
        };
        let standing = match &role {
            Some(name) if pending || fresh => agents::Standing::Untold(name),
            None if pending => agents::Standing::Removed,
            _ => agents::Standing::Told,
        };
        // Most messages summon no one, and then the agents are not read.
        if matches!(standing, agents::Standing::Told) && !text.contains('&') {
            return (text, None);
        }
        let summoned = agents::summon(&self.thread_agents(thread_id), text, standing);
        let switched = summoned.role.filter(|new| role.as_ref() != Some(new));
        (summoned.text, switched)
    }

    /// Records that the chat's agent was told of its role with a message just sent, and makes
    /// `switched`, a primary agent the message summoned, the chat's agent.
    pub(crate) fn told_role(&self, thread_id: &str, switched: Option<String>) {
        let pending = self
            .store
            .with_threads(|threads| threads.iter().any(|t| t.id == thread_id && t.role_pending));
        if !pending && switched.is_none() {
            return;
        }
        let saved = self.store.update_thread(thread_id, |t| {
            t.role_pending = false;
            if let Some(role) = &switched {
                t.role = Some(role.clone());
            }
        });
        if let Err(e) = saved {
            log::warn!("failed to save the chat's agent: {e}");
        } else if switched.is_some() {
            self.emit(AgentEventEnvelope {
                thread_id: thread_id.into(),
                event: AgentEvent::RoleChanged { role: switched },
            });
        }
    }
}
