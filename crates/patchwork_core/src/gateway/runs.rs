//! The chats a chat app runs: opening one for an agent, and how its agent is told to talk.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use super::types::Place;
use crate::agents::Agent;
use crate::chats::{Chat, NewChat, Part, Role};
use crate::provider::AgentKind;
use crate::{Core, Error};

/// What the agent of a chat run from a chat app is told about talking over one.
const CHAT_APP: &str = "You are talking with the person over a chat app while they are away from \
Patchwork. Lead with the answer, and keep a reply to about 250 characters: one or two sentences. \
Go longer only when leaving something out would mislead them or stop them from deciding, such as \
an error's text, a question they must answer or the items they asked to have listed. Never \
recap what they said, list what you could do next or explain how you found the answer.
When they ask what is running, what the tasks are or what is going on in a project, they mean \
Patchwork's chats, the tasks its agents are working on. Answer from the list_chats and read_chat \
tools, with each chat's name and what it is doing. Do not look with shell commands, processes, \
git or log files, and do not ask them to approve any.
When they ask how something is going, report it the way a coworker gives a standup update: what \
what it is doing now and anything stuck or waiting on them, with real names and numbers. What \
is done and what is left come only when they ask for more. Say it plainly when something is broken or \
you do not know.
Each message ends with a patchwork-now note: the chats as they stand and the one you last \
worked with. Resolve what they mean from it before asking them anything.
Chat apps show little Markdown, so keep formatting light: short paragraphs, a few bullets at \
most, no tables or headings.";

/// How it talks when no personality is chosen.
const VOICE: &str = "Talk like a teammate in a team chat: plain words, short sentences, first \
person, a bit of dry humor when it fits, and no filler, sign-offs or corporate tone.";

impl Core {
    /// `text` with how to talk over a chat app after it, in the chosen personality, for a chat run
    /// from a chat app that has no custom agent's role. It goes with the message that starts the
    /// agent's session, and again after the agent compacted its context on its own, since the
    /// summary it goes on from can leave it out. An `agent` that takes no persona gets no
    /// personality.
    pub fn chat_app_voice(
        &self,
        chat_id: &str,
        text: String,
        fresh: bool,
        agent: AgentKind,
    ) -> String {
        let (plain, compacted) = self.store.with_chats(|chats| {
            chats
                .iter()
                .find(|t| t.id == chat_id)
                .map_or((false, false), |t| (t.role.is_none(), compacted(t)))
        });
        if !(fresh || compacted) || !plain || self.bound_conversations(chat_id).is_empty() {
            return text;
        }
        if !agent.takes_persona() {
            return format!("{text}\n\n<chat-app>\n{CHAT_APP}\n</chat-app>");
        }
        let voice = self
            .store
            .settings()
            .gateway
            .personality
            .and_then(|name| self.personality(&name))
            .unwrap_or_else(|| VOICE.to_string());
        format!(
            "{text}\n\n<chat-app>\n{CHAT_APP}\n</chat-app>\n<personality>\n{voice}\n</personality>"
        )
    }

    /// Starts a run of chat app agent `profile` in `chosen`, the place picked by `/new`, as its chat when
    /// `main`.
    ///
    /// Without a place, the run is a dispatcher: it has no project and its agent drives
    /// Patchwork's chats without reaching the computer. Only Claude and Patchwork's own agent
    /// can be launched that way; with neither set up, and in a picked place, the run is a
    /// working chat with the gateway's access.
    ///
    /// # Errors
    ///
    /// Returns an error when the run's chat cannot be made.
    pub fn open_chat_app_run(
        self: &Arc<Self>,
        profile: &Agent,
        chosen: Option<Place>,
        main: bool,
    ) -> Result<Chat, Error> {
        let settings = self.store.settings();
        let dispatcher = if chosen.is_some() {
            None
        } else {
            [profile.agent, AgentKind::Claude, AgentKind::Api]
                .into_iter()
                .filter(|agent| matches!(agent, AgentKind::Claude | AgentKind::Api))
                .find(|agent| *agent == profile.agent || settings.agents.contains(agent))
        };
        let project_id = match chosen {
            Some(Place::Conversation) => None,
            Some(Place::Project(id)) => Some(id),
            None if dispatcher.is_some() => None,
            None => settings.gateway.project_id.clone(),
        }
        .filter(|id| settings.projects.iter().any(|p| &p.id == id));
        let agent = dispatcher.unwrap_or(profile.agent);
        let model = if agent == profile.agent {
            profile.model.clone()
        } else {
            settings.tokens.default_models.get(&agent).cloned()
        };
        let new = NewChat {
            id: None,
            project_id,
            agent,
            model,
            access_mode: settings.gateway.access,
            effort: (agent == profile.agent)
                .then(|| profile.effort.clone())
                .flatten(),
            compact_window: None,
        };
        // Linked before the first turn, which reads it for the agent's instructions, and
        // recorded before it is announced, so it is never listed as a run.
        self.chats().create_linked(new, |chat| {
            let id = &chat.id;
            self.store
                .update_chat(id, |t| t.agent_id = Some(profile.id.clone()))?;
            if dispatcher.is_some() {
                self.mark_dispatcher(id)?;
            }
            if main {
                self.set_agent_chat(&profile.id, id)?;
            }
            Ok(())
        })
    }
}

/// Whether the agent of `chat` compacted its context on its own in the replies to the message
/// before the newest, so the summary it now goes on from may have left out how to talk.
fn compacted(chat: &Chat) -> bool {
    chat.messages
        .iter()
        .rev()
        .skip_while(|m| m.role == Role::User)
        .take_while(|m| m.role != Role::User)
        .any(|m| m.parts.iter().any(|p| matches!(p, Part::Compaction { .. })))
}
