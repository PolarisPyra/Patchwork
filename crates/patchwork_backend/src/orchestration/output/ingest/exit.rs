//! The end of the agent's output: its exit, what it left running, and the turn's final status.

// Rust guideline compliant 2026-10-09

use std::process::Child;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

use super::read::Reading;
use crate::chats::MessageStatus;
use crate::events::ChatEvent;
use crate::logging::{AGENT_LOG, Data};
use crate::orchestration::goal;
use crate::orchestration::output::stream::publish;
use crate::orchestration::turn::end::{TurnEnd, finish};
use crate::orchestration::turn::process::{exit_error, last_lines, with_remedy};
use crate::provider::Parsed;
use crate::settings::LogLevel;
use crate::source_control;
use crate::system::Locked;
use crate::system::processes::{running, scope};

impl Reading {
    /// Records the turn's end once the agent's output has closed.
    pub(super) fn end(mut self, mut child: Child, stderr_tail: JoinHandle<String>) {
        // The stream has closed, whether the turn finished, failed, or was cancelled.
        self.turn.save_text(&mut self.unsaved);
        // An idle agent changes files only through shell commands it left running; other changes
        // since its last reply are the user's own.
        let idle_untouched =
            self.turn.idle.load(Ordering::SeqCst) && !self.background.any_listed_live();
        self.exit_error();
        // The agent process is gone, and the tasks it started go with it.
        for event in self.background.stop_all() {
            self.publish_event(event);
        }
        // So does the work it kept out of its stream: a card still reading running stopped with
        // it. A stop by the person is not a failure, so their cards end stopped rather than failed.
        let failed = !self.turn.cancelled.load(Ordering::SeqCst);
        for parsed in self.reader.finish_open(&self.turn.message_id, failed) {
            if let Parsed::Event(event) = parsed {
                self.publish_event(event);
            }
        }
        let Self {
            turn,
            pid,
            mut children,
            segments,
            completed,
            stream_error,
            session_lost,
            ..
        } = self;
        let exit = child.wait().ok();
        let status = exit.map_or_else(|| "unknown".into(), |s| s.to_string());
        // A turn the user stopped, or the app quit, ends by signal: that is not a failure.
        let stopped = turn.cancelled.load(Ordering::SeqCst);
        let level = if stopped || exit.is_some_and(|s| s.success()) {
            LogLevel::Info
        } else {
            LogLevel::Error
        };
        let source =
            crate::logging::Source::agent(turn.agent, pid, &turn.chat_id, Some(&turn.message_id));
        AGENT_LOG.write(level, "exit", &source, Data::Text(status));
        // Whatever the agent left running in its group or scope (MCP servers, background
        // commands) would otherwise outlive the turn and keep its memory in the agents' slice.
        running::signal_process_group(pid, running::SIGKILL);
        if let Some(unit) = &turn.scope {
            scope::stop(unit);
        }
        turn.exited.store(true, Ordering::SeqCst);
        crate::orchestration::turn::recovery::forget_agent(&turn.core.store, &turn.chat_id, pid);
        let tail = stderr_tail.join().unwrap_or_default();
        let cancelled = turn.cancelled.load(Ordering::SeqCst);
        let (status, error) = if cancelled {
            (turn.core.runner.stopped_status(), None)
        } else if stream_error.is_some() || !exit.is_some_and(|s| s.success()) {
            let error = stream_error
                .or_else(|| last_lines(&tail))
                .unwrap_or_else(|| exit_error(exit));
            (MessageStatus::Error, Some(with_remedy(turn.agent, error)))
        } else {
            (MessageStatus::Complete, None)
        };
        // Subagents end with the agent that ran them.
        for (_, child) in children.drain() {
            publish(
                &turn.core,
                &child.chat_id,
                ChatEvent::TurnCompleted {
                    message_id: child.message_id,
                    status,
                    error: None,
                },
            );
        }

        if !idle_untouched {
            let after = source_control::git_changes(&turn.root).unwrap_or_default();
            let changed = turn.attribution.locked().settle(&turn.root, after);
            for path in changed {
                turn.publish(ChatEvent::FileTouched {
                    message_id: turn.message_id.clone(),
                    path,
                });
            }
        }

        // An agent that went idle had no active goal, and a turn that replaced it may have set one
        // since.
        let goal_continues =
            !turn.idle.load(Ordering::SeqCst) && goal::settle(&turn, status, cancelled, &segments);

        let session_lost = session_lost && !cancelled;
        finish(
            turn,
            &segments[completed..],
            TurnEnd {
                status,
                error,
                cancelled,
                goal_continues,
                session_lost,
            },
        );
    }

    /// Sets the error an agent that exited without finishing its work ends the turn with.
    fn exit_error(&mut self) {
        let turn = &self.turn;
        let name = turn.agent.identity().name;
        let cancelled = turn.cancelled.load(Ordering::SeqCst);
        if turn.stalled.load(Ordering::SeqCst) && self.stream_error.is_none() {
            self.stream_error = Some(format!(
                "{name} said nothing for 15 minutes, so its reply was stopped. Send a message to go on."
            ));
        }
        // An agent that exited while its swarm workers were still running did not finish them.
        if !cancelled && self.stream_error.is_none() && self.reader.is_holding() {
            self.stream_error = Some(format!(
                "{name} exited while swarm workers were still running."
            ));
        }
        // An ACP agent exits only when its stdin closes, so an exit with a prompt still
        // unanswered is a failure even with a clean exit code. Work it started on its own may
        // end with it.
        if self.caps.acp
            && !cancelled
            && self.stream_error.is_none()
            && ((!self.ended && !turn.idle.load(Ordering::SeqCst))
                || !turn.steers.locked().is_empty())
        {
            self.stream_error = Some(format!("{name} exited before it finished replying."));
        }
    }
}
