//! What each event the provider parsed from a line does to the turn.

// Rust guideline compliant 2026-10-09

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde_json::Value;

use super::approvals;
use super::compaction::After;
use super::idle::{can_idle, stays_up};
use super::read::{Commands, Reading, close_steered_segment};
use super::unprompted::Unprompted;
use crate::chats::{Attachment, TokenUsage};
use crate::events::ChatEvent;
use crate::orchestration::messages::queue::FollowUp;
use crate::orchestration::output::usage::{TURN_OUTPUT, clear_context, message_usage, record_request};
use crate::orchestration::turn::process::{terminate, with_remedy, write_line, write_to};
use crate::orchestration::turn::start::Turn;
use crate::provider::{self, Parsed, Steer, TaskEvent};
use crate::system::Locked;

/// Writes `item`, a `Send` or `SendAfterCommands`, to the agent, or keeps it until the agent
/// has listed its slash commands.
fn send(turn: &Turn, commands: &mut Commands, item: Parsed) {
    match (item, commands) {
        (Parsed::Send(line), _) | (Parsed::SendAfterCommands(line), Commands::Listed) => {
            write_line(&turn.stdin, &line);
        }
        (Parsed::SendAfterCommands(line), Commands::Unlisted(waiting)) => *waiting = Some(line),
        _ => {}
    }
}

/// Sends the steering messages an agent stopped its reply for, as one prompt that starts its
/// next reply. False when there were none or the agent can no longer take them, in which case
/// they wait as queued messages for the turn's end to send.
fn send_steers(turn: &Turn) -> bool {
    // Under the stdin lock, as `steer` writes its stop, so a steer either goes in this prompt or
    // stops the reply to it and goes in the next.
    let mut stdin = turn.stdin.locked();
    let steers: Vec<FollowUp> = turn.steers.locked().drain(..).collect();
    turn.cancelling.store(false, Ordering::SeqCst);
    if steers.is_empty() {
        return false;
    }
    let text = steers
        .iter()
        .map(|f| f.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let attachments: Vec<Attachment> = steers
        .iter()
        .flat_map(|f| f.attachments.iter().cloned())
        .collect();
    let images: Vec<provider::Image> = steers
        .iter()
        .flat_map(|f| f.images.iter().cloned())
        .collect();
    let prompt = provider::Prompt {
        text: &text,
        attachments: &attachments,
        images: &images,
    };
    let session_id = turn.session_id.as_deref();
    let line = provider::resume_input(turn.agent, prompt, session_id, None).map(|line| {
        let mode = *turn.mode.locked();
        turn.settings
            .locked()
            .first_line(turn.agent, mode, session_id, line)
    });
    if line.is_some_and(|line| write_to(&mut stdin, &line)) {
        return true;
    }
    drop(stdin);
    turn.queue.locked().extend(steers);
    false
}

impl Reading {
    /// Hands one parsed event to what handles it. `value` is the line it came from.
    pub(super) fn dispatch(&mut self, item: Parsed, value: &Value) {
        let Some(item) = approvals::screen(&self.turn, item) else {
            return;
        };
        match item {
            item @ (Parsed::Send(_) | Parsed::SendAfterCommands(_)) => {
                send(&self.turn, &mut self.commands, item);
            }
            Parsed::SendInOrder(lines) => {
                let mut lines = VecDeque::from(lines);
                let first = lines.pop_front();
                self.turn.settings.locked().chain = lines;
                if let Some(first) = first {
                    send(&self.turn, &mut self.commands, first);
                }
            }
            Parsed::SendNext => {
                let next = self.turn.settings.locked().chain.pop_front();
                if let Some(next) = next {
                    send(&self.turn, &mut self.commands, next);
                }
            }
            Parsed::AgentConfig(config) => {
                self.turn.settings.locked().reported = config;
                // The model may have changed, and prices what follows.
                self.fallback_model = None;
            }
            Parsed::AgentMode(mode) => {
                // So the next reply sets the chat's own mode again.
                let mut settings = self.turn.settings.locked();
                let option = settings.reported.as_array_mut().and_then(|options| {
                    options
                        .iter_mut()
                        .find(|opt| opt["id"].as_str() == Some("mode"))
                });
                if let Some(option) = option {
                    option["currentValue"] = Value::String(mode);
                }
            }
            Parsed::McpCall {
                id,
                server,
                tool,
                arguments,
            } => approvals::call_tool(&self.turn, id, server, tool, arguments),
            Parsed::Approval {
                title,
                detail,
                reply,
                questions,
                target,
            } => approvals::request_approval(
                &self.turn,
                &mut self.unsaved,
                title,
                detail,
                reply,
                questions,
                target,
            ),
            // Answered above.
            Parsed::Check { .. } => {}
            Parsed::Finished => self.reply_finished(value),
            Parsed::ControlResponse(id) => {
                let waiting = self.turn.controls.locked().remove(&id);
                if let Some(waiting) = waiting {
                    let _ = waiting.send(());
                }
            }
            Parsed::Steered => self.steered(),
            Parsed::SteerRejected(message) => self.steer_rejected(&message),
            Parsed::SessionId(id) => self.session_started(id),
            Parsed::SlashCommands(listed) => {
                self.turn
                    .publish(ChatEvent::SlashCommandsUpdated { commands: listed });
                if let Commands::Unlisted(Some(line)) =
                    std::mem::replace(&mut self.commands, Commands::Listed)
                {
                    write_line(&self.turn.stdin, &line);
                }
            }
            Parsed::Task(event) => self.task(event),
            Parsed::Usage(windows) => self.turn.core.usage.record(self.turn.agent, windows),
            Parsed::RequestUsage {
                request_id,
                usage,
                top_level,
                model,
            } => self.request_usage(request_id, usage, top_level, model),
            Parsed::TurnOutput(total) => self.turn_output(total),
            Parsed::TurnStarted(id) => {
                self.ended = false;
                *self.turn.native.locked() = Some(id);
            }
            // Patchwork shows its own compaction of the reply already.
            Parsed::Compacting | Parsed::Compacted(_)
                if self.caps.patchwork_compacts && self.compaction.is_compacting() => {}
            Parsed::Compacting => {
                self.turn.save_text(&mut self.unsaved);
                self.turn.publish(ChatEvent::Compacting {
                    message_id: self.turn.message_id.clone(),
                });
            }
            Parsed::Compacted(summary) => {
                self.turn.publish(ChatEvent::Compacted {
                    message_id: self.turn.message_id.clone(),
                    summary,
                });
            }
            Parsed::Retrying {
                reason,
                attempt,
                max_attempts,
            } => {
                self.turn.emit(ChatEvent::Retrying {
                    message_id: self.turn.message_id.clone(),
                    reason,
                    attempt,
                    max_attempts,
                });
            }
            Parsed::Error(message) => {
                self.stream_error.get_or_insert(message);
            }
            Parsed::SessionLost => self.session_lost = true,
            Parsed::Replaying(on) => self.replaying = on,
            // A reply Stop interrupted ends as stopped, not as cut short by a steering message.
            Parsed::Event(ChatEvent::MessageCutOff { .. })
                if self.turn.cancelled.load(Ordering::SeqCst) => {}
            Parsed::Event(event) => self.event(event),
        }
    }

    /// The agent ended its reply: the turn goes idle, answers steers, compacts or ends.
    fn reply_finished(&mut self, value: &Value) {
        self.ended = true;
        // A refused setting ends the reply before its prompt is sent.
        self.turn.settings.locked().chain.clear();
        // A call the finished reply left open runs in the background, if at all.
        self.open_calls.clear();
        self.unprompted.set(Unprompted::No, &self.turn.alone);
        if self.turn.cancelled.load(Ordering::SeqCst) && self.turn.interrupts.waiting() {
            self.stopped();
            return;
        }
        let was_compacting = self.compaction.is_compacting();
        let after_compaction = self.compaction.finished(
            &self.turn,
            value,
            self.stream_error.is_some(),
            &mut self.unsaved,
        );
        if was_compacting
            && !self.compaction.is_compacting()
            && self.stream_error.is_none()
            && !self.turn.cancelled.load(Ordering::SeqCst)
        {
            // Drop the pre-compaction context size so the UI clears any over-window
            // warning immediately while waiting for the next post-compaction update.
            clear_context(&mut self.requests, &self.turn.message_id);
            self.turn.publish(ChatEvent::Usage {
                usage: message_usage(&self.requests, &self.turn.message_id),
                message_id: self.turn.message_id.clone(),
            });
        }
        match after_compaction {
            After::Compact(line) => {
                match &mut self.commands {
                    Commands::Listed => {
                        write_line(&self.turn.stdin, &line);
                    }
                    Commands::Unlisted(waiting) => *waiting = Some(line),
                }
                self.ended = false;
                return;
            }
            After::GoesOn => {
                self.ended = false;
                return;
            }
            After::Ends => {}
        }
        if self.answers_steers() {
            // An error from the cancelled prompt does not carry over to the steer.
            self.stream_error = None;
            close_steered_segment(&self.turn, &self.segments, &mut self.completed);
            self.start_segment();
            // Its answer ends with another result; stdin stays open for more.
            self.ended = false;
            return;
        }
        // The finished turn can no longer be steered; until Codex starts the
        // next one, messages wait for the run to end.
        *self.turn.native.locked() = None;
        // The calls still open are what the turn left running, or commands the agent moved to
        // the background, so they are listed under the background now.
        if self.caps.lists_open_calls {
            for event in self.background.list_unreported(&self.turn.message_id) {
                self.publish_event(event);
            }
        }
        // An agent that stays up idles for the next message, so its session and connectors are
        // not set up again for each one. It takes messages meanwhile, including while a task it
        // started keeps running.
        if can_idle(&self.turn, self.goal.as_ref(), self.stream_error.as_ref()) {
            self.idle(false, None);
        } else if !self.background.keeps_stdin_open() {
            self.turn.close_unless_steered();
        } else if can_idle(&self.turn, self.goal.as_ref(), None) {
            // A reply that failed while a command it started runs on ends now, with its
            // error. The agent stays up with the command, as after a reply that did not fail,
            // since the command would otherwise hold the reply open until it ends.
            let error = self
                .stream_error
                .take()
                .map(|e| with_remedy(self.turn.agent, e));
            self.idle(false, error.as_deref());
        }
    }

    /// Stop interrupted the agent and it ended its reply.
    ///
    /// An agent that stays up does so for the next message, as after any reply; one that exits
    /// once its stdin closes has it closed. Until the agent is idle or has exited, Stop still
    /// sends SIGTERM if it hangs. Work held open out of the stream stops with its agent, so one
    /// Stop ends both.
    fn stopped(&mut self) {
        if stays_up(&self.turn, self.goal.as_ref()) && !self.reader.is_holding() {
            self.turn.interrupts.answer();
            // An error in the stopped reply does not carry over to the next one.
            self.stream_error = None;
            self.idle(true, None);
        } else if self.caps.exits_on_stdin_close {
            self.turn.stdin.locked().take();
        } else {
            // What it left running toward a goal or in the background stops too.
            terminate(self.pid, Arc::clone(&self.turn.exited));
        }
        self.compaction.stopped(&self.turn);
    }

    /// Whether the finished reply makes way for the steering messages sent during it.
    ///
    /// An agent that ends its reply to take them answers them next. One whose reply was stopped
    /// for them is sent them as its next prompt.
    fn answers_steers(&self) -> bool {
        let steered = !self.turn.steers.locked().is_empty();
        steered
            && match self.caps.steer {
                Steer::EndsReply => {
                    self.turn.steers.locked().clear();
                    true
                }
                Steer::StopsReply => send_steers(&self.turn),
                Steer::Inline => false,
            }
    }

    /// The agent took a steering message into its reply.
    fn steered(&mut self) {
        self.turn.steers.locked().pop_front();
        if !self.steer_reply_empty {
            // The reply the steer arrived during is over; the turn goes on below it.
            close_steered_segment(&self.turn, &self.segments, &mut self.completed);
            self.start_segment();
            self.steer_reply_empty = true;
        }
        if self.ended {
            self.turn.close_unless_steered();
        }
    }

    /// The agent refused a steering message, which waits for the turn's end instead.
    fn steer_rejected(&mut self, message: &str) {
        log::warn!(
            message:%;
            "the agent did not take the steering message: {{message}}"
        );
        let refused = self.turn.steers.locked().pop_front();
        if let Some(refused) = refused {
            // Waits as a queued message, so the turn's end sends it on.
            self.turn.publish(ChatEvent::MessageQueued {
                message_id: refused.message_id.clone(),
                queued: true,
            });
            self.turn.queue.locked().push(refused);
        }
        if self.ended {
            self.turn.close_unless_steered();
        }
    }

    /// The agent named its session, which later turns resume.
    fn session_started(&mut self, id: String) {
        let store = &self.turn.core.store;
        if let Err(e) = store.update_chat(&self.turn.chat_id, |t| t.session_id = Some(id.clone())) {
            log::warn!(error:% = e; "failed to save session id: {{error}}");
        }
        // The session has gone on from an undo's or a fork's point, and a later
        // turn resumes where it is now.
        if self.caps.resume_points
            && let Err(e) = self.turn.core.set_resume_point(&self.turn.chat_id, None)
        {
            log::warn!(error:% = e; "failed to forget a resume point: {{error}}");
        }
        self.turn.session_id = Some(id);
    }

    /// A background task started, changed or ended.
    fn task(&mut self, event: TaskEvent) {
        for event in self.background.apply(event, &self.turn.message_id) {
            self.publish_event(event);
        }
        // A reply that has ended goes idle, or closes stdin, once no task holds it open.
        if self.ended && can_idle(&self.turn, self.goal.as_ref(), self.stream_error.as_ref()) {
            if self.completed < self.segments.len() {
                self.idle(false, None);
            }
        } else if self.ended && !self.background.keeps_stdin_open() {
            self.turn.close_unless_steered();
        }
    }

    /// The model a request ran on when the agent did not name it.
    fn fallback_model(&mut self) -> Option<String> {
        let turn = &self.turn;
        self.fallback_model
            .get_or_insert_with(|| {
                crate::costs::turn_model(turn.agent, turn.settings.locked().model.as_deref())
            })
            .clone()
    }

    /// One model request's usage, priced and added to its reply's.
    fn request_usage(
        &mut self,
        request_id: String,
        mut usage: TokenUsage,
        top_level: bool,
        model: Option<String>,
    ) {
        if self.reader.reports_context(&request_id) {
            // The agent emits the pre-compaction context size as `/compact` runs; ignore
            // it so the UI does not show stale over-window context after compaction.
            if self.compaction.is_compacting() {
                return;
            }
            self.compaction.usage(
                usage.context,
                self.turn.compact_window.load(Ordering::SeqCst),
            );
        }
        if usage.cost_micros.is_none() {
            let model = match model {
                Some(model) => Some(model),
                None => self.fallback_model(),
            };
            usage.cost_micros = crate::costs::priced(model.as_deref(), &usage);
        }
        let message_id = record_request(
            &mut self.requests,
            request_id,
            usage,
            top_level,
            &self.turn.message_id,
        );
        self.turn.save_text(&mut self.unsaved);
        self.turn.publish(ChatEvent::Usage {
            usage: message_usage(&self.requests, &message_id),
            message_id,
        });
    }

    /// The turn's total output, of which what its requests did not count is added.
    fn turn_output(&mut self, total: u64) {
        let counted: u64 = self.requests[self.turn_requests..]
            .iter()
            .filter(|r| r.top_level)
            .map(|r| r.usage.output)
            .sum();
        if total > counted {
            let mut usage = TokenUsage {
                output: total - counted,
                ..TokenUsage::default()
            };
            let model = self.fallback_model();
            usage.cost_micros = crate::costs::priced(model.as_deref(), &usage);
            let id = format!("{TURN_OUTPUT}{}", self.requests.len());
            let message_id =
                record_request(&mut self.requests, id, usage, false, &self.turn.message_id);
            self.turn.publish(ChatEvent::Usage {
                usage: message_usage(&self.requests, &message_id),
                message_id,
            });
        }
        self.turn_requests = self.requests.len();
    }
}
