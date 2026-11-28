//! Sending messages and running turns: naming the chat, approvals, stopping.

// Rust guideline compliant 2026-10-09

use crate::chats::{MessageExt};
use crate::Error;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::orchestration::turn::start::Starting;
use crate::orchestration::{ApprovalDecision, Delivery, FollowUpInput, TurnRequest};

use crate::events::{ChatEvent, ChatEventEnvelope};
use crate::logging::{Data, Source, CHAT_LOG};
use crate::provider::AgentKind;
use crate::settings::LogLevel;
use crate::chats::{
    Role, MessageStatus, StepStatus, Attachment, Message, GoalStatus, BackgroundTask, Chat,
    Approval, ApprovalStatus,
};
use crate::extensions::skills;
use crate::{orchestration, provider};
use crate::chats::title;
use crate::chats::{title_from, without_leading_attachments};
use crate::Core;
use crate::gateway::Platform;

/// A message the user sent, complete from the start.
fn user_message(text: &str, attachments: Vec<Attachment>) -> Message {
    Message {
        attachments,
        ..Message::new(Role::User, text.into(), MessageStatus::Complete)
    }
}

/// Closes the questions in `chat` the agent ended its reply on, which the message being sent
/// answers instead. Returns each one's message id and approval id.
fn answer_open_questions(chat: &mut Chat) -> Vec<(String, String)> {
    let mut answered = Vec::new();
    for message in &mut chat.messages {
        let open = |a: &Approval| a.by_message && a.status == ApprovalStatus::Pending;
        if !message.approvals.iter().any(open) {
            continue;
        }
        let message = Arc::make_mut(message);
        for approval in message.approvals.iter_mut().filter(|a| open(a)) {
            approval.status = ApprovalStatus::Denied;
            answered.push((message.id.clone(), approval.id.clone()));
        }
    }
    answered
}

impl Core {
    /// How many chats have an agent replying now.
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
    /// unless the chat has been renamed meanwhile. With titles switched off in Settings, a
    /// slash command, or a message the model gives no title for, the first line names it. A
    /// message that is only attachments stays blank.
    fn name_chat(
        self: &Arc<Self>,
        chat_id: &str,
        text: &str,
        placeholder: String,
        agent: AgentKind,
        model: Option<String>,
    ) {
        let core = Arc::downgrade(self);
        let chat_id = chat_id.to_string();
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
            let Some(core) = core.upgrade() else {
                return;
            };
            let mut renamed = false;
            let saved = core.store.update_chat(&chat_id, |t| {
                if t.title == placeholder {
                    t.title.clone_from(&generated);
                    renamed = true;
                }
            });
            if let Err(e) = &saved {
                log::warn!(chat:% = chat_id, error:% = e; "could not save the title of {{chat}}: {{error}}");
            }
            if renamed && saved.is_ok() {
                core.emit(ChatEventEnvelope {
                    chat_id,
                    event: ChatEvent::TitleUpdated { title: generated },
                });
            }
        });
    }

    /// Whether the chat has no reply running and is set to resume its last one when its agent's
    /// usage limit resets.
    fn waits_to_resume(&self, chat_id: &str) -> bool {
        !self.runner.is_running(chat_id)
            && self.limit_resume_at(chat_id).is_some()
            && self
                .store
                .chat(chat_id)
                .is_some_and(|t| t.resumable_reply().is_some())
    }

    /// Stores the message as a queued one, which goes to the agent once the reply the chat is
    /// waiting to resume has finished, or sooner when the user sends the queue.
    fn hold_for_resume(
        &self,
        chat_id: &str,
        text: &str,
        attachments: Vec<Attachment>,
        via: Option<Platform>,
    ) -> Result<Message, Error> {
        let mut msg = user_message(text, attachments);
        msg.via = via;
        msg.queued = true;
        let held = msg.clone();
        self.store
            .update_chat(chat_id, |t| t.messages.push(Arc::new(held)))?;
        let source = Source {
            chat: Some(chat_id.to_string()),
            message: Some(msg.id.clone()),
            ..Source::default()
        };
        let note = "queued until the reply waiting on a usage limit resumes";
        CHAT_LOG.write(
            LogLevel::Info,
            "message sent",
            &source,
            Data::Text(note.into()),
        );
        Ok(msg)
    }

    /// Appends the user message, then starts the agent turn in the background.
    /// Returns the stored user message; assistant output arrives via the event sink.
    ///
    /// # Errors
    ///
    /// Returns an error when an attachment image cannot be read or is over 5 MB, when the chat's
    /// turn is still starting, when a `/goal` command is not allowed for the chat's agent or the
    /// current turn, or when the message cannot be saved or the turn cannot be started.
    pub(crate) fn send_message(
        self: &Arc<Self>,
        chat_id: String,
        text: String,
        attachments: Vec<Attachment>,
        queue: bool,
    ) -> Result<Message, Error> {
        self.send_message_from(chat_id, text, attachments, queue, None)
    }

    /// Sends the message as `send_message` does, from the chat app conversation `from` when it
    /// came from one, and shows it in the other chat apps the chat is continued on.
    ///
    /// # Errors
    ///
    /// Returns an error when the message cannot be delivered, as `send_message` does.
    pub fn send_message_from(
        self: &Arc<Self>,
        chat_id: String,
        text: String,
        attachments: Vec<Attachment>,
        queue: bool,
        from: Option<(Platform, &str)>,
    ) -> Result<Message, Error> {
        self.back_before_sending(&chat_id);
        let msg = self.deliver(
            chat_id.clone(),
            text,
            attachments,
            queue,
            from.map(|(platform, _)| platform),
        )?;
        self.mirror_to_chat_apps(chat_id, &msg, from);
        Ok(msg)
    }

    /// Stores the user message and gives it to the agent; see `send_message`.
    fn deliver(
        self: &Arc<Self>,
        chat_id: String,
        text: String,
        attachments: Vec<Attachment>,
        queue: bool,
        via: Option<Platform>,
    ) -> Result<Message, Error> {
        let mut images = provider::load_images(&attachments)?;
        // Refused before anything is stored: a turn still starting has no agent to write to.
        if self.runner.is_starting(&chat_id) {
            return Err(crate::orchestration::turn::start::STILL_STARTING.into());
        }
        // A chat waiting for its usage limit to reset holds the message for after the reply it
        // is waiting to resume. Sent now it would start a turn of its own, the chat would move
        // past that reply, and the wait would be dropped.
        if !provider::is_goal_command(&text) && self.waits_to_resume(&chat_id) {
            return self.hold_for_resume(&chat_id, &text, attachments, via);
        }
        // Patchwork runs the goal itself, the same for every agent: a pause or a clear is
        // answered here, and a new goal or a resume is sent as the goal's brief.
        let goal_local = provider::goal_command(&text).and_then(|command| match command {
            provider::GoalCommand::Show => Some(Err(Error::from(
                "Name the goal after /goal, or pause, resume or clear it",
            ))),
            provider::GoalCommand::Clear => Some(self.stop_goal(&chat_id)),
            provider::GoalCommand::Pause => {
                self.pause_goal(&chat_id);
                Some(Ok(()))
            }
            provider::GoalCommand::Set(_) | provider::GoalCommand::Resume => None,
        });
        if let Some(done) = goal_local {
            done?;
            let msg = user_message(&text, Vec::new());
            let m = msg.clone();
            self.store
                .update_chat(&chat_id, |t| t.messages.push(Arc::new(m)))?;
            self.emit(ChatEventEnvelope {
                chat_id,
                event: ChatEvent::MessageStarted {
                    message: msg.clone(),
                },
            });
            return Ok(msg);
        }
        // A goal set or resumed while a turn runs is pursued from now: its brief steers the turn,
        // or waits in the queue, and the turn's end carries it on.
        if let Some(command) = provider::goal_command(&text) {
            let current = self.store.chat(&chat_id).and_then(|t| t.goal);
            let goal = if let provider::GoalCommand::Set(objective) = command {
                orchestration::goal::new_goal(current.as_ref(), objective)
            } else {
                let goal = current.ok_or("There is no goal to resume")?;
                goal.with_status(GoalStatus::Active, crate::system::now_ms())
            };
            // Only an explicit new goal or resume reopens a dismissed goal.
            orchestration::goal::clear_dismissal(&self.store, &chat_id)?;
            orchestration::publish(self, &chat_id, ChatEvent::GoalUpdated { goal: Some(goal) });
        }
        // Antigravity has no setting for when it compacts, so a chat over its compact window
        // compacts first, and the message waits for it in the queue. A message sent while it
        // compacts waits too: steering would stop the compaction.
        let queue = if self.compacts_first(&chat_id, &text) {
            self.deliver(chat_id.clone(), "/compact".into(), Vec::new(), false, None)?;
            true
        } else {
            queue || self.compacting(&chat_id)
        };
        // Checked before the message is stored, so a refused message leaves no trace.
        let mut msg = user_message(&text, attachments.clone());
        msg.via = via;
        // Stored before the agent sees it, so its answer is stored after it.
        let m = msg.clone();
        let mut session_id = None;
        let mut subagents = Vec::new();
        let mut first_title = None;
        let mut answered = Vec::new();
        self.store.update_chat(&chat_id, |t| {
            answered = answer_open_questions(t);
            // A sub agent's chat is named after what it was asked to do when it is made.
            if t.messages.is_empty() && t.parent.is_none() {
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
            t.messages.push(Arc::new(m));
            session_id.clone_from(&t.session_id);
        })?;
        // The agent takes this message as the answer to the question it ended its reply on, and
        // Antigravity refuses images in an answer. The text still names each image's path.
        if !answered.is_empty() {
            images.clear();
        }
        for (message_id, approval_id) in answered {
            orchestration::publish(
                self,
                &chat_id,
                ChatEvent::ApprovalResolved {
                    message_id,
                    approval_id,
                    allowed: false,
                    answers: HashMap::new(),
                    rule: None,
                    auto: false,
                },
            );
        }
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
            self.name_chat(&chat_id, subject, placeholder, agent, model);
        }
        // A goal that has ended is over: the next message starts unrelated work, so its banner
        // goes. A new /goal replaces it instead.
        if !provider::is_goal_command(&text)
            && let Some(goal) = self.store.chat(&chat_id).and_then(|t| t.goal)
            && matches!(goal.status, GoalStatus::Complete | GoalStatus::Blocked)
        {
            self.store
                .dismiss_goal(&chat_id, &orchestration::goal::signature(&goal))?;
            orchestration::publish(self, &chat_id, ChatEvent::GoalUpdated { goal: None });
        }
        self.switch_to_agent_model(&chat_id, &text);
        // A /goal runs as a turn of its own, not in an idle agent's process.
        if provider::is_goal_command(&text) {
            self.runner.retire_idle(&chat_id);
        }
        // A message that names a skill as "$name" or summons an agent as "&name" anywhere reaches
        // the agent in the form it runs, as `start_turn` sends it too. Only a steer sends it
        // from here, since summoning can change the chat's agent.
        let mut switched = None;
        let mut steer_text = || {
            let skills = skills::enabled(&self.store.settings());
            let agent = self
                .store
                .with_chats(|chats| chats.iter().find(|t| t.id == chat_id).map(|t| t.agent));
            let text = orchestration::goal::agent_text(&self.store, &chat_id, text.clone());
            let text = match (agent, skills::invoked(&text, &skills)) {
                (Some(agent), Some((skill, rest))) => skills::invocation(agent, &skill, rest),
                _ => text,
            };
            let (text, role) = self.summon_agents(&chat_id, text, false);
            switched = Some(role);
            self.chat_app_context(&chat_id, text)
        };
        // An idle agent this message resumes counts the files its reply changes from now.
        self.runner.rebase_idle(&chat_id);
        // While a turn runs, the message steers it, or waits for it to end when the
        // agent cannot take it now.
        let delivery = if queue
            && self.runner.enqueue(
                &chat_id,
                FollowUpInput {
                    message_id: &msg.id,
                    text: &text,
                    attachments: &attachments,
                    images: &images,
                    merged: &[],
                },
            ) {
            Delivery::Queued
        } else {
            self.runner.steer(
                &chat_id,
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
            self.told_role(&chat_id, switched);
        }
        let delivered = match delivery {
            Delivery::Steered => "steered into the running reply",
            Delivery::Queued => "queued behind the running reply",
            Delivery::Resumed => "sent to the idle agent",
            Delivery::NotRunning => "starting a reply",
        };
        let source = Source {
            chat: Some(chat_id.clone()),
            message: Some(msg.id.clone()),
            ..Source::default()
        };
        let note = format!("{delivered}, {} attachments", attachments.len());
        CHAT_LOG.write(LogLevel::Info, "message sent", &source, Data::Text(note));
        match delivery {
            Delivery::Steered => {
                msg.steer = true;
                self.store.update_chat(&chat_id, |t| {
                    if let Some(m) = t
                        .messages
                        .iter_mut()
                        .find(|m| m.id == msg.id)
                        .map(Arc::make_mut)
                    {
                        m.steer = true;
                    }
                })?;
            }
            Delivery::Queued => {
                // Only ever raised here: a turn that took the message from its queue meanwhile
                // has flagged it itself, and whoever sends it lowers the flag.
                self.store.update_chat(&chat_id, |t| {
                    if let Some(m) = t
                        .messages
                        .iter_mut()
                        .find(|m| m.id == msg.id)
                        .map(Arc::make_mut)
                    {
                        m.queued |= self.runner.is_queued(&chat_id, &m.id);
                        msg.queued = m.queued;
                    }
                })?;
            }
            Delivery::Resumed => self.capture_checkpoint(&chat_id, &msg.id),
            Delivery::NotRunning => {
                // Another send may have started the turn since the check at the top. This one is
                // refused then, and its message taken back out, so it leaves no trace.
                let starting = match Starting::claim(&self.runner, &chat_id) {
                    Ok(starting) => starting,
                    Err(e) => {
                        self.store.update_chat(&chat_id, |t| {
                            t.messages.retain(|m| m.id != msg.id);
                        })?;
                        return Err(e);
                    }
                };
                // The app adds a message it sent once this returns, but not one the core sends
                // itself (carrying a goal on), so the chat learns of it before the reply starts.
                self.emit(ChatEventEnvelope {
                    chat_id: chat_id.clone(),
                    event: ChatEvent::MessageStarted {
                        message: msg.clone(),
                    },
                });
                self.runner.start_claimed(
                    self,
                    TurnRequest {
                        chat_id,
                        text,
                        attachments,
                        images,
                        resumes: false,
                    },
                    starting,
                )?;
            }
        }
        Ok(msg)
    }

    /// Whether `text` should wait for an Antigravity chat to compact first: no turn is running,
    /// and the context its last reply reported, with no /compact since, reached the window.
    fn compacts_first(&self, chat_id: &str, text: &str) -> bool {
        let Some(chat) = self.store.chat(chat_id) else {
            return false;
        };
        if chat.agent != AgentKind::Antigravity
            || orchestration::is_upkeep(text)
            || provider::is_goal_command(text)
            || self.runner.is_running(chat_id)
        {
            return false;
        }
        let window = chat
            .compact_window
            .unwrap_or_else(|| self.store.settings().tokens.compact_window(chat.agent));
        if window == 0 || compact_failed(&chat) {
            return false;
        }
        chat.current_context().is_some_and(|c| c >= window)
    }

    /// Whether the Antigravity reply under way is compacting (either `/compact` or Patchwork's
    /// mid-turn compaction). A steer would stop it, so messages sent meanwhile wait in the queue.
    pub(crate) fn compacting(&self, chat_id: &str) -> bool {
        if self.runner.is_compacting(chat_id) {
            return true;
        }
        let Some(chat) = self.store.chat(chat_id) else {
            return false;
        };
        chat.agent == AgentKind::Antigravity
            && self.runner.is_running(chat_id)
            && chat
                .messages
                .iter()
                .rev()
                .find(|m| m.role == Role::User && !m.queued)
                .is_some_and(|m| {
                    orchestration::upkeep(&m.text) == Some(orchestration::Upkeep::Compact)
                })
    }

    /// Stops the running turn by interrupting the agent, so its session records the stop and an
    /// agent that can stays up for the next message.
    pub fn cancel_turn(&self, chat_id: &str) {
        CHAT_LOG.text(
            LogLevel::Info,
            chat_id,
            "chat",
            "stopped by the person".into(),
        );
        self.stop_turn(chat_id, true);
    }

    /// Stops the running turn as `cancel_turn` does; without `interrupt`, the agent process ends
    /// with it. The commands Patchwork runs for the chat's agent, which outlive its process as
    /// background jobs, stop with it.
    pub(crate) fn stop_turn(&self, chat_id: &str, interrupt: bool) {
        let session_id = self.store.chat(chat_id).and_then(|t| t.session_id);
        if interrupt {
            self.runner.interrupt(chat_id, session_id.as_deref());
        } else {
            self.runner.cancel(chat_id);
        }
        // After the turn is marked stopped, so a command it starts meanwhile stops itself.
        self.runner.stop_jobs(chat_id);
    }

    /// Drops the note on chat `chat_id`'s background task `tool_id` that Patchwork closing ended
    /// it, once the user has dismissed it or asked for it again.
    ///
    /// # Errors
    ///
    /// Returns an error when the change cannot be saved.
    pub fn dismiss_cut_off_task(&self, chat_id: &str, tool_id: &str) -> Result<(), Error> {
        let cut_off = |task: &BackgroundTask| task.cut_off && task.tool_id == tool_id;
        self.store.update_chat(chat_id, |t| {
            for message in &mut t.messages {
                if message.background.iter().any(cut_off) {
                    for task in &mut Arc::make_mut(message).background {
                        if cut_off(task) {
                            task.cut_off = false;
                        }
                    }
                }
            }
        })
    }

    /// Answers a pending approval in the chat's running turn.
    ///
    /// # Errors
    ///
    /// Returns an error when no turn is waiting for the approval and the request was already
    /// answered, or when sending the answer in a new turn fails.
    pub fn respond_approval(
        self: &Arc<Self>,
        chat_id: &str,
        approval_id: &str,
        allow: bool,
        answers: std::collections::HashMap<String, Vec<String>>,
        rule: Option<crate::permissions::RuleRef>,
    ) -> Result<(), Error> {
        self.runner.respond(
            self,
            chat_id,
            ApprovalDecision {
                approval_id,
                allow,
                answers,
                rule,
            },
        )
    }
}

/// Whether the chat's last `/compact` failed and no reply has reported the context since. Asking
/// again would fail the same way before every message, so the next one goes without it.
fn compact_failed(chat: &Chat) -> bool {
    let Some(at) = chat
        .messages
        .iter()
        .rposition(|m| m.role == Role::User && !m.queued && provider::is_compact_command(&m.text))
    else {
        return false;
    };
    let mut replies = chat.messages[at + 1..]
        .iter()
        .filter(|m| m.role == Role::Assistant);
    let failed = replies
        .next()
        .is_some_and(|m| matches!(m.status, MessageStatus::Error | MessageStatus::Interrupted));
    failed && !replies.any(|m| m.usage.as_ref().is_some_and(|u| u.context > 0))
}
