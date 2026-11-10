//! Answering the approvals a turn waits on: by the user, or by the permission rules once they
//! change.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;
use std::sync::{Arc, PoisonError};

use crate::orchestration::turn::process::write_line;
use crate::orchestration::{AgentEvent, Runner, publish};
use crate::Server;
use crate::permissions::{self, Decision, RuleRef, Target};
use crate::provider;
use crate::threads::{ApprovalStatus, Question};

/// The user's answer to an approval request: whether it was allowed, any answers to questions,
/// and the rule the user saved with it.
#[derive(Debug)]
pub struct ApprovalDecision<'a> {
    pub approval_id: &'a str,
    pub allow: bool,
    pub answers: HashMap<String, Vec<String>>,
    pub rule: Option<RuleRef>,
}

/// What the permission rules answer to `target` in the thread, or None to ask the user.
pub(in crate::orchestration) fn decide(
    server: &Server,
    thread_id: &str,
    root: &str,
    target: &Target,
) -> Option<Decision> {
    let project = server.store.thread(thread_id).and_then(|t| t.project_id);
    let settings = server.store.settings();
    permissions::decide(
        &settings.permissions,
        project.as_deref(),
        std::path::Path::new(root),
        target,
        &crate::skills::folders(&settings),
    )
}

/// What the permission rules answer to `target` in the thread when the access mode runs it
/// without asking.
pub(in crate::orchestration) fn unasked(
    server: &Server,
    thread_id: &str,
    root: &str,
    target: &Target,
    asks: bool,
) -> permissions::Unasked {
    let project = server.store.thread(thread_id).and_then(|t| t.project_id);
    let settings = server.store.settings();
    permissions::unasked(
        &settings.permissions,
        project.as_deref(),
        std::path::Path::new(root),
        target,
        &crate::skills::folders(&settings),
        asks,
    )
}

impl Runner {
    /// Sends the user's answer to a pending approval and records it.
    pub(crate) fn respond(
        &self,
        server: &Arc<Server>,
        thread_id: &str,
        decision: ApprovalDecision<'_>,
    ) -> Result<(), Error> {
        self.resolve(server, thread_id, decision, false)
    }

    /// Answers the pending approvals the permission rules now cover, in every running turn, once
    /// the user changed the rules.
    pub fn apply_rules(&self, server: &Arc<Server>) {
        let waiting: Vec<(String, String, Target, String)> = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            turns
                .iter()
                .flat_map(|(thread_id, turn)| {
                    let pending = turn.pending.lock().unwrap_or_else(PoisonError::into_inner);
                    pending
                        .iter()
                        .filter_map(|(id, p)| {
                            let target = p.target.clone()?;
                            Some((thread_id.clone(), id.clone(), target, p.root.clone()))
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        for (thread_id, approval_id, target, root) in waiting {
            let Some(Decision { allow, rule }) = decide(server, &thread_id, &root, &target) else {
                continue;
            };
            let decision = ApprovalDecision {
                approval_id: &approval_id,
                allow,
                answers: HashMap::new(),
                rule: Some(rule),
            };
            // The user may have answered it meanwhile.
            let _ = self.resolve(server, &thread_id, decision, true);
        }
    }

    /// Sends an answer to a pending approval and records it; `auto` when a rule gave it.
    fn resolve(
        &self,
        server: &Arc<Server>,
        thread_id: &str,
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
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            turns.get(thread_id).and_then(|turn| {
                turn.pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(approval_id)
                    .map(|pending| (turn.agent, Arc::clone(&turn.stdin), pending))
            })
        };
        // No turn is waiting for this one: the app closed under it, or the reply it belonged to
        // ended. The answer still counts, and a question is answered by asking it again in a new
        // turn, so a request the person can still read above the composer can still be answered.
        let Some((agent, stdin, pending)) = waited else {
            return answer_in_new_turn(server, thread_id, approval_id, allow, &answers, rule, auto);
        };
        // A permission hook holds the call until it hears the answer.
        if pending.reply.get("hook").is_some() {
            if let Some(waiter) = self
                .hooks
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(approval_id)
            {
                let _ = waiter.send(allow);
            }
        } else {
            write_line(
                &stdin,
                &provider::approval_reply(agent, &pending.reply, allow, &answers),
            );
        }
        publish(
            server,
            thread_id,
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

/// Answers an approval no running turn is waiting for, in `thread_id`: records the answer on the
/// stored request, and sends what it says as the next turn's message. That is how a question the
/// app asked before it closed, or the one above the composer after its reply was stopped, can still
/// be answered: the agent is told, in a message of the user's, rather than nothing happening.
///
/// A permission request does not become a message: there is no file to write and no command to run
/// once the turn that asked is gone, so it is only recorded as answered or denied.
fn answer_in_new_turn(
    server: &Arc<Server>,
    thread_id: &str,
    approval_id: &str,
    allow: bool,
    answers: &HashMap<String, Vec<String>>,
    rule: Option<RuleRef>,
    auto: bool,
) -> Result<(), Error> {
    // The request as stored: one already answered, or one this chat never had, is refused rather
    // than answered a second time.
    let (message_id, questions) = server
        .store
        .thread(thread_id)
        .and_then(|thread| {
            thread.messages.into_iter().find_map(|message| {
                let approval = message
                    .approvals
                    .into_iter()
                    .find(|a| a.id == approval_id && a.status == ApprovalStatus::Pending)?;
                Some((message.id, approval.questions))
            })
        })
        .ok_or("This request was already answered")?;
    // Said before it is recorded, so a turn that cannot start leaves the request waiting for
    // another try rather than answered and lost.
    if allow && let Some(text) = answer_message(&questions, answers) {
        server.send_message(thread_id.to_string(), text, Vec::new(), false)?;
    }
    let status = if allow {
        ApprovalStatus::Allowed
    } else {
        ApprovalStatus::Denied
    };
    let recorded = answers.clone();
    server.store.update_thread(thread_id, |thread| {
        for message in &mut thread.messages {
            let Some(approval) = message.approvals.iter_mut().find(|a| a.id == approval_id) else {
                continue;
            };
            approval.status = status;
            approval.answers.clone_from(&recorded);
            approval.rule.clone_from(&rule);
            approval.auto = auto;
            break;
        }
    })?;
    publish(
        server,
        thread_id,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threads::QuestionOption;

    fn question(id: &str, asked: &str) -> Question {
        Question {
            id: id.into(),
            header: "Direction".into(),
            question: asked.into(),
            options: vec![QuestionOption {
                label: "Dark".into(),
                description: "graphite".into(),
            }],
            multi_select: false,
        }
    }

    #[test]
    fn an_answer_becomes_a_message_naming_the_question() {
        let questions = vec![question("q1", "Which direction?")];
        let answers = HashMap::from([("q1".to_string(), vec!["Dark".to_string()])]);
        assert_eq!(
            answer_message(&questions, &answers).as_deref(),
            Some("My answer to \u{201c}Which direction?\u{201d}: Dark")
        );
    }

    #[test]
    fn every_question_answered_is_in_the_message() {
        let questions = vec![
            question("q1", "Which direction?"),
            question("q2", "And the font?"),
        ];
        let answers = HashMap::from([
            ("q1".to_string(), vec!["Dark".to_string()]),
            (
                "q2".to_string(),
                vec!["Mono".to_string(), "Sans".to_string()],
            ),
        ]);
        let text = answer_message(&questions, &answers).unwrap();
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(text.contains("Mono, Sans"), "{text}");
    }

    /// Nothing to say: a request dismissed, or one answered with no question attached. No message
    /// is sent, so dismissing a question does not start a turn.
    #[test]
    fn dismissing_a_question_says_nothing() {
        let questions = vec![question("q1", "Which direction?")];
        assert!(answer_message(&questions, &HashMap::new()).is_none());
        assert!(answer_message(&[], &HashMap::new()).is_none());
    }
}
