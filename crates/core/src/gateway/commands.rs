//! The slash commands a chat app conversation can send, which Patchwork answers itself instead
//! of passing to the agent.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::sync::Arc;

use super::hub::Place;
use super::types::Incoming;
use crate::Core;
use crate::system::now_ms;
use crate::threads::{Role, Standing};

/// How many threads `/threads` lists.
const LISTED: usize = 10;

/// What `/help` answers.
const HELP: &str = "Messages go to the agent in this conversation's Patchwork chat.

/new: start a new chat with the next message
/new <project>: start it in a project
/projects: list the projects
/threads: list recent chats
/use <number>: switch to a chat from /threads
/stop: stop the agent
/status: show the chat and what it is doing
/running: list every chat that is working
/diff: get what the last reply changed as a file
/model, /agent, /access, /plan: show or change what this chat runs with
/unmute: hear from every muted chat again

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
                            &format!("No project is named {rest}. /projects lists them."),
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
            "projects" => {
                if settings.projects.is_empty() {
                    "Patchwork has no projects yet.".to_string()
                } else {
                    let names: Vec<_> = settings
                        .projects
                        .iter()
                        .map(|p| format!("• {}", p.name))
                        .collect();
                    format!(
                        "{}\n\nStart a chat in one with /new <project>.",
                        names.join("\n")
                    )
                }
            }
            "threads" | "chats" => {
                let mut threads: Vec<_> = self
                    .list_threads()
                    .into_iter()
                    .filter(|t| t.archived_at.is_none() && t.parent.is_none())
                    .collect();
                threads.sort_by_key(|t| std::cmp::Reverse(t.created_at));
                threads.truncate(LISTED);
                let lines: Vec<_> = threads
                    .iter()
                    .enumerate()
                    .map(|(i, t)| format!("{}. {}", i + 1, t.title))
                    .collect();
                self.gateway
                    .lock()
                    .chats
                    .entry((platform, chat.to_string()))
                    .or_default()
                    .listed = threads.iter().map(|t| t.id.clone()).collect();
                if lines.is_empty() {
                    "Patchwork has no chats yet.".to_string()
                } else {
                    format!("{}\n\nSwitch with /use <number>.", lines.join("\n"))
                }
            }
            "use" => {
                let picked = rest.parse::<usize>().ok().and_then(|n| {
                    self.gateway
                        .lock()
                        .chats
                        .get(&(platform, chat.to_string()))
                        .and_then(|c| c.listed.get(n.checked_sub(1)?).cloned())
                });
                match picked.and_then(|id| self.store.thread(&id)) {
                    Some(t) => {
                        self.bind_chat(platform, chat, Some(&t.id));
                        format!("Now talking to {}.", t.title)
                    }
                    None => "Send /threads, then /use with a number from the list.".to_string(),
                }
            }
            "stop" => match thread {
                Some(id) if self.runner.is_running(id) => {
                    self.cancel_turn(id);
                    "Stopped.".to_string()
                }
                _ => "Nothing is running.".to_string(),
            },
            "status" => match thread.and_then(|id| self.store.thread(id)) {
                Some(t) => {
                    let doing = if self.runner.is_running(&t.id) {
                        "working"
                    } else {
                        "idle"
                    };
                    format!("{}: {doing}", t.title)
                }
                None => "No chat yet. Your next message starts one.".to_string(),
            },
            "running" => self.running(),
            "diff" => match self.send_diff(platform, chat, thread) {
                Some(text) => text,
                None => return true,
            },
            "mute" => self.mute_notices(rest),
            "unmute" => self.unmute_notices(),
            "resume" => self.resume_at_reset(rest),
            "model" | "agent" | "plan" | "access" => self.thread_option(&word, rest, thread),
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

    /// The chats people started that are replying now: each one's title, project, what it is
    /// doing and for how long.
    fn running(&self) -> String {
        let projects = self.store.settings().projects;
        let now = now_ms();
        let lines: Vec<String> = self
            .store
            .threads()
            .into_iter()
            .filter(|t| {
                t.parent.is_none()
                    && (self.runner.is_running(&t.id) || self.runner.is_starting(&t.id))
            })
            .map(|t| {
                let mut line = format!("• {}", t.title);
                if let Some(p) = t
                    .project_id
                    .as_ref()
                    .and_then(|id| projects.iter().find(|p| &p.id == id))
                {
                    let _ = write!(line, " in {}", p.name);
                }
                let doing = if t.standing() == Standing::Waiting {
                    "waiting for an answer".to_string()
                } else {
                    t.doing()
                };
                let _ = write!(line, ": {doing}");
                if let Some(reply) = t.messages.iter().rfind(|m| m.role == Role::Assistant) {
                    let minutes = now.saturating_sub(reply.created_at) / 60_000;
                    let _ = match minutes {
                        0 => write!(line, ", under a minute"),
                        1..60 => write!(line, ", {minutes} min"),
                        _ => write!(line, ", {} h {} min", minutes / 60, minutes % 60),
                    };
                }
                line
            })
            .collect();
        if lines.is_empty() {
            "Nothing is running.".to_string()
        } else {
            lines.join("\n")
        }
    }
}
