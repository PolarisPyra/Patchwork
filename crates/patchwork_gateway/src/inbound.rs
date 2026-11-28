//! What Patchwork does with a message from a chat app: pairing a stranger, answering the
//! agent's request, or sending it on to the chat's chat.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::hub::Seen;
use super::outbound::savable;
use super::platform::Platform;
use super::types::Incoming;
use patchwork_core::permissions::{Effect, PermissionRule, PermissionScope, RuleRef};
use patchwork_platform::new_id;
use patchwork_core::chats::{Approval, ApprovalStatus, Question};
use patchwork_core::{Error, Core};
#[allow(
    unused_imports,
    reason = "each file takes the core's gateway methods it calls"
)]
use crate::prelude::*;
use crate::hub::HUB;

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

/// The core's inbound methods the gateway calls.
pub(crate) trait Inbound {
    /// Handles a message from a chat app, on the chat of its connection.
    fn gateway_incoming(self: &Arc<Self>, message: &Incoming);

    /// Whether the gateway took this message already, and notes it when it did not.
    ///
    /// A chat app sends a message again when its connection drops before Patchwork's answer
    /// reaches it, and acting on both would run what the person asked for twice.
    fn repeated(&self, message: &Incoming) -> bool;

    /// Pairs the sender when the message is the pairing code, and otherwise tells them once how
    /// to pair.
    fn pair(&self, message: &Incoming);

    /// The request the agent in `chat_id` is waiting on the user to answer.
    fn waiting_approval(&self, chat_id: &str) -> Option<Approval>;

    /// Answers the agent's request with the user's reply.
    fn answer(
        self: &Arc<Self>,
        platform: Platform,
        conversation: &str,
        chat_id: &str,
        approval: &Approval,
        text: &str,
    );

    /// Allows the agent's request and saves rules that allow requests like it from now on, in
    /// the chat's project or, outside one, in every chat.
    fn allow_always(
        self: &Arc<Self>,
        platform: Platform,
        conversation: &str,
        chat_id: &str,
        approval: &Approval,
    );

    /// Sends the message to `chat`, or to a new chat when the chat has none.
    fn pass_on(self: &Arc<Self>, message: &Incoming, chat: Option<String>);

    /// Binds a chat app conversation to the platform's agent's chat, and returns the chat's id
    /// and whether it was made for this message. The agent is made the first time its platform
    /// sends a message, and its chat with the first conversation; `/new` starts a run of its own.
    fn open_gateway_chat(
        self: &Arc<Self>,
        platform: Platform,
        conversation: &str,
    ) -> Result<(String, bool), Error>;
}

impl Inbound for Core {
    fn gateway_incoming(self: &Arc<Self>, message: &Incoming) {
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
        let chat = self.bound_chat(platform, conversation).filter(|id| {
            self.store()
                .chat(id)
                .is_some_and(|t| t.archived_at.is_none())
        });
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

    fn repeated(&self, message: &Incoming) -> bool {
        let now = Instant::now();
        let (key, by_id) = match &message.id {
            Some(id) => (id.clone(), true),
            None => (format!("{}\n{}", message.sender, message.text), false),
        };
        let mut state = HUB.lock();
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
        let first = HUB.lock().told.insert((platform, message.sender.clone()));
        if first {
            self.say(
                platform,
                conversation,
                "To use Patchwork here, make a pairing code in Settings → Gateway on your computer and send it here.",
            );
        }
    }

    fn waiting_approval(&self, chat_id: &str) -> Option<Approval> {
        if !self.turn_running(chat_id) {
            return None;
        }
        self.store()
            .chat(chat_id)?
            .messages
            .iter()
            .rev()
            .flat_map(|m| m.approvals.iter())
            .find(|a| a.status == ApprovalStatus::Pending && !a.auto)
            .cloned()
    }

    fn answer(
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
        HUB.lock()
            .answered
            .insert(approval.id.clone(), (platform, conversation.to_string()));
        if let Err(error) = self.respond_approval(chat_id, &approval.id, allow, answers, None) {
            HUB.lock().answered.remove(&approval.id);
            self.say(
                platform,
                conversation,
                &format!("Patchwork could not answer: {error}"),
            );
        }
    }

    fn allow_always(
        self: &Arc<Self>,
        platform: Platform,
        conversation: &str,
        chat_id: &str,
        approval: &Approval,
    ) {
        let project = self.store().chat(chat_id).and_then(|t| t.project_id);
        let name = project.as_ref().map_or_else(
            || "Chats".to_string(),
            |id| {
                self.store()
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
        HUB.lock()
            .answered
            .insert(approval.id.clone(), (platform, conversation.to_string()));
        if let Err(error) = self.respond_approval(chat_id, &approval.id, true, HashMap::new(), rule)
        {
            HUB.lock().answered.remove(&approval.id);
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
        let access = self.store().settings().gateway.access;
        if !new
            && self.is_dispatcher(&id)
            && self
                .store()
                .chat(&id)
                .is_some_and(|t| t.access_mode != access)
            && let Err(error) = self.chats().set_access_mode(&id, access)
        {
            log::warn!(error:% = error; "could not set a chat's access: {{error}}");
        }
        // A dispatcher takes words, not commands: its agent would answer a leading slash itself,
        // as a command it does not have.
        let words = text.trim_start_matches('/');
        let text = if self.is_dispatcher(&id) && !words.is_empty() {
            words.to_string()
        } else {
            text
        };
        let queue = self.turn_running(&id) && !self.store().settings().steer_while_working;
        match self.send_message_from(
            id.clone(),
            text,
            attachments,
            queue,
            Some((platform, conversation)),
        ) {
            Ok(_) => {
                if let Some(connector) = HUB.connector(platform) {
                    connector.typing(conversation);
                }
            }
            Err(error) => {
                // A chat that could not take its first message leaves nothing behind.
                if new {
                    self.unbind_chat(&id);
                    self.chats().discard(&id);
                }
                self.say(
                    platform,
                    conversation,
                    &format!("Patchwork could not send that: {error}"),
                );
            }
        }
    }

    fn open_gateway_chat(
        self: &Arc<Self>,
        platform: Platform,
        conversation: &str,
    ) -> Result<(String, bool), Error> {
        let profile = self.chat_app_agent(platform)?;
        let chosen = HUB
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
}
