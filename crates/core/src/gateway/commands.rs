//! The few slash commands a chat app conversation can send, which Patchwork answers itself
//! instead of passing to the agent: the ones that act on the conversation itself, and the ones
//! the buttons under a notice send. Everything else is asked for in words and done through the
//! capabilities.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use super::hub::Place;
use super::types::Incoming;
use crate::Core;

/// What `/help` answers.
const HELP: &str = "Say what you want in your own words: ask what is running, start or stop a chat, change a setting or the theme, open a project or a file on the computer, and the rest. Ask what Patchwork can do here for the whole list.

/new: start this conversation over with the next message
/new <project>: talk straight to a new chat in a project
/stop: stop the reply being written

When the agent asks before acting, reply allow, always or deny, or press a button. When it asks a question, reply with the numbers of the answers, one line per question.";

impl Core {
    /// Answers a command Patchwork handles, and says whether it was one.
    pub(super) fn gateway_command(
        self: &Arc<Self>,
        message: &Incoming,
        thread: Option<&str>,
    ) -> bool {
        let (platform, chat) = (message.platform, message.chat.as_str());
        let text = message.text.trim().trim_start_matches('/');
        let (word, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        // Telegram names the bot in commands sent in groups: `/new@patchwork_bot`.
        let word = word.split('@').next().unwrap_or(word).to_lowercase();
        let rest = rest.trim();
        let settings = self.store.settings();
        let reply = match word.as_str() {
            "help" | "start" => HELP.to_string(),
            "new" => {
                let project = if rest.is_empty() {
                    None
                } else if rest.eq_ignore_ascii_case("chat") {
                    Some(Place::Chat)
                } else {
                    let lower = rest.to_lowercase();
                    let found = settings
                        .projects
                        .iter()
                        .find(|p| p.name.to_lowercase() == lower)
                        .or_else(|| {
                            settings
                                .projects
                                .iter()
                                .find(|p| p.name.to_lowercase().contains(&lower))
                        });
                    let Some(p) = found else {
                        self.say(
                            platform,
                            chat,
                            &format!("No project is named {rest}. Ask which projects there are."),
                        );
                        return true;
                    };
                    Some(Place::Project(p.id.clone()))
                };
                let name = match &project {
                    Some(Place::Project(id)) => settings
                        .projects
                        .iter()
                        .find(|p| &p.id == id)
                        .map(|p| p.name.clone()),
                    Some(Place::Chat) => None,
                    None => settings
                        .gateway
                        .project_id
                        .as_ref()
                        .and_then(|id| settings.projects.iter().find(|p| &p.id == id))
                        .map(|p| p.name.clone()),
                };
                self.bind_chat(platform, chat, None);
                self.gateway
                    .lock()
                    .chats
                    .entry((platform, chat.to_string()))
                    .or_default()
                    .project = project;
                match name {
                    Some(name) => format!("Your next message starts a new chat in {name}."),
                    None => "Your next message starts a new chat.".to_string(),
                }
            }
            "stop" => match thread {
                Some(id) if self.runner.is_running(id) => {
                    self.cancel_turn(id);
                    "Stopped.".to_string()
                }
                _ => "Nothing is running.".to_string(),
            },
            "mute" => self.mute_notices(rest),
            "resume" => self.resume_at_reset(rest),
            "answer" => {
                let (id, reply) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                self.answer_by_id(platform, chat, id, reply.trim());
                return true;
            }
            "open" => match self.continue_on_chat(rest, platform, chat) {
                Ok(()) => return true,
                Err(error) => format!("Patchwork could not open that chat: {error}"),
            },
            _ => return false,
        };
        self.say(platform, chat, &reply);
        true
    }
}
