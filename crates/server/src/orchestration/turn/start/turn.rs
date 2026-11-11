//! What the reader of an agent's output needs to save and publish one turn's events, and the
//! empty reply its output streams into.

// Rust guideline compliant 2026-10-09

use super::running::{NativeTurn, Pending};
use crate::orchestration::turn::end::background_id;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::orchestration::changes::attribution::{self, Reach, SharedAttribution};
use crate::provider::TurnSpec;
use crate::provider;
use crate::Server;
use crate::threads::AccessMode;
use crate::events::AgentEvent;
use crate::provider::AgentKind;
use crate::threads::Attachment;
use crate::threads::Message;
use crate::threads::MessageStatus;
use crate::system::new_id;
use crate::system::now_ms;
use crate::threads::Role;
use crate::orchestration::turn::process::Interrupts;
use crate::orchestration::turn::process::close_stdin_unless_steered;
use crate::orchestration::turn::process::Stdin;
use crate::orchestration::messages::queue::Queue;
use crate::orchestration::messages::queue::Steers;
use crate::orchestration::output::stream::publish;
use crate::orchestration::output::stream::Unsaved;
use crate::orchestration::tasks::subagents::Controls;
use crate::orchestration::tasks::subagents::Tasks;

/// Everything the stdout reader needs to persist and publish one turn's events.
pub(in crate::orchestration) struct Turn {
    pub(in crate::orchestration) server: Arc<Server>,
    pub(in crate::orchestration) thread_id: String,
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
    pub(in crate::orchestration) tasks: Tasks,
    pub(in crate::orchestration) controls: Controls,
    pub(in crate::orchestration) idle: Arc<AtomicBool>,
    pub(in crate::orchestration) root: String,
    pub(in crate::orchestration) session_id: Option<String>,
    pub(in crate::orchestration) model: Option<String>,
    pub(in crate::orchestration) vision_model: Option<crate::settings::VisionModel>,
    pub(in crate::orchestration) text: String,
    /// See `TurnRequest::resumes`; kept for the turn that reruns this one in a new session.
    pub(in crate::orchestration) resumes: bool,
    /// The turn's message carried the conversation so far, for an agent with no session of it.
    pub(in crate::orchestration) sent_context: bool,
    pub(in crate::orchestration) mode: AccessMode,
    pub(in crate::orchestration) plan: bool,
    pub(in crate::orchestration) effort: Option<String>,
    pub(in crate::orchestration) compact_window: u64,
    pub(in crate::orchestration) attachments: Vec<Attachment>,
    pub(in crate::orchestration) images: Vec<provider::Image>,
    /// What `spec` passes as `TurnSpec::instructions`.
    pub(in crate::orchestration) instructions: Option<String>,
    /// What `spec` passes as `TurnSpec::skills_dir`.
    pub(in crate::orchestration) skills_dir: Option<String>,
    /// What `spec` passes as `TurnSpec::guarded`.
    pub(in crate::orchestration) guarded: Vec<crate::permissions::Guarded>,
    /// What `spec` passes as `TurnSpec::agents`.
    pub(in crate::orchestration) agents: Vec<crate::agents::Agent>,
    /// What `spec` passes as `TurnSpec::hooks`.
    pub(in crate::orchestration) hooks: Vec<crate::plugins::AgentHook>,
}

impl Turn {
    pub(in crate::orchestration) fn publish(&self, event: AgentEvent) {
        // Changes that waited for a call to cover them, which this finished one does.
        let covered = match &event {
            AgentEvent::ToolStarted { tool, .. } => {
                let reach = Reach::of(tool);
                attribution::lock(&self.attribution).tool_started(&tool.id, tool.started_at, reach);
                None
            }
            AgentEvent::ToolFinished {
                tool_id,
                message_id,
                ..
            } => {
                let paths = attribution::lock(&self.attribution).tool_finished(tool_id);
                Some((message_id.clone(), paths))
            }
            // A background task writes until it ends, after its call has returned.
            AgentEvent::BackgroundTaskStarted { task, .. } => {
                let reach = Reach::Anything(task.description.clone());
                let id = background_id(&task.tool_id);
                attribution::lock(&self.attribution).tool_started(&id, None, reach);
                None
            }
            AgentEvent::BackgroundTaskEnded { message_id, task } => {
                let id = background_id(&task.tool_id);
                let paths = attribution::lock(&self.attribution).tool_finished(&id);
                Some((message_id.clone(), paths))
            }
            _ => None,
        };
        publish(&self.server, &self.thread_id, event);
        if let Some((message_id, paths)) = covered {
            for path in paths {
                let touched = AgentEvent::FileTouched {
                    message_id: message_id.clone(),
                    path,
                };
                publish(&self.server, &self.thread_id, touched);
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
            root: &self.root,
            thread_id: &self.thread_id,
            session_id: self.session_id.as_deref(),
            model: self.model.as_deref(),
            vision_model: self.vision_model.as_ref(),
            text: &self.text,
            mode: self.mode,
            plan: self.plan,
            effort: self.effort.as_deref(),
            compact_window: self.compact_window,
            attachments: &self.attachments,
            images: &self.images,
            instructions: self.instructions.as_deref(),
            skills_dir: self.skills_dir.as_deref(),
            guarded: &self.guarded,
            agents: &self.agents,
            hooks: &self.hooks,
        }
    }
}

/// An empty assistant message the agent's output streams into.
pub(in crate::orchestration) fn assistant_message() -> Message {
    Message {
        id: new_id(),
        role: Role::Assistant,
        text: String::new(),
        created_at: now_ms(),
        status: MessageStatus::Streaming,
        tools: vec![],
        plan: vec![],
        files: vec![],
        error: None,
        approvals: vec![],
        attachments: vec![],
        steer: false,
        queued: false,
        background: Vec::new(),
        cut_off: false,
        refusal: None,
        parts: Vec::new(),
        finished_at: None,
        usage: None,
        turn: None,
        checks: None,
    }
}
