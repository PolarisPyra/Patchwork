//! Sending messages and running turns: naming the thread, approvals, stopping.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::PathBuf;
use std::sync::Arc;

use crate::orchestration::{ApprovalDecision, Delivery, FollowUpInput, TurnRequest};

use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::logging::{Data, Source, THREAD_LOG};
use crate::provider::AgentKind;
use crate::settings::LogLevel;
use crate::system::{now_ms, new_id};
use crate::threads::{Role, MessageStatus, StepStatus, Attachment, Message, GoalStatus};
use crate::{orchestration, provider, skills};
use crate::threads::title;
use crate::threads::{title_from, without_leading_attachments};
use crate::Server;

/// A message the user sent, complete from the start.
fn user_message(text: &str, attachments: Vec<Attachment>) -> Message {
    Message {
        id: new_id(),
        role: Role::User,
        text: text.into(),
        created_at: now_ms(),
        status: MessageStatus::Complete,
        tools: vec![],
        plan: vec![],
        files: vec![],
        error: None,
        approvals: vec![],
        attachments,
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

impl Server {
    /// Stops every running agent; for quitting the app.
    /// How many threads have an agent replying now.
    pub fn running_turns(&self) -> usize {
        self.runner.running().len()
    }

    /// Stops every running agent, and at the same time the commands Patchwork's own agent
    /// left running; for quitting the app.
    pub fn stop_agents(&self) {
        std::thread::scope(|s| {
            s.spawn(|| self.runner.jobs.stop_all(self));
            self.runner.stop_all();
        });
    }

    /// Replaces the `placeholder` title with one a model writes from `text`, in the background,
    /// unless the thread has been renamed meanwhile. With titles switched off in Settings, a
    /// slash command, or a message the model gives no title for, the first line names it. A
    /// message that is only attachments stays blank.
    fn name_thread(
        self: &Arc<Self>,
        thread_id: &str,
        text: &str,
        placeholder: String,
        agent: AgentKind,
        model: Option<String>,
    ) {
        let server = Arc::downgrade(self);
        let thread_id = thread_id.to_string();
        let text = text.to_string();
        let tokens = self.store.settings().tokens;
        let writer = title::Writer {
            model: tokens.title_model.clone(),
            local_only: tokens.titles_local_only,
        };
        let enabled = tokens.generate_titles;
        std::thread::spawn(move || {
            let first_line = title_from(&text);
            let generated = if !enabled || first_line.starts_with('/') || text.trim().is_empty() {
                None
            } else {
                title::generate(&text, agent, model.as_deref(), &writer)
            };
            let generated = generated.unwrap_or(first_line);
            let Some(server) = server.upgrade() else {
                return;
            };
            let mut renamed = false;
            let _ = server.store.update_thread(&thread_id, |t| {
                if t.title == placeholder {
                    t.title.clone_from(&generated);
                    renamed = true;
                }
            });
            if renamed {
                server.emit(AgentEventEnvelope {
                    thread_id,
                    event: AgentEvent::TitleUpdated { title: generated },
                });
            }
        });
    }

    /// Appends the user message, then starts the agent turn in the background.
    /// Returns the stored user message; assistant output arrives via the event sink.
    ///
    /// # Errors
    ///
    /// Returns an error when an attachment image cannot be read or is over 5 MB, when the chat's
    /// turn is still starting, when a `/goal` command is not allowed for the chat's agent or the
    /// current turn, or when the message cannot be saved or the turn cannot be started.
    pub fn send_message(
        self: &Arc<Self>,
        thread_id: String,
        text: String,
        attachments: Vec<Attachment>,
        queue: bool,
    ) -> Result<Message, Error> {
        let images = provider::load_images(&attachments)?;
        // Refused before anything is stored: a turn still starting has no agent to write to.
        if self.runner.is_starting(&thread_id) {
            return Err(crate::orchestration::turn::start::STILL_STARTING.into());
        }
        if let Some(command) = provider::goal_command(&text) {
            if self.runner.is_running(&thread_id) {
                return Err(
                    "A /goal command runs as a turn of its own; send it once this turn ends".into(),
                );
            }
            let agent = self.store.thread(&thread_id).map(|t| t.agent);
            if agent == Some(AgentKind::OpenCode) {
                return Err(
                    "OpenCode has no /goal command. Describe the task in a message instead.".into(),
                );
            }
            if agent == Some(AgentKind::Api) {
                return Err(
                    "API providers have no /goal command. Describe the task in a message instead."
                        .into(),
                );
            }
            if agent == Some(AgentKind::Antigravity) {
                if provider::antigravity::antigravity_bin().is_none() {
                    return Err("/goal needs the Antigravity CLI. Install it in Settings.".into());
                }
                // Antigravity's /goal only sets a goal, so any other word would become one.
                if !matches!(command, provider::GoalCommand::Set(_)) {
                    return Err("Antigravity's /goal only sets a goal: /goal <objective>. Clear a goal from the goal banner.".into());
                }
            }
            // Claude would take either word as a new goal it can never meet, and loop on it.
            let word = match command {
                provider::GoalCommand::Pause => Some("pause"),
                provider::GoalCommand::Resume => Some("resume"),
                _ => None,
            };
            if agent == Some(AgentKind::Claude)
                && let Some(word) = word
            {
                return Err(format!(
                    "Claude has no /goal {word}. Send a message to carry on with the goal, or /goal clear to drop it."
                ).into());
            }
            if matches!(
                command,
                provider::GoalCommand::Set(_) | provider::GoalCommand::Resume
            ) {
                // Only an explicit new goal or resume request can reopen a dismissed goal.
                // A provider restating cached goal state must not erase the dismissal.
                orchestration::goal::clear_dismissal(&self.store, &thread_id)?;
            }
        }
        // Antigravity has no setting for when it compacts, so a chat over its compact window
        // compacts first, and the message waits for it in the queue.
        let queue = if self.compacts_first(&thread_id, &text) {
            self.send_message(thread_id.clone(), "/compact".into(), Vec::new(), false)?;
            true
        } else {
            queue
        };
        // Checked before the message is stored, so a refused message leaves no trace.
        let mut msg = user_message(&text, attachments.clone());
        // Stored before the agent sees it, so its answer is stored after it.
        let m = msg.clone();
        let mut session_id = None;
        let mut subagents = Vec::new();
        let mut first_title = None;
        self.store.update_thread(&thread_id, |t| {
            if t.messages.is_empty() {
                first_title = Some((
                    t.title.clone(),
                    t.agent,
                    t.model.clone(),
                    t.project_id.clone(),
                ));
            }
            // Subagents of the running turn still working, which a steer must not cut off.
            subagents = t
                .messages
                .iter()
                .flat_map(|m| &m.tools)
                .filter(|c| {
                    c.subagent.is_some() && c.parent.is_none() && c.status == StepStatus::Running
                })
                .map(|c| c.id.clone())
                .collect();
            t.messages.push(m);
            session_id.clone_from(&t.session_id);
        })?;
        if let Some((placeholder, agent, model, project_id)) = first_title {
            let root = project_id.and_then(|id| {
                self.store
                    .settings()
                    .projects
                    .into_iter()
                    .find(|p| p.id == id)
                    .map(|p| PathBuf::from(p.path))
            });
            // Files the message opens with, by attachment or by a name in the project, name nothing.
            let subject = without_leading_attachments(&text, |word| {
                attachments.iter().any(|a| a.name == word)
                    || root.as_ref().is_some_and(|root| root.join(word).is_file())
            });
            self.name_thread(&thread_id, subject, placeholder, agent, model);
        }
        // A goal that has ended is over: the next message starts unrelated work, so its banner
        // goes. The agent has already dropped the goal, or `settle` told it to with a /goal
        // command, which is why those leave the banner alone.
        if !provider::is_goal_command(&text)
            && let Some(goal) = self.store.thread(&thread_id).and_then(|t| t.goal)
            && matches!(goal.status, GoalStatus::Complete | GoalStatus::Blocked)
        {
            self.store
                .dismiss_goal(&thread_id, &orchestration::goal::signature(&goal))?;
            orchestration::publish(self, &thread_id, AgentEvent::GoalUpdated { goal: None });
        }
        self.switch_to_agent_model(&thread_id, &text);
        // A /goal runs as a turn of its own, not in an idle agent's process.
        if provider::is_goal_command(&text) {
            self.runner.retire_idle(&thread_id);
        }
        // A message that names a skill as "$name" or summons an agent as "&name" anywhere reaches
        // the agent in the form it runs, as `start_turn` sends it too. Only a steer sends it
        // from here, since summoning can change the chat's agent.
        let mut switched = None;
        let mut steer_text = || {
            let skills = skills::enabled(&self.store.settings());
            let agent = self.store.with_threads(|threads| {
                threads.iter().find(|t| t.id == thread_id).map(|t| t.agent)
            });
            let text = match (agent, skills::invoked(&text, &skills)) {
                (Some(agent), Some((skill, rest))) => skills::invocation(agent, &skill, rest),
                _ => text.clone(),
            };
            let (text, role) = self.summon_agents(&thread_id, text, false);
            switched = Some(role);
            text
        };
        // An idle agent this message resumes counts the files its reply changes from now.
        self.runner.rebase_idle(&thread_id);
        // While a turn runs, the message steers it, or waits for it to end when the
        // agent cannot take it now.
        let delivery = if queue {
            match self.runner.enqueue(
                &thread_id,
                FollowUpInput {
                    message_id: &msg.id,
                    text: &text,
                    attachments: &attachments,
                    images: &images,
                    merged: &[],
                },
            ) {
                Ok(true) => Delivery::Queued,
                Ok(false) => self.runner.steer(
                    &thread_id,
                    FollowUpInput {
                        message_id: &msg.id,
                        text: &steer_text(),
                        attachments: &attachments,
                        images: &images,
                        merged: &[],
                    },
                    session_id.as_deref(),
                    &subagents,
                ),
                Err(error) => {
                    self.store.update_thread(&thread_id, |t| {
                        t.messages.retain(|queued| queued.id != msg.id);
                    })?;
                    return Err(error);
                }
            }
        } else {
            self.runner.steer(
                &thread_id,
                FollowUpInput {
                    message_id: &msg.id,
                    text: &steer_text(),
                    attachments: &attachments,
                    images: &images,
                    merged: &[],
                },
                session_id.as_deref(),
                &subagents,
            )
        };
        // A message that did not reach the running turn starts one, which summons again.
        if !matches!(delivery, Delivery::NotRunning)
            && let Some(switched) = switched
        {
            self.told_role(&thread_id, switched);
        }
        let delivered = match delivery {
            Delivery::Steered => "steered into the running reply",
            Delivery::Queued => "queued behind the running reply",
            Delivery::Resumed => "sent to the idle agent",
            Delivery::NotRunning => "starting a reply",
        };
        let source = Source {
            thread: Some(thread_id.clone()),
            message: Some(msg.id.clone()),
            ..Source::default()
        };
        let note = format!("{delivered}, {} attachments", attachments.len());
        THREAD_LOG.write(LogLevel::Info, "message sent", &source, Data::Text(note));
        match delivery {
            Delivery::Steered => {
                msg.steer = true;
                self.store.update_thread(&thread_id, |t| {
                    if let Some(m) = t.messages.iter_mut().find(|m| m.id == msg.id) {
                        m.steer = true;
                    }
                })?;
            }
            Delivery::Queued => {
                // Only ever raised here: a turn that took the message from its queue meanwhile
                // has flagged it itself, and whoever sends it lowers the flag.
                self.store.update_thread(&thread_id, |t| {
                    if let Some(m) = t.messages.iter_mut().find(|m| m.id == msg.id) {
                        m.queued |= self.runner.is_queued(&thread_id, &m.id);
                        msg.queued = m.queued;
                    }
                })?;
            }
            Delivery::Resumed => self.capture_checkpoint(&thread_id, &msg.id),
            Delivery::NotRunning => {
                // The app adds a message it sent once this returns, but not one the server sends
                // itself (a goal clear), so the chat learns of it before the reply starts.
                self.emit(AgentEventEnvelope {
                    thread_id: thread_id.clone(),
                    event: AgentEvent::MessageStarted {
                        message: msg.clone(),
                    },
                });
                self.runner.start_turn(
                    self,
                    TurnRequest {
                        thread_id,
                        text,
                        attachments,
                        images,
                    },
                )?;
            }
        }
        Ok(msg)
    }

    /// Whether `text` should wait for an Antigravity chat to compact first: no turn is running,
    /// and the context its last reply reported, with no /compact since, reached the window.
    fn compacts_first(&self, thread_id: &str, text: &str) -> bool {
        let Some(thread) = self.store.thread(thread_id) else {
            return false;
        };
        if thread.agent != AgentKind::Antigravity
            || orchestration::is_upkeep(text)
            || provider::is_goal_command(text)
            || self.runner.is_running(thread_id)
        {
            return false;
        }
        let window = thread
            .compact_window
            .unwrap_or_else(|| self.store.settings().tokens.compact_window(thread.agent));
        if window == 0 {
            return false;
        }
        for m in thread.messages.iter().rev() {
            if m.role == Role::User && m.text.trim() == "/compact" {
                return false;
            }
            if let Some(usage) = &m.usage {
                return usage.context >= window;
            }
        }
        false
    }

    /// Stops the running turn by interrupting the agent, so its session records the stop and an
    /// agent that can stays up for the next message. Codex is first told to pause the goal it is
    /// pursuing, so it does not carry on with it later.
    pub fn cancel_turn(&self, thread_id: &str) {
        THREAD_LOG.text(
            LogLevel::Info,
            thread_id,
            "chat",
            "stopped by the person".into(),
        );
        self.stop_turn(thread_id, true);
    }

    /// Stops the running turn as `cancel_turn` does; without `interrupt`, the agent process ends
    /// with it.
    pub(crate) fn stop_turn(&self, thread_id: &str, interrupt: bool) {
        let thread = self.store.thread(thread_id);
        let session_id = thread.as_ref().and_then(|t| t.session_id.clone());
        let pause = thread.and_then(|t| {
            let active = t.goal.is_some_and(|g| g.status == GoalStatus::Active);
            provider::goal_pause(t.agent, t.session_id.as_deref()?).filter(|_| active)
        });
        if interrupt {
            self.runner
                .interrupt(thread_id, session_id.as_deref(), pause);
        } else {
            self.runner.cancel(thread_id, pause);
        }
    }

    /// Stops the subagent that the call `tool_id` started.
    ///
    /// # Errors
    ///
    /// Returns an error when the subagent's command already finished, when the chat has no running
    /// turn, when the subagent cannot be stopped, or when the turn no longer accepts input.
    pub fn stop_subagent(&self, thread_id: &str, tool_id: &str) -> Result<(), Error> {
        self.runner.stop_subagent(thread_id, tool_id)
    }

    /// Answers a pending approval in the thread's running turn.
    ///
    /// # Errors
    ///
    /// Returns an error when no turn is waiting for the approval and the request was already
    /// answered, or when sending the answer in a new turn fails.
    pub fn respond_approval(
        self: &Arc<Self>,
        thread_id: &str,
        approval_id: &str,
        allow: bool,
        answers: std::collections::HashMap<String, Vec<String>>,
        rule: Option<crate::permissions::RuleRef>,
    ) -> Result<(), Error> {
        self.runner.respond(
            self,
            thread_id,
            ApprovalDecision {
                approval_id,
                allow,
                answers,
                rule,
            },
        )
    }
}
