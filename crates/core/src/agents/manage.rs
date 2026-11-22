//! Making, changing and removing agents.

// Rust guideline compliant 2026-10-09

use super::types::{BlockedTools, Agent};
use crate::Core;
use crate::backgrounds::ChatBackground;
use crate::Error;
use crate::gateway::Platform;
use crate::system::{new_id, now_ms};
use crate::chats::AccessMode;

/// The longest an agent's name may be, in characters.
const NAME_MAX: usize = 40;

/// The longest an agent's instructions may be, in characters.
const INSTRUCTIONS_MAX: usize = 20_000;

/// The first word of a new agent's name.
const NAME_ADJECTIVES: &[&str] = &[
    "Amber", "Bold", "Brave", "Bright", "Calm", "Clever", "Cosmic", "Swift", "Gentle", "Golden",
    "Happy", "Jolly", "Lucky", "Mellow", "Misty", "Nimble", "Quiet", "Rapid", "Silver", "Sunny",
    "Tidy", "Velvet", "Witty", "Zesty",
];

/// The second word of a new agent's name.
const NAME_ANIMALS: &[&str] = &[
    "Badger", "Beaver", "Falcon", "Ferret", "Finch", "Fox", "Gecko", "Heron", "Koala", "Lemur",
    "Lynx", "Marten", "Moose", "Newt", "Otter", "Owl", "Panda", "Puffin", "Raven", "Robin", "Seal",
    "Sparrow", "Tapir", "Wombat",
];

impl Core {
    /// Every agent, oldest first.
    #[must_use]
    pub fn agents(&self) -> Vec<Agent> {
        self.store.agents()
    }

    /// The agent with `id`, if there is one.
    #[must_use]
    pub fn agent(&self, id: &str) -> Option<Agent> {
        self.store.agents().into_iter().find(|b| b.id == id)
    }

    /// Makes an agent on the agent new chats start on, with a name no other agent has, and returns
    /// it.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent cannot be saved.
    pub fn create_agent(&self) -> Result<Agent, Error> {
        let settings = self.store.settings();
        let kind = settings.default_agent();
        let agent = Agent {
            id: new_id(),
            name: self.random_agent_name(),
            instructions: String::new(),
            agent: kind,
            model: settings.tokens.default_models.get(&kind).cloned(),
            effort: None,
            // An agent runs on its own, so its edits are approved and anything else asks.
            access_mode: AccessMode::AutoAcceptEdits,
            projects: Vec::new(),
            created_at: i64::try_from(now_ms()).unwrap_or(i64::MAX),
            archived_at: None,
            background: None,
            blocked: BlockedTools::default(),
            chat_app: None,
            chat_id: None,
        };
        self.store.save_agent(&agent)?;
        Ok(agent)
    }

    /// The agent that answers messages from `platform`, made when there is none and brought
    /// back to the sidebar when it was archived.
    ///
    /// A new one is named for the platform and starts on the agent new chats start on, with
    /// the access chat apps are given and the project they work in.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent cannot be saved.
    pub(crate) fn chat_app_agent(&self, platform: Platform) -> Result<Agent, Error> {
        if let Some(mut agent) = self
            .agents()
            .into_iter()
            .find(|b| b.chat_app == Some(platform))
        {
            if agent.archived_at.is_some() {
                agent.archived_at = None;
                self.store.save_agent(&agent)?;
            }
            return Ok(agent);
        }
        let settings = self.store.settings();
        let kind = settings.default_agent();
        let agent = Agent {
            id: new_id(),
            name: self.free_agent_name(platform.label()),
            instructions: String::new(),
            agent: kind,
            model: settings.tokens.default_models.get(&kind).cloned(),
            effort: None,
            access_mode: settings.gateway.access,
            projects: settings
                .gateway
                .project_id
                .iter()
                .filter(|id| {
                    settings
                        .projects
                        .iter()
                        .any(|p| &p.id == *id && p.host().is_none())
                })
                .cloned()
                .collect(),
            created_at: i64::try_from(now_ms()).unwrap_or(i64::MAX),
            archived_at: None,
            background: None,
            blocked: BlockedTools::default(),
            chat_app: Some(platform),
            chat_id: None,
        };
        self.store.save_agent(&agent)?;
        Ok(agent)
    }

    /// A random adjective and animal no agent's name has, or a numbered "Agent" once every pair is
    /// taken.
    fn random_agent_name(&self) -> String {
        let taken: Vec<String> = self.agents().into_iter().map(|b| b.name).collect();
        let pairs = NAME_ADJECTIVES.len() * NAME_ANIMALS.len();
        // Start at a random pair and walk on from it, so a free one is found when it exists.
        let start = (uuid::Uuid::new_v4().as_u128() % pairs as u128) as usize;
        (0..pairs)
            .map(|i| {
                let n = (start + i) % pairs;
                let adjective = NAME_ADJECTIVES[n / NAME_ANIMALS.len()];
                format!("{adjective} {}", NAME_ANIMALS[n % NAME_ANIMALS.len()])
            })
            .find(|name| !taken.iter().any(|t| t.eq_ignore_ascii_case(name)))
            .unwrap_or_else(|| self.free_agent_name("Agent"))
    }

    /// `base`, or `base` with the first number after it that no agent's name has.
    fn free_agent_name(&self, base: &str) -> String {
        let taken: Vec<String> = self.agents().into_iter().map(|b| b.name).collect();
        // One more number than there are agents always leaves a name free.
        (1..=taken.len() + 1)
            .map(|n| {
                if n == 1 {
                    base.to_string()
                } else {
                    format!("{base} {n}")
                }
            })
            .find(|name| !taken.iter().any(|t| t.eq_ignore_ascii_case(name)))
            .unwrap_or_else(|| base.into())
    }

    /// Saves the changes to an agent that exists, and returns it as saved.
    ///
    /// Its name is trimmed, and its linked projects keep only those on this computer.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent no longer exists, its name is empty, too long or another
    /// agent's, its instructions are too long, or saving it fails.
    pub fn save_agent(&self, mut agent: Agent) -> Result<Agent, Error> {
        let others: Vec<Agent> = self.agents();
        let Some(saved) = others.iter().find(|b| b.id == agent.id) else {
            return Err("The agent no longer exists".into());
        };
        agent.created_at = saved.created_at;
        // Only archiving moves an agent in or out of the sidebar.
        agent.archived_at = saved.archived_at;
        agent.background.clone_from(&saved.background);
        agent.blocked.clone_from(&saved.blocked);
        agent.chat_app = saved.chat_app;
        agent.chat_id.clone_from(&saved.chat_id);
        agent.name = agent.name.trim().to_string();
        if agent.name.is_empty() {
            return Err("An agent needs a name".into());
        }
        if agent.name.chars().count() > NAME_MAX || agent.name.chars().any(char::is_control) {
            return Err(
                format!("An agent's name is at most {NAME_MAX} characters on one line").into(),
            );
        }
        if others
            .iter()
            .any(|b| b.id != agent.id && b.name.eq_ignore_ascii_case(&agent.name))
        {
            return Err(format!("Another agent is called {}", agent.name).into());
        }
        if agent.instructions.chars().count() > INSTRUCTIONS_MAX {
            return Err(format!(
                "An agent's instructions are at most {INSTRUCTIONS_MAX} characters"
            )
            .into());
        }
        let projects = self.store.settings().projects;
        agent
            .projects
            .retain(|id| projects.iter().any(|p| &p.id == id && p.host().is_none()));
        agent.projects.dedup();
        self.store.save_agent(&agent)?;
        Ok(agent)
    }

    /// Renames agent `id` to `name`.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent does not exist or the name is turned down.
    pub fn rename_agent(&self, id: &str, name: &str) -> Result<Agent, Error> {
        let agent = self.agent(id).ok_or("The agent no longer exists")?;
        self.save_agent(Agent {
            name: name.to_string(),
            ..agent
        })
    }

    /// Moves agent `id` out of the sidebar when `archived`, or back into it.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent does not exist or cannot be saved.
    pub fn set_agent_archived(&self, id: &str, archived: bool) -> Result<(), Error> {
        let mut agent = self.agent(id).ok_or("The agent no longer exists")?;
        agent.archived_at = archived.then(|| i64::try_from(now_ms()).unwrap_or(i64::MAX));
        self.store.save_agent(&agent)
    }

    /// Gives agent `id` a background of its own, or None to show the one every chat shares.
    /// Returns the agent as saved.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent does not exist or cannot be saved.
    pub fn set_agent_background(
        &self,
        id: &str,
        background: Option<ChatBackground>,
    ) -> Result<Agent, Error> {
        let mut agent = self.agent(id).ok_or("The agent no longer exists")?;
        agent.background = background;
        self.store.save_agent(&agent)?;
        Ok(agent)
    }

    /// Sets the tools agent `id`'s runs may not use, and returns the agent as saved.
    ///
    /// Servers are kept by name, trimmed, without blanks or repeats.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent does not exist or cannot be saved.
    pub fn set_agent_blocked(&self, id: &str, mut blocked: BlockedTools) -> Result<Agent, Error> {
        let mut agent = self.agent(id).ok_or("The agent no longer exists")?;
        blocked.kinds.sort();
        blocked.kinds.dedup();
        blocked.servers = blocked
            .servers
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        blocked.servers.sort();
        blocked.servers.dedup();
        agent.blocked = blocked;
        self.store.save_agent(&agent)?;
        Ok(agent)
    }

    /// The MCP servers an agent can block, by name: Patchwork's own and the agents', sorted.
    #[must_use]
    pub fn mcp_server_names(&self) -> Vec<String> {
        let projects = self.store.settings().projects;
        let mut names: Vec<String> = crate::provider::mcp::list()
            .into_iter()
            .map(|s| s.name)
            .chain(
                crate::provider::mcp::clis::list(&projects)
                    .into_iter()
                    .flat_map(|g| g.servers)
                    .map(|s| s.name),
            )
            .filter(|name| name != crate::provider::mcp::OWN_SERVER)
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    }

    /// Deletes agent `id` and the files it knows. Its runs stay, as plain chats.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent cannot be deleted or its runs cannot be unlinked.
    pub fn remove_agent(&self, id: &str) -> Result<(), Error> {
        self.store.remove_agent(id)?;
        let runs: Vec<String> = self.store.with_chats(|chats| {
            chats
                .iter()
                .filter(|t| t.agent_id.as_deref() == Some(id))
                .map(|t| t.id.clone())
                .collect()
        });
        for run in runs {
            self.store.update_chat(&run, |t| t.agent_id = None)?;
        }
        if let Some(dir) = self.agent_dir(id)
            && let Err(e) = std::fs::remove_dir_all(&dir)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("could not remove the agent's folder {}: {e}", dir.display());
        }
        Ok(())
    }
}
