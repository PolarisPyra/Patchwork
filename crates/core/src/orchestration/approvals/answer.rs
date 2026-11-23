//! Answering the approvals a turn waits on: by the user, or by the permission rules once they
//! change.

// Rust guideline compliant 2026-11-23

use crate::system::Locked;
use crate::Error;
use std::collections::HashMap;
use std::sync::Arc;

use crate::orchestration::turn::process::write_line;
use crate::orchestration::{Runner, publish};
use crate::events::AgentEvent;
use crate::Core;
use crate::permissions::{self, Decision, RuleRef, Target};
use crate::provider;
use crate::chats::{AccessMode, ApprovalStatus, Chat, Question};

/// The user's answer to an approval request: whether it was allowed, any answers to questions,
/// and the rule the user saved with it.
#[derive(Debug)]
pub struct ApprovalDecision<'a> {
    pub approval_id: &'a str,
    pub allow: bool,
    pub answers: HashMap<String, Vec<String>>,
    pub rule: Option<RuleRef>,
}

/// What the permission rules answer to `target` in the chat, or None to ask the user.
pub(in crate::orchestration) fn decide(
    core: &Core,
    chat_id: &str,
    root: &str,
    target: &Target,
) -> Option<Decision> {
    let chat = core.store.chat(chat_id);
    let settings = core.store.settings();
    permissions::decide(
        &settings.permissions,
        chat.as_ref().and_then(|t| t.project_id.as_deref()),
        std::path::Path::new(root),
        target,
        &readable(core, chat_id, chat.as_ref(), &settings),
    )
}

/// What the permission rules answer to `target` in the chat when the access mode runs it
/// without asking.
pub(in crate::orchestration) fn unasked(
    core: &Core,
    chat_id: &str,
    root: &str,
    target: &Target,
    asks: bool,
) -> permissions::Unasked {
    let chat = core.store.chat(chat_id);
    let settings = core.store.settings();
    permissions::unasked(
        &settings.permissions,
        chat.as_ref().and_then(|t| t.project_id.as_deref()),
        std::path::Path::new(root),
        target,
        &readable(core, chat_id, chat.as_ref(), &settings),
        asks,
    )
}

/// The folders chat `chat_id`'s agent reads without asking: the skills, every file and
/// folder the user attached in the chat, and for an agent's run the files the agent knows.
fn readable(
    core: &Core,
    chat_id: &str,
    chat: Option<&Chat>,
    settings: &crate::settings::Settings,
) -> Vec<std::path::PathBuf> {
    let mut folders = crate::extensions::skills::folders(settings);
    folders.extend(
        chat.iter()
            .flat_map(|t| &t.messages)
            .flat_map(|m| &m.attachments)
            .map(|a| std::path::PathBuf::from(&a.path)),
    );
    folders.extend(core.run_knowledge_dir(chat_id));
    folders
}

impl Runner {
    /// Sends the user's answer to a pending approval and records it.
    pub(crate) fn respond(
        &self,
        core: &Arc<Core>,
        chat_id: &str,
        decision: ApprovalDecision<'_>,
    ) -> Result<(), Error> {
        self.resolve(core, chat_id, decision, false)
    }

    /// Answers the pending approvals the permission rules now cover, in every running turn, once
    /// the user changed the rules.
    pub fn apply_rules(&self, core: &Arc<Core>) {
        let waiting: Vec<(String, String, Target, String)> = {
            let turns = self.turns.locked();
            turns
                .iter()
                .flat_map(|(chat_id, turn)| {
                    let pending = turn.pending.locked();
                    pending
                        .iter()
                        .filter_map(|(id, p)| {
                            let target = p.target.clone()?;
                            Some((chat_id.clone(), id.clone(), target, p.root.clone()))
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        for (chat_id, approval_id, target, root) in waiting {
            let Some(Decision { allow, rule }) = decide(core, &chat_id, &root, &target) else {
                continue;
            };
            let decision = ApprovalDecision {
                approval_id: &approval_id,
                allow,
                answers: HashMap::new(),
                rule: Some(rule),
            };
            // The user may have answered it meanwhile.
            let _ = self.resolve(core, &chat_id, decision, true);
        }
    }

    /// Hands the chat's new access mode to its running turn, and answers the approvals waiting
    /// there that Full Access runs without asking.
    ///
    /// True when the agent takes the mode at its next reply, as Antigravity does. Otherwise the
    /// agent needs a new process.
    pub(crate) fn set_access_mode(
        &self,
        core: &Arc<Core>,
        chat_id: &str,
        mode: AccessMode,
    ) -> bool {
        let (waiting, live): (Vec<(String, Target, String)>, bool) = {
            let turns = self.turns.locked();
            let Some(turn) = turns.get(chat_id) else {
                return false;
            };
            *turn.mode.locked() = mode;
            let live = turn.agent == provider::AgentKind::Antigravity;
            if mode != AccessMode::FullAccess {
                return live;
            }
            let waiting = turn
                .pending
                .locked()
                .iter()
                // A permission hook decides for itself; only the agent's own requests are answered.
                .filter(|(_, p)| p.reply.get("hook").is_none())
                .filter_map(|(id, p)| Some((id.clone(), p.target.clone()?, p.root.clone())))
                .collect();
            (waiting, live)
        };
        for (approval_id, target, root) in waiting {
            let blocked = core.agent_refuses(chat_id, std::path::Path::new(&root), &target);
            if blocked || unasked(core, chat_id, &root, &target, false) != permissions::Unasked::Run
            {
                continue;
            }
            let decision = ApprovalDecision {
                approval_id: &approval_id,
                allow: true,
                answers: HashMap::new(),
                rule: None,
            };
            // The user may have answered it meanwhile.
            let _ = self.resolve(core, chat_id, decision, true);
        }
        live
    }

    /// Sends an answer to a pending approval and records it; `auto` when a rule gave it.
    fn resolve(
        &self,
        core: &Arc<Core>,
        chat_id: &str,
        decision: ApprovalDecision<'_>,
        auto: bool,
    ) -> Result<(), Error> {
        let ApprovalDecision {
            approval_id,
            allow,
            answers,
            rule,
        } = decision;
        let waited = {
            let turns = self.turns.locked();
            turns.get(chat_id).and_then(|turn| {
                turn.pending
                    .locked()
                    .remove(approval_id)
                    .map(|pending| (turn.agent, Arc::clone(&turn.stdin), pending))
            })
        };
        // No turn is waiting for this one: the app closed under it, or the reply it belonged to
        // ended. The answer still counts, and a question is answered by asking it again in a new
        // turn, so a request the person can still read above the composer can still be answered.
        let Some((agent, stdin, pending)) = waited else {
            return answer_in_new_turn(core, chat_id, approval_id, allow, &answers, rule, auto);
        };
        // A permission hook holds the call until it hears the answer.
        if pending.reply.get("hook").is_some() {
            if let Some(waiter) = self.hooks.locked().remove(approval_id) {
                let _ = waiter.send(allow);
            }
        } else {
            write_line(
                &stdin,
                &provider::approval_reply(agent, &pending.reply, allow, &answers),
            );
        }
        publish(
            core,
            chat_id,
            AgentEvent::ApprovalResolved {
                message_id: pending.message_id,
                approval_id: approval_id.into(),
                allowed: allow,
                answers,
                rule,
                auto,
            },
        );
        Ok(())
    }
}

/// Answers an approval no running turn is waiting for, in `chat_id`: records the answer on the
/// stored request, and sends what it says as the next turn's message. That is how a question the
/// app asked before it closed, or the one above the composer after its reply was stopped, can still
/// be answered: the agent is told, in a message of the user's, rather than nothing happening.
///
/// A permission request does not become a message: there is no file to write and no command to run
/// once the turn that asked is gone, so it is only recorded as answered or denied.
fn answer_in_new_turn(
    core: &Arc<Core>,
    chat_id: &str,
    approval_id: &str,
    allow: bool,
    answers: &HashMap<String, Vec<String>>,
    rule: Option<RuleRef>,
    auto: bool,
) -> Result<(), Error> {
    // The request as stored: one already answered, or one this chat never had, is refused rather
    // than answered a second time.
    let (message_id, questions) = core
        .store
        .chat(chat_id)
        .and_then(|chat| {
            chat.messages.iter().find_map(|message| {
                let approval = message
                    .approvals
                    .iter()
                    .find(|a| a.id == approval_id && a.status == ApprovalStatus::Pending)?;
                Some((message.id.clone(), approval.questions.clone()))
            })
        })
        .ok_or("This request was already answered")?;
    // Said before it is recorded, so a turn that cannot start leaves the request waiting for
    // another try rather than answered and lost.
    if allow && let Some(text) = answer_message(&questions, answers) {
        core.send_message(chat_id.to_string(), text, Vec::new(), false)?;
    }
    let status = if allow {
        ApprovalStatus::Allowed
    } else {
        ApprovalStatus::Denied
    };
    let recorded = answers.clone();
    core.store.update_chat(chat_id, |chat| {
        for message in &mut chat.messages {
            let Some(at) = message.approvals.iter().position(|a| a.id == approval_id) else {
                continue;
            };
            let approval = &mut Arc::make_mut(message).approvals[at];
            approval.status = status;
            approval.answers.clone_from(&recorded);
            approval.rule.clone_from(&rule);
            approval.auto = auto;
            break;
        }
    })?;
    publish(
        core,
        chat_id,
        AgentEvent::ApprovalResolved {
            message_id,
            approval_id: approval_id.into(),
            allowed: allow,
            answers: answers.clone(),
            rule,
            auto,
        },
    );
    // Asked for and dismissed both stay quiet: only an answer the person gave is worth a message,
    // and that one was sent above.
    Ok(())
}

/// What a person's answers say as a message of their own, so the agent that asked hears them even
/// when the session that asked is gone. Nothing when they answered no question.
fn answer_message(
    questions: &[Question],
    answers: &HashMap<String, Vec<String>>,
) -> Option<String> {
    let said: Vec<String> = questions
        .iter()
        .filter_map(|q| {
            let answer = answers.get(&q.id)?;
            let answer = answer.join(", ");
            (!answer.is_empty()).then(|| format!("My answer to “{}”: {answer}", q.question.trim()))
        })
        .collect();
    if said.is_empty() {
        return None;
    }
    Some(said.join("\n"))
}
