//! The few slash commands a chat app conversation can send, which Patchwork answers itself
//! instead of passing to the agent: the ones that act on the conversation itself, and the ones
//! the buttons under a notice send. Everything else is asked for in words and done through the
//! capabilities.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use super::files::SCREENSHOTS_OFF;
use super::hub::Place;
use super::platform::Platform;
use super::types::Incoming;
use crate::Core;

/// What `/help` answers.
const HELP: &str = "Say what you want in your own words: ask what is running, start or stop a chat, change a setting or the theme, open a project or a file on the computer, and the rest. Ask what Patchwork can do here for the whole list.

/new: start this conversation over with the next message
/new <project>: talk straight to a new chat in a project
/stop: stop the reply being written
/clear: delete the messages here and start this conversation over
/screenshot: see the computer's screen, while Settings → Permissions lets the agent see it

When the agent asks before acting, reply allow, always or deny, or press a button. When it asks a question, reply with the numbers of the answers, one line per question.";

impl Core {
    /// Answers a command Patchwork handles, and says whether it was one.
    pub(super) fn gateway_command(
        self: &Arc<Self>,
        message: &Incoming,
        chat: Option<&str>,
    ) -> bool {
        let (platform, conversation) = (message.platform, message.conversation.as_str());
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
                    Some(Place::Conversation)
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
                            conversation,
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
                    Some(Place::Conversation) => None,
                    None => settings
                        .gateway
                        .project_id
                        .as_ref()
                        .and_then(|id| settings.projects.iter().find(|p| &p.id == id))
                        .map(|p| p.name.clone()),
                };
                self.bind_conversation(platform, conversation, None);
                self.gateway
                    .lock()
                    .conversations
                    .entry((platform, conversation.to_string()))
                    .or_default()
                    .project = project;
                match name {
                    Some(name) => format!("Your next message starts a new chat in {name}."),
                    None => "Your next message starts a new chat.".to_string(),
                }
            }
            "stop" => match chat {
                Some(id) if self.runner.is_running(id) => {
                    self.cancel_turn(id);
                    "Stopped.".to_string()
                }
                _ => "Nothing is running.".to_string(),
            },
            "clear" => {
                if let Some(id) = chat
                    && self.runner.is_running(id)
                {
                    self.cancel_turn(id);
                }
                self.clear_conversation(platform, conversation);
                return true;
            }
            "screenshot" => {
                if settings.see_screen {
                    self.screenshot_later(platform, conversation);
                    return true;
                }
                format!("{SCREENSHOTS_OFF}.")
            }
            "mute" => self.mute_notices(rest),
            "resume" => self.resume_at_reset(rest),
            "answer" => {
                let (id, reply) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                self.answer_by_id(platform, conversation, id, reply.trim());
                return true;
            }
            _ => return false,
        };
        self.say(platform, conversation, &reply);
        true
    }

    /// Sends a screenshot to `conversation` off the caller's thread: the desktop may ask the
    /// person to allow screenshots first, and the chat app's messages wait on this thread.
    fn screenshot_later(self: &Arc<Self>, platform: Platform, conversation: &str) {
        let (weak, to) = (Arc::downgrade(self), conversation.to_string());
        let spawned = std::thread::Builder::new()
            .name("gateway-screenshot".to_string())
            .spawn(move || {
                if let Some(core) = weak.upgrade() {
                    core.send_screenshot(platform, &to);
                }
            });
        if let Err(error) = spawned {
            log::error!(error:% = error; "could not start a screenshot: {{error}}");
            self.say(
                platform,
                conversation,
                "Patchwork could not take a screenshot.",
            );
        }
    }

    /// Starts the conversation `chat` over and deletes the messages in it that its chat app lets
    /// Patchwork delete. A long conversation takes a while, so it is done off the caller's
    /// chat.
    fn clear_conversation(self: &Arc<Self>, platform: Platform, conversation: &str) {
        let Some(connector) = self.gateway.connector(platform) else {
            return;
        };
        self.bind_conversation(platform, conversation, None);
        let (weak, conversation) = (Arc::downgrade(self), conversation.to_string());
        let spawned = std::thread::Builder::new()
            .name("gateway-clear".to_string())
            .spawn(move || {
                let reply = match connector.clear(&conversation) {
                    Ok(true) => "Cleared. Your next message starts a new chat.".to_string(),
                    Ok(false) => format!(
                        "Cleared Patchwork's messages. {} does not let it delete yours here. Your next message starts a new chat.",
                        platform.label()
                    ),
                    Err(error) => format!(
                        "Patchwork could not delete the messages here: {error}. Your next message starts a new chat."
                    ),
                };
                if let Some(core) = weak.upgrade() {
                    core.say(platform, &conversation, &reply);
                }
            });
        if let Err(error) = spawned {
            log::error!(error:% = error; "could not start clearing a conversation: {{error}}");
        }
    }
}
