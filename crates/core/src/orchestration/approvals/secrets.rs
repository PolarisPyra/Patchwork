//! An agent's request for a secret (see `crate::secrets`): a private card in its reply, which the
//! turn waits on until the user saves or declines it, the turn ends, or the agent's time runs out.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::sync::atomic::Ordering;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::Core;
use crate::orchestration::{Runner, publish};
use crate::events::AgentEvent;
use crate::secrets::{Secret, SecretAsk, SecretStatus};
use crate::threads::Approval;
use super::hooks::reply_id;

/// How often the wait checks whether its turn is still running.
const POLL: Duration = Duration::from_millis(500);

impl Runner {
    /// Shows the agent's request for a secret in the thread's running turn and waits for the
    /// answer, or until the turn ends, `stopped`, or the time the agent gave runs out. Returns the
    /// tool's result for the agent, which holds where the secret is but never the secret.
    pub(crate) fn request_secret(
        &self,
        core: &Core,
        thread_id: &str,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let ask = SecretAsk::parse(args)?;
        let message_id = reply_id(core, thread_id).ok_or("This turn is no longer running")?;
        let flags = self.turn_flags(thread_id)?;
        let approval = Approval {
            secret: Some(Secret {
                placeholder: ask.placeholder,
                status: SecretStatus::Pending,
            }),
            ..Approval::pending(ask.label, ask.reason)
        };
        let id = approval.id.clone();
        let answers = core.secrets.add(&id, thread_id);
        publish(
            core,
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
                    if core.secrets.remove(&id) {
                        break Err(status);
                    }
                    // The user answered just now, and the answer is on its way.
                    break answers
                        .recv_timeout(Duration::from_secs(5))
                        .map_err(|_discarded| status);
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
            core,
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
