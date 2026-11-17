//! What Patchwork does with a message from a chat app: pairing a stranger, answering the
//! agent's request, or sending it on to the chat's thread.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::hub::{Place, Seen};
use super::outbound::savable;
use super::platform::Platform;
use super::types::Incoming;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::permissions::{Effect, PermissionRule, PermissionScope, RuleRef};
use crate::provider::AgentKind;
use crate::system::new_id;
use crate::threads::{Approval, ApprovalStatus, NewThread, Part, Question, Role, Thread};
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
    /// summary it goes on from can leave it out.
    pub(crate) fn chat_app_voice(&self, thread_id: &str, text: String, fresh: bool) -> String {
        let (plain, compacted) = self.store.with_threads(|threads| {
            threads
                .iter()
                .find(|t| t.id == thread_id)
                .map_or((false, false), |t| (t.role.is_none(), compacted(t)))
        });
        if !(fresh || compacted) || !plain || self.bound_chats(thread_id).is_empty() {
            return text;
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

    /// Handles a message from a chat app, on the thread of its connection.
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
        let (platform, chat) = (message.platform, message.chat.as_str());
        self.note_chat(platform, &message.sender, chat);
        let thread = self
            .bound_thread(platform, chat)
            .filter(|id| self.store.thread(id).is_some());
        let text = message.text.trim();
        let command = text.starts_with('/');
        if !command
            && let Some(id) = &thread
            && let Some(approval) = self.waiting_approval(id)
        {
            self.answer(platform, chat, id, &approval, text);
            return;
        }
        if !command && self.answer_notice(platform, chat, text) {
            return;
        }
        if command && self.gateway_command(message, thread.as_deref()) {
            return;
        }
        self.pass_on(message, thread);
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
                && seen.chat == message.chat
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
                chat: message.chat.clone(),
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
        let (platform, chat) = (message.platform, message.chat.as_str());
        if self.take_gateway_code(&message.text) {
            match self.add_pairing(platform, &message.sender, &message.name, chat) {
                Ok(()) => self.say(
                    platform,
                    chat,
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
                chat,
                "To use Patchwork here, make a pairing code in Settings → Gateway on your computer and send it here.",
            );
        }
    }

    /// The request the agent in `thread_id` is waiting on the user to answer.
    pub(super) fn waiting_approval(&self, thread_id: &str) -> Option<Approval> {
        if !self.runner.is_running(thread_id) {
            return None;
        }
        self.store
            .thread(thread_id)?
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
        chat: &str,
        thread_id: &str,
        approval: &Approval,
        text: &str,
    ) {
        if approval.secret.is_some() {
            self.say(
                platform,
                chat,
                "Enter the secret in Patchwork on your computer. Chat apps never carry it.",
            );
            return;
        }
        let word = text.to_lowercase();
        if word == ALWAYS && savable(approval) {
            self.allow_always(platform, chat, thread_id, approval);
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
            self.say(platform, chat, "Reply allow or deny.");
            return;
        };
        self.gateway
            .lock()
            .answered
            .insert(approval.id.clone(), (platform, chat.to_string()));
        if let Err(error) = self.respond_approval(thread_id, &approval.id, allow, answers, None) {
            self.gateway.lock().answered.remove(&approval.id);
            self.say(
                platform,
                chat,
                &format!("Patchwork could not answer: {error}"),
            );
        }
    }

    /// Allows the agent's request and saves rules that allow requests like it from now on, in
    /// the thread's project or, outside one, in every chat.
    fn allow_always(
        self: &Arc<Self>,
        platform: Platform,
        chat: &str,
        thread_id: &str,
        approval: &Approval,
    ) {
        let project = self.store.thread(thread_id).and_then(|t| t.project_id);
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
            .insert(approval.id.clone(), (platform, chat.to_string()));
        if let Err(error) =
            self.respond_approval(thread_id, &approval.id, true, HashMap::new(), rule)
        {
            self.gateway.lock().answered.remove(&approval.id);
            self.say(
                platform,
                chat,
                &format!("Patchwork could not answer: {error}"),
            );
            return;
        }
        // Saved after the answer, so the rules answer the other requests waiting, not this one.
        let saved = match self.save_permission_rules(&scope, rules) {
            Ok(()) => format!("Patchwork will allow this in {name} from now on."),
            Err(error) => format!("Allowed this once. Patchwork could not save the rule: {error}"),
        };
        self.say(platform, chat, &saved);
    }

    /// Sends the message to `thread`, or to a new thread when the chat has none.
    fn pass_on(self: &Arc<Self>, message: &Incoming, thread: Option<String>) {
        let (platform, chat) = (message.platform, message.chat.as_str());
        let attachments: Vec<_> = message
            .files
            .iter()
            .filter_map(|bytes| self.attach_image(bytes).ok())
            .collect();
        let text = message.text.trim().to_string();
        if text.is_empty() && attachments.is_empty() {
            return;
        }
        let (id, new) = match thread {
            Some(id) => (id, false),
            None => match self.open_gateway_thread(platform, chat) {
                Ok(id) => (id, true),
                Err(error) => {
                    self.say(
                        platform,
                        chat,
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
                .thread(&id)
                .is_some_and(|t| t.access_mode != access)
        {
            match self.set_thread_access_mode(&id, access) {
                Ok(()) => self.emit(AgentEventEnvelope {
                    thread_id: id.clone(),
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
        match self.send_message_from(id.clone(), text, attachments, queue, Some((platform, chat))) {
            Ok(_) => {
                if let Some(connector) = self.gateway.connector(platform) {
                    connector.typing(chat);
                }
            }
            Err(error) => {
                // A chat that could not take its first message leaves nothing behind.
                if new {
                    self.delete_thread(&id);
                    self.unbind_thread(&id);
                    self.emit(AgentEventEnvelope {
                        thread_id: id,
                        event: AgentEvent::ThreadDeleted,
                    });
                }
                self.say(
                    platform,
                    chat,
                    &format!("Patchwork could not send that: {error}"),
                );
            }
        }
    }

    /// Starts a thread for a chat app conversation and binds the conversation to it.
    ///
    /// Without a place picked by `/new`, the thread is a dispatcher: it has no project and its
    /// agent drives Patchwork's chats without reaching the computer. Only Claude and Patchwork's
    /// own agent can be launched that way; with neither set up, and after `/new <project>` or
    /// `/new chat`, the thread is a working chat with the gateway's access.
    fn open_gateway_thread(
        self: &Arc<Self>,
        platform: Platform,
        chat: &str,
    ) -> Result<String, Error> {
        let settings = self.store.settings();
        let chosen = self
            .gateway
            .lock()
            .chats
            .get_mut(&(platform, chat.to_string()))
            .and_then(|c| c.project.take());
        let default = settings.default_agent();
        let dispatcher = if chosen.is_some() {
            None
        } else {
            [default, AgentKind::Claude, AgentKind::Api]
                .into_iter()
                .filter(|agent| matches!(agent, AgentKind::Claude | AgentKind::Api))
                .find(|agent| *agent == default || settings.agents.contains(agent))
        };
        let project_id = match chosen {
            Some(Place::Chat) => None,
            Some(Place::Project(id)) => Some(id),
            None if dispatcher.is_some() => None,
            None => settings.gateway.project_id.clone(),
        }
        .filter(|id| settings.projects.iter().any(|p| &p.id == id));
        let agent = dispatcher.unwrap_or(default);
        let thread = self.create_thread(NewThread {
            id: None,
            project_id,
            agent,
            model: settings.tokens.default_models.get(&agent).cloned(),
            access_mode: settings.gateway.access,
            effort: None,
            compact_window: None,
            plan_mode: false,
        })?;
        let thread = self.store.thread(&thread.id).unwrap_or(thread);
        let id = thread.id.clone();
        if dispatcher.is_some()
            && let Err(error) = self.mark_dispatcher(&id)
        {
            self.delete_thread(&id);
            return Err(error);
        }
        self.emit(AgentEventEnvelope {
            thread_id: id.clone(),
            event: AgentEvent::ThreadCreated { thread },
        });
        self.bind_chat(platform, chat, Some(&id));
        Ok(id)
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

/// Whether the agent of `thread` compacted its context on its own in the replies to the message
/// before the newest, so the summary it now goes on from may have left out how to talk.
fn compacted(thread: &Thread) -> bool {
    thread
        .messages
        .iter()
        .rev()
        .skip_while(|m| m.role == Role::User)
        .take_while(|m| m.role != Role::User)
        .any(|m| m.parts.iter().any(|p| matches!(p, Part::Compaction { .. })))
}
