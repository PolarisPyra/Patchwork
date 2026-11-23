//! What Patchwork does with a message from a chat app: pairing a stranger, answering the
//! agent's request, or sending it on to the chat's chat.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::hub::{Place, Seen};
use super::outbound::savable;
use super::platform::Platform;
use super::types::Incoming;
use crate::agents::Agent;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::permissions::{Effect, PermissionRule, PermissionScope, RuleRef};
use crate::provider::AgentKind;
use crate::system::new_id;
use crate::chats::{Approval, ApprovalStatus, NewChat, Part, Question, Role, Chat};
use crate::{Error, Core};

/// How many taken messages the gateway remembers, to know one that arrives again.
const REMEMBERED: usize = 512;

/// How long the same words from the same sender count as one message, on a platform that gives
/// its messages no id.
const SAME_WORDS: Duration = Duration::from_secs(3);

/// Words that allow what the agent asks.
const ALLOW: &[&str] = &["allow", "yes", "y", "ok", "approve"];

/// The word that allows what the agent asks, and requests like it from now on.
const ALWAYS: &str = "always";

/// Words that turn down what the agent asks.
pub(super) const DENY: &[&str] = &["deny", "no", "n", "reject", "cancel"];

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
    pub(crate) fn chat_app_voice(
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

    /// Handles a message from a chat app, on the chat of its connection.
    pub(super) fn gateway_incoming(self: &Arc<Self>, message: &Incoming) {
        if self.repeated(message) {
            log::info!(
                platform:? = message.platform, id:? = message.id;
                "dropped a chat app message that arrived twice"
            );
            return;
        }
        if !self.is_paired(message.platform, &message.sender) {
            self.pair(message);
            return;
        }
        let (platform, conversation) = (message.platform, message.conversation.as_str());
        self.note_conversation(platform, &message.sender, conversation);
        // An archived chat lets go of the conversation, which then goes to the platform's agent's
        // chat, where it can be seen.
        let chat = self
            .bound_chat(platform, conversation)
            .filter(|id| self.store.chat(id).is_some_and(|t| t.archived_at.is_none()));
        let text = message.text.trim();
        let command = text.starts_with('/');
        if !command
            && let Some(id) = &chat
            && let Some(approval) = self.waiting_approval(id)
        {
            self.answer(platform, conversation, id, &approval, text);
            return;
        }
        if !command && self.answer_notice(platform, conversation, text) {
            return;
        }
        if command && self.gateway_command(message, chat.as_deref()) {
            return;
        }
        self.pass_on(message, chat);
    }

    /// Whether the gateway took this message already, and notes it when it did not.
    ///
    /// A chat app sends a message again when its connection drops before Patchwork's answer
    /// reaches it, and acting on both would run what the person asked for twice.
    fn repeated(&self, message: &Incoming) -> bool {
        let now = Instant::now();
        let (key, by_id) = match &message.id {
            Some(id) => (id.clone(), true),
            None => (format!("{}\n{}", message.sender, message.text), false),
        };
        let mut state = self.gateway.lock();
        let again = state.seen.iter().any(|seen| {
            seen.platform == message.platform
                && seen.conversation == message.conversation
                && seen.by_id == by_id
                && seen.key == key
                && (by_id || now.duration_since(seen.at) < SAME_WORDS)
        });
        if !again {
            if state.seen.len() >= REMEMBERED {
                state.seen.pop_front();
            }
            state.seen.push_back(Seen {
                platform: message.platform,
                conversation: message.conversation.clone(),
                key,
                by_id,
                at: now,
            });
        }
        again
    }

    /// Pairs the sender when the message is the pairing code, and otherwise tells them once how
    /// to pair.
    fn pair(&self, message: &Incoming) {
        let (platform, conversation) = (message.platform, message.conversation.as_str());
        if self.take_gateway_code(&message.text) {
            match self.add_pairing(platform, &message.sender, &message.name, conversation) {
                Ok(()) => self.say(
                    platform,
                    conversation,
                    "Paired. Send a message to start a chat in Patchwork, or /help for what else you can do.",
                ),
                Err(error) => {
                    log::warn!(error:% = error; "could not save a pairing: {{error}}");
                }
            }
            return;
        }
        let first = self
            .gateway
            .lock()
            .told
            .insert((platform, message.sender.clone()));
        if first {
            self.say(
                platform,
                conversation,
                "To use Patchwork here, make a pairing code in Settings → Gateway on your computer and send it here.",
            );
        }
    }

    /// The request the agent in `chat_id` is waiting on the user to answer.
    pub(super) fn waiting_approval(&self, chat_id: &str) -> Option<Approval> {
        if !self.runner.is_running(chat_id) {
            return None;
        }
        self.store
            .chat(chat_id)?
            .messages
            .iter()
            .rev()
            .flat_map(|m| m.approvals.iter())
            .find(|a| a.status == ApprovalStatus::Pending && !a.auto)
            .cloned()
    }

    /// Answers the agent's request with the user's reply.
    pub(super) fn answer(
        self: &Arc<Self>,
        platform: Platform,
        conversation: &str,
        chat_id: &str,
        approval: &Approval,
        text: &str,
    ) {
        if approval.secret.is_some() {
            self.say(
                platform,
                conversation,
                "Enter the secret in Patchwork on your computer. Chat apps never carry it.",
            );
            return;
        }
        let word = text.to_lowercase();
        if word == ALWAYS && savable(approval) {
            self.allow_always(platform, conversation, chat_id, approval);
            return;
        }
        let decision = if DENY.contains(&word.as_str()) {
            Some((false, HashMap::new()))
        } else if approval.questions.is_empty() {
            ALLOW
                .contains(&word.as_str())
                .then(|| (true, HashMap::new()))
        } else {
            let answers = answers(&approval.questions, text);
            (!answers.is_empty()).then_some((true, answers))
        };
        let Some((allow, answers)) = decision else {
            self.say(platform, conversation, "Reply allow or deny.");
            return;
        };
        self.gateway
            .lock()
            .answered
            .insert(approval.id.clone(), (platform, conversation.to_string()));
        if let Err(error) = self.respond_approval(chat_id, &approval.id, allow, answers, None) {
            self.gateway.lock().answered.remove(&approval.id);
            self.say(
                platform,
                conversation,
                &format!("Patchwork could not answer: {error}"),
            );
        }
    }

    /// Allows the agent's request and saves rules that allow requests like it from now on, in
    /// the chat's project or, outside one, in every chat.
    fn allow_always(
        self: &Arc<Self>,
        platform: Platform,
        conversation: &str,
        chat_id: &str,
        approval: &Approval,
    ) {
        let project = self.store.chat(chat_id).and_then(|t| t.project_id);
        let name = project.as_ref().map_or_else(
            || "Chats".to_string(),
            |id| {
                self.store
                    .settings()
                    .projects
                    .into_iter()
                    .find(|p| &p.id == id)
                    .map_or_else(|| "this project".to_string(), |p| p.name)
            },
        );
        let scope = project.map_or(PermissionScope::Chats, PermissionScope::Project);
        let rules: Vec<PermissionRule> = approval
            .kind
            .iter()
            .flat_map(|kind| {
                approval.suggestions.iter().map(|pattern| PermissionRule {
                    id: new_id(),
                    kind: *kind,
                    pattern: pattern.clone(),
                    effect: Effect::Allow,
                })
            })
            .collect();
        let rule = rules.first().map(|r| RuleRef {
            scope: scope.clone(),
            rule_id: Some(r.id.clone()),
        });
        self.gateway
            .lock()
            .answered
            .insert(approval.id.clone(), (platform, conversation.to_string()));
        if let Err(error) = self.respond_approval(chat_id, &approval.id, true, HashMap::new(), rule)
        {
            self.gateway.lock().answered.remove(&approval.id);
            self.say(
                platform,
                conversation,
                &format!("Patchwork could not answer: {error}"),
            );
            return;
        }
        // Saved after the answer, so the rules answer the other requests waiting, not this one.
        let saved = match self.save_permission_rules(&scope, rules) {
            Ok(()) => format!("Patchwork will allow this in {name} from now on."),
            Err(error) => format!("Allowed this once. Patchwork could not save the rule: {error}"),
        };
        self.say(platform, conversation, &saved);
    }

    /// Sends the message to `chat`, or to a new chat when the chat has none.
    fn pass_on(self: &Arc<Self>, message: &Incoming, chat: Option<String>) {
        let (platform, conversation) = (message.platform, message.conversation.as_str());
        let attachments: Vec<_> = message
            .files
            .iter()
            .filter_map(|bytes| self.attach_image(bytes).ok())
            .collect();
        let text = message.text.trim().to_string();
        if text.is_empty() && attachments.is_empty() {
            return;
        }
        let (id, new) = match chat {
            Some(id) => (id, false),
            None => match self.open_gateway_chat(platform, conversation) {
                Ok(opened) => opened,
                Err(error) => {
                    self.say(
                        platform,
                        conversation,
                        &format!("Patchwork could not start a chat: {error}"),
                    );
                    return;
                }
            },
        };
        // A dispatcher's access is the one in Settings, so changing it there or from a chat
        // app reaches the conversations that already have one.
        let access = self.store.settings().gateway.access;
        if !new
            && self.is_dispatcher(&id)
            && self
                .store
                .chat(&id)
                .is_some_and(|t| t.access_mode != access)
        {
            match self.set_chat_access_mode(&id, access) {
                Ok(()) => self.emit(AgentEventEnvelope {
                    chat_id: id.clone(),
                    event: AgentEvent::OptionsChanged,
                }),
                Err(error) => {
                    log::warn!(error:% = error; "could not set a chat's access: {{error}}");
                }
            }
        }
        // A dispatcher takes words, not commands: its agent would answer a leading slash itself,
        // as a command it does not have.
        let words = text.trim_start_matches('/');
        let text = if self.is_dispatcher(&id) && !words.is_empty() {
            words.to_string()
        } else {
            text
        };
        let queue = self.runner.is_running(&id) && !self.store.settings().steer_while_working;
        match self.send_message_from(
            id.clone(),
            text,
            attachments,
            queue,
            Some((platform, conversation)),
        ) {
            Ok(_) => {
                if let Some(connector) = self.gateway.connector(platform) {
                    connector.typing(conversation);
                }
            }
            Err(error) => {
                // A chat that could not take its first message leaves nothing behind.
                if new {
                    self.delete_chat(&id);
                    self.unbind_chat(&id);
                    self.emit(AgentEventEnvelope {
                        chat_id: id,
                        event: AgentEvent::ChatDeleted,
                    });
                }
                self.say(
                    platform,
                    conversation,
                    &format!("Patchwork could not send that: {error}"),
                );
            }
        }
    }

    /// Binds a chat app conversation to the platform's agent's chat, and returns the chat's id
    /// and whether it was made for this message. The agent is made the first time its platform
    /// sends a message, and its chat with the first conversation; `/new` starts a run of its own.
    fn open_gateway_chat(
        self: &Arc<Self>,
        platform: Platform,
        conversation: &str,
    ) -> Result<(String, bool), Error> {
        let profile = self.chat_app_agent(platform)?;
        let chosen = self
            .gateway
            .lock()
            .conversations
            .get_mut(&(platform, conversation.to_string()))
            .and_then(|c| c.project.take());
        if chosen.is_none()
            && let Some(id) = self.agent_chat(&profile)
        {
            self.bind_conversation(platform, conversation, Some(&id));
            return Ok((id, false));
        }
        let main = chosen.is_none();
        let id = self.open_chat_app_run(&profile, chosen, main)?.id;
        self.bind_conversation(platform, conversation, Some(&id));
        Ok((id, true))
    }

    /// Starts a run of chat app agent `profile` in `chosen`, the place picked by `/new`, as its chat when
    /// `main`.
    ///
    /// Without a place, the run is a dispatcher: it has no project and its agent drives
    /// Patchwork's chats without reaching the computer. Only Claude and Patchwork's own agent
    /// can be launched that way; with neither set up, and in a picked place, the run is a
    /// working chat with the gateway's access.
    pub(crate) fn open_chat_app_run(
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
        let chat = self.create_chat(NewChat {
            id: None,
            project_id,
            agent,
            model,
            access_mode: settings.gateway.access,
            effort: (agent == profile.agent)
                .then(|| profile.effort.clone())
                .flatten(),
            compact_window: None,
        })?;
        let id = chat.id.clone();
        // Linked before the first turn, which reads it for the agent's instructions.
        let linked = self
            .store
            .update_chat(&id, |t| t.agent_id = Some(profile.id.clone()));
        let marked = match linked {
            Ok(()) if dispatcher.is_some() => self.mark_dispatcher(&id),
            done => done,
        }
        // Recorded before it is announced, so it is never listed as a run.
        .and_then(|()| {
            if main {
                self.set_agent_chat(&profile.id, &id)
            } else {
                Ok(())
            }
        });
        if let Err(error) = marked {
            self.delete_chat(&id);
            return Err(error);
        }
        let chat = self.store.chat(&id).unwrap_or(chat);
        self.emit(AgentEventEnvelope {
            chat_id: id,
            event: AgentEvent::ChatCreated { chat: chat.clone() },
        });
        Ok(chat)
    }
}

/// The answers in `text` to `questions`: one line per question, each the numbers of the options
/// picked or the answer in words.
pub(super) fn answers(questions: &[Question], text: &str) -> HashMap<String, Vec<String>> {
    let parts: Vec<&str> = if questions.len() == 1 {
        vec![text.trim()]
    } else {
        text.split(['\n', ';'])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect()
    };
    questions
        .iter()
        .zip(parts)
        .map(|(question, part)| {
            let picked: Option<Vec<String>> = part
                .split([',', ' '])
                .filter(|n| !n.is_empty())
                .map(|n| {
                    n.parse::<usize>()
                        .ok()
                        .and_then(|n| n.checked_sub(1))
                        .and_then(|i| question.options.get(i))
                        .map(|o| o.label.clone())
                })
                .collect();
            let answer = match picked {
                Some(labels) if !labels.is_empty() => labels,
                _ => vec![part.to_string()],
            };
            (question.id.clone(), answer)
        })
        .collect()
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
