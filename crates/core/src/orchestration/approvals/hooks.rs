//! Answers the permission hooks Codex and Antigravity run before a tool call (see
//! `permissions::hook`): a deny rule refuses the call, and an ask rule holds it until the user
//! answers in the transcript, like any approval.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use serde_json::{Value, json};

use crate::orchestration::turn::start::PendingApproval;
use crate::orchestration::{Runner, publish, unasked};
use crate::{Core, Error};
use crate::permissions::{self, PermissionKind, Target, Unasked};
use crate::events::AgentEvent;
use crate::threads::{AccessMode, Approval, ApprovalStatus, Role};

impl Runner {
    /// The answer to a hook's `request` (its thread, root, title, detail, kind, and resources):
    /// `{"deny": reason}` to refuse the call, or `{"deny": null}` to let it run as the access
    /// mode decides.
    pub fn hook_request(&self, core: &Core, request: &Value) -> Value {
        let refused = |reason: &str| json!({"deny": reason});
        let text = |key: &str| request[key].as_str().unwrap_or_default().to_string();
        let (thread_id, root) = (text("thread"), text("root"));
        // A request whose kind this version cannot read is refused rather than let past the rules.
        let Ok(kind) = serde_json::from_value(request["kind"].clone()) else {
            return refused("Patchwork could not read this call to check it against its rules");
        };
        let resources = request["resources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str);
        let target = Target::new(kind, resources);
        let full = core
            .store
            .thread(&thread_id)
            .is_some_and(|t| t.access_mode == AccessMode::FullAccess);
        // A tool the bot blocks is refused before any rule is read.
        if core.bot_refuses(&thread_id, Path::new(&root), &target) {
            return Self::refuse_blocked(core, &thread_id, &text("title"), &text("detail"), kind);
        }
        let denied = match unasked(core, &thread_id, &root, &target, !full) {
            Unasked::Run => return json!({"deny": null}),
            Unasked::Refuse(decision) => Some(decision),
            Unasked::Ask => None,
        };
        let Some(message_id) = reply_id(core, &thread_id) else {
            return refused("This turn is no longer running");
        };
        let mut approval = Approval {
            kind: Some(kind),
            suggestions: permissions::suggest(&target, Path::new(&root)),
            ..Approval::pending(text("title"), crate::provider::one_line(&text("detail")))
        };
        if let Some(decision) = denied {
            approval.status = ApprovalStatus::Denied;
            approval.rule = Some(decision.rule);
            approval.auto = true;
            publish(
                core,
                &thread_id,
                AgentEvent::ApprovalRequested {
                    message_id,
                    approval,
                },
            );
            return refused("A Patchwork permission rule refuses this call. Do not try it again.");
        }

        // An ask rule: the call waits for the user's answer.
        match self.hold(core, thread_id, message_id, root, approval, Some(target)) {
            Ok(()) => json!({"deny": null}),
            Err(reason) => refused(reason),
        }
    }

    /// Records a call the bot behind thread `thread_id` blocks as refused, in its latest reply,
    /// and returns the hook's answer that refuses it.
    fn refuse_blocked(
        core: &Core,
        thread_id: &str,
        title: &str,
        detail: &str,
        kind: PermissionKind,
    ) -> Value {
        let reason = crate::bots::BLOCKED_REASON;
        if let Some(message_id) = reply_id(core, thread_id) {
            let approval = Approval {
                status: ApprovalStatus::Denied,
                kind: Some(kind),
                auto: true,
                ..Approval::pending(
                    title.to_string(),
                    format!("{reason}: {}", crate::provider::one_line(detail)),
                )
            };
            publish(
                core,
                thread_id,
                AgentEvent::ApprovalRequested {
                    message_id,
                    approval,
                },
            );
        }
        json!({"deny": format!("{reason}. Do not try it again.")})
    }

    /// Asks the person, in the latest reply of the chat `thread_id`, to allow what `title` and
    /// `detail` say, and waits for their answer while the chat's turn runs. Patchwork's own
    /// operations ask this way before they change something.
    ///
    /// # Errors
    ///
    /// Returns why not when the person declines or the turn ends first.
    pub(in crate::orchestration) fn confirm(
        &self,
        core: &Core,
        thread_id: &str,
        title: &str,
        detail: &str,
    ) -> Result<(), Error> {
        let message_id = reply_id(core, thread_id).ok_or("This turn is no longer running")?;
        let approval = Approval::pending(title.to_string(), crate::provider::one_line(detail));
        self.hold(
            core,
            thread_id.to_string(),
            message_id,
            String::new(),
            approval,
            None,
        )
        .map_err(Error::from)
    }

    /// Shows `approval` in the message `message_id` and waits until the person answers it or the
    /// turn of `thread_id` stops.
    fn hold(
        &self,
        core: &Core,
        thread_id: String,
        message_id: String,
        root: String,
        approval: Approval,
        target: Option<Target>,
    ) -> Result<(), &'static str> {
        let (tx, rx) = mpsc::channel();
        let stopped = {
            let turns = self.turns.locked();
            let Some(turn) = turns.get(&thread_id) else {
                return Err("This turn is no longer running");
            };
            self.hooks.locked().insert(approval.id.clone(), tx);
            turn.pending.locked().insert(
                approval.id.clone(),
                PendingApproval {
                    message_id: message_id.clone(),
                    reply: json!({"hook": approval.id}),
                    target: target.clone(),
                    root,
                },
            );
            [Arc::clone(&turn.cancelled), Arc::clone(&turn.exited)]
        };
        let id = approval.id.clone();
        publish(
            core,
            &thread_id,
            AgentEvent::ApprovalRequested {
                message_id,
                approval,
            },
        );
        loop {
            // Wakes once a second to see whether the turn was stopped while the approval waits.
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(true) => {
                    // The agent may still ask about the call itself; it was answered here.
                    if let Some(target) = target {
                        self.grants
                            .locked()
                            .entry(thread_id)
                            .or_default()
                            .push(target);
                    }
                    return Ok(());
                }
                Ok(false) => return Err("The user declined this call. Do not try it again."),
                Err(RecvTimeoutError::Timeout)
                    if !stopped.iter().any(|s| s.load(Ordering::SeqCst)) => {}
                Err(_) => {
                    self.hooks.locked().remove(&id);
                    return Err("This turn is no longer running");
                }
            }
        }
    }

    /// Whether the user already allowed `target` through a hook, which then counts once.
    pub(in crate::orchestration) fn take_grant(
        &self,
        thread_id: &str,
        target: &Target,
        root: &Path,
    ) -> bool {
        let mut grants = self.grants.locked();
        let Some(granted) = grants.get_mut(thread_id) else {
            return false;
        };
        let Some(i) = granted
            .iter()
            .position(|g| permissions::same_request(g, target, root))
        else {
            return false;
        };
        granted.remove(i);
        true
    }
}

/// The id of the newest reply in the thread: the one its running turn writes into.
pub(super) fn reply_id(core: &Core, thread_id: &str) -> Option<String> {
    core.store.thread(thread_id).and_then(|t| {
        t.messages
            .iter()
            .rfind(|m| m.role == Role::Assistant)
            .map(|m| m.id.clone())
    })
}
