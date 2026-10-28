//! Answers the permission hooks Codex and Antigravity run before a tool call (see
//! `provider::hook`): a deny rule refuses the call, and an ask rule holds it until the user
//! answers in the transcript, like any approval.

use std::collections::HashMap;
use std::path::Path;
use std::sync::PoisonError;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use serde_json::{Value, json};

use super::turn::PendingApproval;
use super::{Runner, publish, unasked};
use crate::Server;
use crate::permissions::{self, Target, Unasked};
use crate::types::{AccessMode, AgentEvent, Approval, ApprovalStatus, Role, new_id};

impl Runner {
    /// The answer to a hook's `request` (its thread, root, title, detail, kind, and resources):
    /// `{"deny": reason}` to refuse the call, or `{"deny": null}` to let it run as the access
    /// mode decides.
    pub fn hook_request(&self, server: &Server, request: &Value) -> Value {
        let refused = |reason: &str| json!({"deny": reason});
        let text = |key: &str| request[key].as_str().unwrap_or_default().to_string();
        let (thread_id, root) = (text("thread"), text("root"));
        let Ok(kind) = serde_json::from_value(request["kind"].clone()) else {
            return json!({"deny": null});
        };
        let resources = request["resources"].as_array().into_iter().flatten().filter_map(Value::as_str);
        let target = Target::new(kind, resources);
        let full = server
            .store
            .thread(&thread_id)
            .is_some_and(|t| t.access_mode == AccessMode::FullAccess);
        let denied = match unasked(server, &thread_id, &root, &target, !full) {
            Unasked::Run => return json!({"deny": null}),
            Unasked::Refuse(decision) => Some(decision),
            Unasked::Ask => None,
        };
        let Some(message_id) = server.store.thread(&thread_id).and_then(|t| {
            t.messages.iter().rev().find(|m| m.role == Role::Assistant).map(|m| m.id.clone())
        }) else {
            return refused("This turn is no longer running");
        };
        let mut approval = Approval {
            id: new_id(),
            title: text("title"),
            detail: crate::provider::one_line(&text("detail")),
            status: ApprovalStatus::Pending,
            questions: Vec::new(),
            answers: HashMap::new(),
            kind: Some(kind),
            suggestions: permissions::suggest(&target, Path::new(&root)),
            rule: None,
            auto: false,
        };
        if let Some(decision) = denied {
            approval.status = ApprovalStatus::Denied;
            approval.rule = Some(decision.rule);
            approval.auto = true;
            publish(server, &thread_id, AgentEvent::ApprovalRequested { message_id, approval });
            return refused("A Patchwork permission rule refuses this call. Do not try it again.");
        }

        // An ask rule: the call waits for the user's answer.
        let (tx, rx) = mpsc::channel();
        let stopped = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(turn) = turns.get(&thread_id) else {
                return refused("This turn is no longer running");
            };
            self.hooks
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(approval.id.clone(), tx);
            turn.pending.lock().unwrap_or_else(PoisonError::into_inner).insert(
                approval.id.clone(),
                PendingApproval {
                    message_id: message_id.clone(),
                    reply: json!({"hook": approval.id}),
                    target: Some(target.clone()),
                    root: root.clone(),
                },
            );
            [turn.cancelled.clone(), turn.exited.clone()]
        };
        let id = approval.id.clone();
        publish(server, &thread_id, AgentEvent::ApprovalRequested { message_id, approval });
        loop {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(true) => {
                    // The agent may still ask about the call itself; it was answered here.
                    self.grants
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .entry(thread_id)
                        .or_default()
                        .push(target);
                    return json!({"deny": null});
                }
                Ok(false) => return refused("The user declined this call. Do not try it again."),
                Err(RecvTimeoutError::Timeout) if !stopped.iter().any(|s| s.load(Ordering::SeqCst)) => {}
                Err(_) => {
                    self.hooks.lock().unwrap_or_else(PoisonError::into_inner).remove(&id);
                    return refused("This turn is no longer running");
                }
            }
        }
    }

    /// Whether the user already allowed `target` through a hook, which then counts once.
    pub(super) fn take_grant(&self, thread_id: &str, target: &Target, root: &Path) -> bool {
        let mut grants = self.grants.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(granted) = grants.get_mut(thread_id) else { return false };
        let Some(i) = granted.iter().position(|g| permissions::same_request(g, target, root)) else {
            return false;
        };
        granted.remove(i);
        true
    }
}
