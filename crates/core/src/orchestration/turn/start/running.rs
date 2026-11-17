//! What a turn that is running holds, and what one is asked to send.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use serde_json::Value;

use crate::orchestration::changes::attribution::SharedAttribution;
use crate::permissions::Target;
use crate::provider;
use crate::provider::AgentKind;
use crate::threads::Attachment;
use crate::orchestration::turn::process::Interrupts;
use crate::orchestration::turn::process::Stdin;
use crate::orchestration::messages::queue::Queue;
use crate::orchestration::messages::queue::Steers;
use crate::orchestration::tasks::subagents::Controls;
use crate::orchestration::tasks::subagents::Tasks;

/// An approval waiting for the user.
pub(in crate::orchestration) struct PendingApproval {
    pub(in crate::orchestration) message_id: String,
    /// What `approval_reply` needs to answer it.
    pub(in crate::orchestration) reply: Value,
    /// What it asks to do, for the permission rules.
    pub(in crate::orchestration) target: Option<Target>,
    /// The folder the thread works in.
    pub(in crate::orchestration) root: String,
}

/// Pending approvals of one turn, by approval id.
pub(in crate::orchestration) type Pending = Arc<Mutex<HashMap<String, PendingApproval>>>;

/// The agent's id for the turn now running (Codex reports it once the turn starts).
pub(in crate::orchestration) type NativeTurn = Arc<Mutex<Option<String>>>;

pub(in crate::orchestration) struct RunningTurn {
    pub(in crate::orchestration) pid: u32,
    pub(in crate::orchestration) cancelled: Arc<AtomicBool>,
    /// The interrupts Stop wrote to the agent, and whether it answered them.
    pub(in crate::orchestration) interrupts: Arc<Interrupts>,
    /// Set once the agent process has exited and been reaped.
    pub(in crate::orchestration) exited: Arc<AtomicBool>,
    /// The systemd scope the agent runs in, when scopes are on.
    pub(in crate::orchestration) scope: Option<String>,
    pub(in crate::orchestration) agent: AgentKind,
    pub(in crate::orchestration) stdin: Stdin,
    pub(in crate::orchestration) pending: Pending,
    pub(in crate::orchestration) native: NativeTurn,
    pub(in crate::orchestration) steers: Steers,
    pub(in crate::orchestration) queue: Queue,
    pub(in crate::orchestration) tasks: Tasks,
    pub(in crate::orchestration) controls: Controls,
    /// The agent has finished its reply and stays up only for shell commands it left running;
    /// the thread counts as idle, and the next message starts a new reply in this process.
    pub(in crate::orchestration) idle: Arc<AtomicBool>,
    /// The thread's settings changed while the agent was replying, so it is not used again once
    /// idle: the next message starts a new process with the settings as they are now.
    pub(in crate::orchestration) stale: AtomicBool,
    /// The folder the agent works in.
    pub(in crate::orchestration) root: String,
    /// Shared with the project's watcher, which adds files to the reply as they change.
    pub(in crate::orchestration) attribution: SharedAttribution,
}

/// What a new turn sends to the agent, and the thread it belongs to.
#[derive(Debug)]
pub struct TurnRequest {
    pub thread_id: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub images: Vec<provider::Image>,
    /// The turn carries on one the app's closing cut off, so no message of the user's starts it:
    /// the project was snapshotted before the turn it continues, and no prompt was submitted.
    pub resumes: bool,
}
