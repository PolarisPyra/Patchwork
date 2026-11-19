//! Making, changing and removing bots.

// Rust guideline compliant 2026-10-09

use super::types::{BlockedTools, Bot};
use crate::Core;
use crate::backgrounds::ChatBackground;
use crate::Error;
use crate::gateway::Platform;
use crate::system::{new_id, now_ms};
use crate::threads::AccessMode;

/// The longest a bot's name may be, in characters.
const NAME_MAX: usize = 40;

/// The longest a bot's instructions may be, in characters.
const INSTRUCTIONS_MAX: usize = 20_000;

/// The first word of a new bot's name.
const NAME_ADJECTIVES: &[&str] = &[
    "Amber", "Bold", "Brave", "Bright", "Calm", "Clever", "Cosmic", "Swift", "Gentle", "Golden",
    "Happy", "Jolly", "Lucky", "Mellow", "Misty", "Nimble", "Quiet", "Rapid", "Silver", "Sunny",
    "Tidy", "Velvet", "Witty", "Zesty",
];

/// The second word of a new bot's name.
const NAME_ANIMALS: &[&str] = &[
    "Badger", "Beaver", "Falcon", "Ferret", "Finch", "Fox", "Gecko", "Heron", "Koala", "Lemur",
    "Lynx", "Marten", "Moose", "Newt", "Otter", "Owl", "Panda", "Puffin", "Raven", "Robin", "Seal",
    "Sparrow", "Tapir", "Wombat",
];

impl Core {
    /// Every bot, oldest first.
    #[must_use]
    pub fn bots(&self) -> Vec<Bot> {
        self.store.bots()
    }

    /// The bot with `id`, if there is one.
    #[must_use]
    pub fn bot(&self, id: &str) -> Option<Bot> {
        self.store.bots().into_iter().find(|b| b.id == id)
    }

    /// Makes a bot on the agent new chats start on, with a name no other bot has, and returns
    /// it.
    ///
    /// # Errors
    ///
    /// Returns an error when the bot cannot be saved.
    pub fn create_bot(&self) -> Result<Bot, Error> {
        let settings = self.store.settings();
        let agent = settings.default_agent();
        let bot = Bot {
            id: new_id(),
            name: self.random_bot_name(),
            instructions: String::new(),
            agent,
            model: settings.tokens.default_models.get(&agent).cloned(),
            effort: None,
            // A bot runs on its own, so its edits are approved and anything else asks.
            access_mode: AccessMode::AutoAcceptEdits,
            projects: Vec::new(),
            created_at: i64::try_from(now_ms()).unwrap_or(i64::MAX),
            archived_at: None,
            background: None,
            blocked: BlockedTools::default(),
            chat_app: None,
            thread_id: None,
        };
        self.store.save_bot(&bot)?;
        Ok(bot)
    }

    /// The bot that answers messages from `platform`, made when there is none and brought
    /// back to the sidebar when it was archived.
    ///
    /// A new one is named for the platform and starts on the agent new chats start on, with
    /// the access chat apps are given and the project they work in.
    ///
    /// # Errors
    ///
    /// Returns an error when the bot cannot be saved.
    pub(crate) fn chat_app_bot(&self, platform: Platform) -> Result<Bot, Error> {
        if let Some(mut bot) = self
            .bots()
            .into_iter()
            .find(|b| b.chat_app == Some(platform))
        {
            if bot.archived_at.is_some() {
                bot.archived_at = None;
                self.store.save_bot(&bot)?;
            }
            return Ok(bot);
        }
        let settings = self.store.settings();
        let agent = settings.default_agent();
        let bot = Bot {
            id: new_id(),
            name: self.free_bot_name(platform.label()),
            instructions: String::new(),
            agent,
            model: settings.tokens.default_models.get(&agent).cloned(),
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
            thread_id: None,
        };
        self.store.save_bot(&bot)?;
        Ok(bot)
    }

    /// A random adjective and animal no bot's name has, or a numbered "Bot" once every pair is
    /// taken.
    fn random_bot_name(&self) -> String {
        let taken: Vec<String> = self.bots().into_iter().map(|b| b.name).collect();
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
            .unwrap_or_else(|| self.free_bot_name("Bot"))
    }

    /// `base`, or `base` with the first number after it that no bot's name has.
    fn free_bot_name(&self, base: &str) -> String {
        let taken: Vec<String> = self.bots().into_iter().map(|b| b.name).collect();
        // One more number than there are bots always leaves a name free.
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

    /// Saves the changes to a bot that exists, and returns it as saved.
    ///
    /// Its name is trimmed, and its linked projects keep only those on this computer.
    ///
    /// # Errors
    ///
    /// Returns an error when the bot no longer exists, its name is empty, too long or another
    /// bot's, its instructions are too long, or saving it fails.
    pub fn save_bot(&self, mut bot: Bot) -> Result<Bot, Error> {
        let others: Vec<Bot> = self.bots();
        let Some(saved) = others.iter().find(|b| b.id == bot.id) else {
            return Err("The bot no longer exists".into());
        };
        bot.created_at = saved.created_at;
        // Only archiving moves a bot in or out of the sidebar.
        bot.archived_at = saved.archived_at;
        bot.background.clone_from(&saved.background);
        bot.blocked.clone_from(&saved.blocked);
        bot.chat_app = saved.chat_app;
        bot.thread_id.clone_from(&saved.thread_id);
        bot.name = bot.name.trim().to_string();
        if bot.name.is_empty() {
            return Err("A bot needs a name".into());
        }
        if bot.name.chars().count() > NAME_MAX || bot.name.chars().any(char::is_control) {
            return Err(
                format!("A bot's name is at most {NAME_MAX} characters on one line").into(),
            );
        }
        if others
            .iter()
            .any(|b| b.id != bot.id && b.name.eq_ignore_ascii_case(&bot.name))
        {
            return Err(format!("Another bot is called {}", bot.name).into());
        }
        if bot.instructions.chars().count() > INSTRUCTIONS_MAX {
            return Err(
                format!("A bot's instructions are at most {INSTRUCTIONS_MAX} characters").into(),
            );
        }
        let projects = self.store.settings().projects;
        bot.projects
            .retain(|id| projects.iter().any(|p| &p.id == id && p.host().is_none()));
        bot.projects.dedup();
        self.store.save_bot(&bot)?;
        Ok(bot)
    }

    /// Renames bot `id` to `name`.
    ///
    /// # Errors
    ///
    /// Returns an error when the bot does not exist or the name is turned down.
    pub fn rename_bot(&self, id: &str, name: &str) -> Result<Bot, Error> {
        let bot = self.bot(id).ok_or("The bot no longer exists")?;
        self.save_bot(Bot {
            name: name.to_string(),
            ..bot
        })
    }

    /// Moves bot `id` out of the sidebar when `archived`, or back into it.
    ///
    /// # Errors
    ///
    /// Returns an error when the bot does not exist or cannot be saved.
    pub fn set_bot_archived(&self, id: &str, archived: bool) -> Result<(), Error> {
        let mut bot = self.bot(id).ok_or("The bot no longer exists")?;
        bot.archived_at = archived.then(|| i64::try_from(now_ms()).unwrap_or(i64::MAX));
        self.store.save_bot(&bot)
    }

    /// Gives bot `id` a background of its own, or None to show the one every chat shares.
    /// Returns the bot as saved.
    ///
    /// # Errors
    ///
    /// Returns an error when the bot does not exist or cannot be saved.
    pub fn set_bot_background(
        &self,
        id: &str,
        background: Option<ChatBackground>,
    ) -> Result<Bot, Error> {
        let mut bot = self.bot(id).ok_or("The bot no longer exists")?;
        bot.background = background;
        self.store.save_bot(&bot)?;
        Ok(bot)
    }

    /// Sets the tools bot `id`'s runs may not use, and returns the bot as saved.
    ///
    /// Servers are kept by name, trimmed, without blanks or repeats.
    ///
    /// # Errors
    ///
    /// Returns an error when the bot does not exist or cannot be saved.
    pub fn set_bot_blocked(&self, id: &str, mut blocked: BlockedTools) -> Result<Bot, Error> {
        let mut bot = self.bot(id).ok_or("The bot no longer exists")?;
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
        bot.blocked = blocked;
        self.store.save_bot(&bot)?;
        Ok(bot)
    }

    /// The MCP servers a bot can block, by name: Patchwork's own and the agents', sorted.
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

    /// Deletes bot `id` and the files it knows. Its runs stay, as plain chats.
    ///
    /// # Errors
    ///
    /// Returns an error when the bot cannot be deleted or its runs cannot be unlinked.
    pub fn remove_bot(&self, id: &str) -> Result<(), Error> {
        self.store.remove_bot(id)?;
        let runs: Vec<String> = self.store.with_threads(|threads| {
            threads
                .iter()
                .filter(|t| t.bot_id.as_deref() == Some(id))
                .map(|t| t.id.clone())
                .collect()
        });
        for run in runs {
            self.store.update_thread(&run, |t| t.bot_id = None)?;
        }
        if let Some(dir) = self.bot_dir(id)
            && let Err(e) = std::fs::remove_dir_all(&dir)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("could not remove the bot's folder {}: {e}", dir.display());
        }
        Ok(())
    }
}
