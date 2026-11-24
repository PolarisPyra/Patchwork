//! What a turn that is running holds, and what one is asked to send.

// Rust guideline compliant 2026-11-23

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Mutex;

use serde_json::Value;

use crate::orchestration::changes::attribution::SharedAttribution;
use crate::permissions::Target;
use crate::provider;
use crate::provider::antigravity;
use crate::provider::{AgentKind, Parsed};
use crate::chats::{AccessMode, Attachment};
use crate::orchestration::turn::process::Interrupts;
use crate::orchestration::turn::process::Stdin;
use crate::orchestration::messages::queue::Queue;
use crate::orchestration::messages::queue::Steers;
use crate::orchestration::output::ingest::Wake;
use crate::orchestration::tasks::subagents::Controls;

/// An approval waiting for the user.
pub(in crate::orchestration) struct PendingApproval {
    pub(in crate::orchestration) message_id: String,
    /// What `approval_reply` needs to answer it.
    pub(in crate::orchestration) reply: Value,
    /// What it asks to do, for the permission rules.
    pub(in crate::orchestration) target: Option<Target>,
    /// The folder the chat works in.
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
    pub(in crate::orchestration) controls: Controls,
    /// The agent has finished its reply and stays up only for shell commands it left running;
    /// the chat counts as idle, and the next message starts a new reply in this process.
    pub(in crate::orchestration) idle: Arc<AtomicBool>,
    /// Antigravity is at work on a reply it started by itself, while the chat counts as idle.
    /// Retiring the agent waits for that work to end.
    pub(in crate::orchestration) alone: Arc<AtomicBool>,
    /// Patchwork stopped the Antigravity reply to compact it. A steer meanwhile waits for the
    /// compaction to end instead of stopping it.
    pub(in crate::orchestration) compacting: Arc<AtomicBool>,
    /// Antigravity was sent a cancel for a steer and has yet to take the steers. A steer
    /// meanwhile joins them instead of sending another cancel.
    pub(in crate::orchestration) cancelling: Arc<AtomicBool>,
    /// The chat's access mode, shared with the turn so a change applies to the reply under way.
    pub(in crate::orchestration) mode: Arc<Mutex<AccessMode>>,
    pub(in crate::orchestration) settings: Settings,
    /// Live compact window (0 when off), updated when the user changes it mid-turn or while idle.
    pub(in crate::orchestration) compact_window: Arc<AtomicU64>,
    /// Starts the reply to a message the idle agent was sent, before the agent says anything.
    pub(in crate::orchestration) wake: Wake,
    /// The chat's settings changed while the agent was replying, so it is not used again once
    /// idle: the next message starts a new process with the settings as they are now.
    pub(in crate::orchestration) stale: AtomicBool,
    /// The folder the agent works in.
    pub(in crate::orchestration) root: String,
    /// Shared with the project's watcher, which adds files to the reply as they change.
    pub(in crate::orchestration) attribution: SharedAttribution,
}

/// The model and effort the chat wants, and what the agent last said it runs with. Antigravity
/// takes changes in its session at its next reply, without a new process.
#[derive(Default)]
pub(in crate::orchestration) struct LiveSettings {
    pub(in crate::orchestration) model: Option<String>,
    pub(in crate::orchestration) effort: Option<String>,
    /// The agent's settings as it last reported them.
    pub(in crate::orchestration) reported: Value,
    /// Lines waiting to be sent, each once the agent answers the one before.
    pub(in crate::orchestration) chain: VecDeque<Parsed>,
}

/// One turn's live settings, shared by the turn and the runner.
pub(in crate::orchestration) type Settings = Arc<Mutex<LiveSettings>>;

impl LiveSettings {
    /// The line that starts the agent's next reply with `prompt`. For Antigravity that is the
    /// first request bringing its session to the chat's settings, with the rest and then the
    /// prompt waiting in `chain`.
    pub(in crate::orchestration) fn first_line(
        &mut self,
        agent: AgentKind,
        mode: AccessMode,
        session_id: Option<&str>,
        prompt: String,
    ) -> String {
        let Some(session_id) = session_id.filter(|_| agent.speaks_acp()) else {
            return prompt;
        };
        let wanted = antigravity::Wanted {
            agent,
            model: self.model.as_deref(),
            effort: self.effort.as_deref(),
            mode,
        };
        let mut lines = antigravity::configure(session_id, wanted, &self.reported).into_iter();
        let Some(first) = lines.next() else {
            return prompt;
        };
        self.chain = lines.chain([prompt]).map(Parsed::Send).collect();
        first
    }
}

/// What a new turn sends to the agent, and the chat it belongs to.
#[derive(Debug)]
pub struct TurnRequest {
    pub chat_id: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub images: Vec<provider::Image>,
    /// The turn carries on one the app's closing cut off, so no message of the user's starts it:
    /// the project was snapshotted before the turn it continues, and no prompt was submitted.
    pub resumes: bool,
}
