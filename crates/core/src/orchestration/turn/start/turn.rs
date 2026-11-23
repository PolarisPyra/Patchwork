//! What the reader of an agent's output needs to save and publish one turn's events, and the
//! empty reply its output streams into.

// Rust guideline compliant 2026-11-23

use crate::system::Locked;
use super::running::{NativeTurn, Pending, Settings};
use crate::orchestration::output::ingest::Wake;
use crate::orchestration::turn::end::background_id;

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::orchestration::changes::attribution::{Reach, SharedAttribution};
use crate::provider::TurnSpec;
use crate::provider;
use crate::Core;
use crate::chats::AccessMode;
use crate::events::AgentEvent;
use crate::provider::AgentKind;
use crate::chats::Attachment;
use crate::chats::Message;
use crate::chats::MessageStatus;
use crate::chats::Role;
use crate::orchestration::turn::process::Interrupts;
use crate::orchestration::turn::process::close_stdin_unless_steered;
use crate::orchestration::turn::process::Stdin;
use crate::orchestration::messages::queue::Queue;
use crate::orchestration::messages::queue::Steers;
use crate::orchestration::output::stream::publish;
use crate::orchestration::output::stream::Unsaved;
use crate::orchestration::tasks::subagents::Controls;

/// Everything the stdout reader needs to persist and publish one turn's events.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate fact about the turn"
)]
pub(in crate::orchestration) struct Turn {
    pub(in crate::orchestration) core: Arc<Core>,
    pub(in crate::orchestration) chat_id: String,
    pub(in crate::orchestration) message_id: String,
    pub(in crate::orchestration) agent: AgentKind,
    pub(in crate::orchestration) cancelled: Arc<AtomicBool>,
    pub(in crate::orchestration) interrupts: Arc<Interrupts>,
    pub(in crate::orchestration) exited: Arc<AtomicBool>,
    /// The files this reply changed, as far as they are known yet.
    pub(in crate::orchestration) attribution: SharedAttribution,
    /// The agent's systemd scope, when agents run in scopes.
    pub(in crate::orchestration) scope: Option<String>,
    pub(in crate::orchestration) stdin: Stdin,
    pub(in crate::orchestration) pending: Pending,
    pub(in crate::orchestration) native: NativeTurn,
    pub(in crate::orchestration) steers: Steers,
    pub(in crate::orchestration) queue: Queue,
    pub(in crate::orchestration) controls: Controls,
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
    /// Starts the reply to a message the idle agent was sent, before the agent says anything.
    pub(in crate::orchestration) wake: Wake,
    /// When the agent last said anything (ms since epoch), which `watch` reads to tell the user
    /// a reply has gone quiet.
    pub(in crate::orchestration) last_output: Arc<AtomicU64>,
    /// The reply has calls still running, which may say nothing for as long as they run.
    pub(in crate::orchestration) calls_open: Arc<AtomicBool>,
    /// `watch` stopped the Antigravity reply for saying nothing for `STALL_LIMIT`.
    pub(in crate::orchestration) stalled: Arc<AtomicBool>,
    pub(in crate::orchestration) root: String,
    pub(in crate::orchestration) session_id: Option<String>,
    pub(in crate::orchestration) model: Option<String>,
    pub(in crate::orchestration) vision_model: Option<crate::settings::VisionModel>,
    pub(in crate::orchestration) text: String,
    /// See `TurnRequest::resumes`; kept for the turn that reruns this one in a new session.
    pub(in crate::orchestration) resumes: bool,
    /// The turn's message carried the conversation so far, for an agent with no session of it.
    pub(in crate::orchestration) sent_context: bool,
    /// The chat's access mode, changed live when the user switches it mid-turn.
    pub(in crate::orchestration) mode: Arc<Mutex<AccessMode>>,
    pub(in crate::orchestration) effort: Option<String>,
    pub(in crate::orchestration) compact_window: Arc<AtomicU64>,
    /// The model and effort for the next reply, which `model` and `effort` were at launch.
    pub(in crate::orchestration) settings: Settings,
    /// What `spec` passes as `TurnSpec::unlimited_steps`.
    pub(in crate::orchestration) unlimited_steps: bool,
    /// What `spec` passes as `TurnSpec::token_limit`.
    pub(in crate::orchestration) token_limit: u64,
    pub(in crate::orchestration) attachments: Vec<Attachment>,
    pub(in crate::orchestration) images: Vec<provider::Image>,
    /// What `spec` passes as `TurnSpec::instructions`.
    pub(in crate::orchestration) instructions: Option<String>,
    /// What `spec` passes as `TurnSpec::skills_dir`.
    pub(in crate::orchestration) skills_dir: Option<String>,
    /// What `spec` passes as `TurnSpec::guarded`.
    pub(in crate::orchestration) guarded: Vec<crate::permissions::Guarded>,
    /// What `spec` passes as `TurnSpec::agents`.
    pub(in crate::orchestration) agents: Vec<crate::extensions::agents::Agent>,
    /// What `spec` passes as `TurnSpec::hooks`.
    pub(in crate::orchestration) hooks: Vec<crate::extensions::hooks::AgentHook>,
    /// What `spec` passes as `TurnSpec::dispatcher`.
    pub(in crate::orchestration) dispatcher: bool,
}

impl Turn {
    pub(in crate::orchestration) fn publish(&self, event: AgentEvent) {
        // Changes that waited for a call to cover them, which this finished one does.
        let covered = match &event {
            AgentEvent::ToolStarted { tool, .. } => {
                let reach = Reach::of(tool);
                self.attribution
                    .locked()
                    .tool_started(&tool.id, tool.started_at, reach);
                None
            }
            AgentEvent::ToolFinished {
                tool_id,
                message_id,
                ..
            } => {
                let paths = self.attribution.locked().tool_finished(tool_id);
                Some((message_id.clone(), paths))
            }
            // A background task writes until it ends, after its call has returned.
            AgentEvent::BackgroundTaskStarted { task, .. } => {
                let reach = Reach::Anything(task.description.clone());
                let id = background_id(&task.tool_id);
                self.attribution.locked().tool_started(&id, None, reach);
                None
            }
            AgentEvent::BackgroundTaskEnded { message_id, task } => {
                let id = background_id(&task.tool_id);
                let paths = self.attribution.locked().tool_finished(&id);
                Some((message_id.clone(), paths))
            }
            _ => None,
        };
        publish(&self.core, &self.chat_id, event);
        if let Some((message_id, paths)) = covered {
            for path in paths {
                let touched = AgentEvent::FileTouched {
                    message_id: message_id.clone(),
                    path,
                };
                publish(&self.core, &self.chat_id, touched);
            }
        }
    }

    /// Starts a new assistant message for what the agent says after taking a steering
    /// message, so the answer follows that message instead of growing the reply above it.
    pub(in crate::orchestration) fn next_segment(&mut self, unsaved: &mut Unsaved) {
        self.save_text(unsaved);
        let message = assistant_message();
        self.message_id.clone_from(&message.id);
        self.publish(AgentEvent::MessageStarted { message });
    }

    /// Closes stdin, which lets the agent exit, unless it still owes an answer to a
    /// steering message.
    pub(in crate::orchestration) fn close_unless_steered(&self) {
        close_stdin_unless_steered(&self.stdin, &self.steers);
    }

    pub(in crate::orchestration) fn spec(&self) -> TurnSpec<'_> {
        TurnSpec {
            root: crate::system::ssh::here(&self.root),
            host: crate::system::ssh::split(&self.root).map(|at| at.host),
            chat_id: &self.chat_id,
            session_id: self.session_id.as_deref(),
            model: self.model.as_deref(),
            vision_model: self.vision_model.as_ref(),
            text: &self.text,
            mode: *self.mode.locked(),
            effort: self.effort.as_deref(),
            compact_window: self.compact_window.load(Ordering::SeqCst),
            unlimited_steps: self.unlimited_steps,
            token_limit: self.token_limit,
            attachments: &self.attachments,
            images: &self.images,
            instructions: self.instructions.as_deref(),
            skills_dir: self.skills_dir.as_deref(),
            guarded: &self.guarded,
            agents: &self.agents,
            hooks: &self.hooks,
            dispatcher: self.dispatcher,
        }
    }
}

/// An empty assistant message the agent's output streams into.
pub(in crate::orchestration) fn assistant_message() -> Message {
    Message::new(Role::Assistant, String::new(), MessageStatus::Streaming)
}
