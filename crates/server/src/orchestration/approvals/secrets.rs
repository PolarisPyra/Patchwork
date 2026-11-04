//! An agent's request for a secret (see `crate::secrets`): a private card in its reply, which the
//! turn waits on until the user saves or declines it, the turn ends, or the agent's time runs out.

use std::collections::HashMap;
use std::sync::PoisonError;
use std::sync::atomic::Ordering;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::Server;
use crate::orchestration::{AgentEvent, Runner, publish};
use crate::secrets::{Secret, SecretAsk, SecretStatus};
use crate::system::new_id;
use crate::threads::{Approval, ApprovalStatus, Role};

/// How often the wait checks whether its turn is still running.
const POLL: Duration = Duration::from_millis(500);

impl Runner {
    /// Shows the agent's request for a secret in the thread's running turn and waits for the
    /// answer, or until the turn ends, `stopped`, or the time the agent gave runs out. Returns the
    /// tool's result for the agent, which holds where the secret is but never the secret.
    pub fn request_secret(
        &self,
        server: &Server,
        thread_id: &str,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, String> {
        let ask = SecretAsk::parse(args)?;
        let ended = || "This turn is no longer running".to_string();
        let message_id = server
            .store
            .thread(thread_id)
            .and_then(|t| {
                t.messages
                    .iter()
                    .rev()
                    .find(|m| m.role == Role::Assistant)
                    .map(|m| m.id.clone())
            })
            .ok_or_else(ended)?;
        let flags = {
            let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
            let turn = turns.get(thread_id).ok_or_else(ended)?;
            [turn.cancelled.clone(), turn.exited.clone()]
        };
        let approval = Approval {
            id: new_id(),
            title: ask.label,
            detail: ask.reason,
            status: ApprovalStatus::Pending,
            questions: Vec::new(),
            answers: HashMap::new(),
            kind: None,
            suggestions: Vec::new(),
            rule: None,
            auto: false,
            secret: Some(Secret {
                placeholder: ask.placeholder,
                status: SecretStatus::Pending,
            }),
        };
        let id = approval.id.clone();
        let answers = server.secrets.add(&id, thread_id);
        publish(
            server,
            thread_id,
            AgentEvent::ApprovalRequested {
                message_id: message_id.clone(),
                approval,
            },
        );
        let deadline = Instant::now() + ask.wait;
        let answer = loop {
            match answers.recv_timeout(POLL) {
                Ok(file) => break Ok(file),
                Err(RecvTimeoutError::Timeout) => {
                    let status = if stopped() || flags.iter().any(|f| f.load(Ordering::SeqCst)) {
                        SecretStatus::Cancelled
                    } else if Instant::now() >= deadline {
                        SecretStatus::TimedOut
                    } else {
                        continue;
                    };
                    if server.secrets.remove(&id) {
                        break Err(status);
                    }
                    // The user answered just now, and the answer is on its way.
                    break answers
                        .recv_timeout(Duration::from_secs(5))
                        .map_err(|_| status);
                }
                Err(RecvTimeoutError::Disconnected) => break Err(SecretStatus::Cancelled),
            }
        };
        let (status, file) = match answer {
            Ok(Some(file)) => (SecretStatus::Saved, Some(file)),
            Ok(None) => (SecretStatus::Declined, None),
            Err(status) => (status, None),
        };
        publish(
            server,
            thread_id,
            AgentEvent::SecretResolved {
                message_id,
                approval_id: id,
                status,
            },
        );
        Ok(crate::secrets::tool_result(status, file.as_deref()))
    }
}
