//! Answering the approvals a turn waits on: by the user, or by the permission rules once they
//! change.

use std::collections::HashMap;
use std::sync::PoisonError;

use crate::orchestration::turn::process::write_line;
use crate::orchestration::{AgentEvent, Runner, publish};
use crate::Server;
use crate::permissions::{self, Decision, RuleRef, Target};
use crate::provider;

/// The user's answer to an approval request: whether it was allowed, any answers to questions,
/// and the rule the user saved with it.
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
    pub fn respond(
        &self,
        server: &Server,
        thread_id: &str,
        decision: ApprovalDecision<'_>,
    ) -> Result<(), String> {
        self.resolve(server, thread_id, decision, false)
    }

    /// Answers the pending approvals the permission rules now cover, in every running turn, once
    /// the user changed the rules.
    pub fn apply_rules(&self, server: &Server) {
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
        server: &Server,
        thread_id: &str,
        decision: ApprovalDecision<'_>,
        auto: bool,
    ) -> Result<(), String> {
        let ApprovalDecision {
            approval_id,
            allow,
            answers,
            rule,
        } = decision;
        let (agent, stdin, pending) = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let turn = turns
                .get(thread_id)
                .ok_or("This turn is no longer running")?;
            let pending = turn
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(approval_id)
                .ok_or("This request was already answered")?;
            (turn.agent, turn.stdin.clone(), pending)
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
