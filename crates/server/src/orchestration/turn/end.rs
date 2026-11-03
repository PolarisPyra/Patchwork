//! How a turn ends: its final status saved and published, the plugins' turn hooks, and the next queued message started.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use serde_json::Value;
use crate::plugins::HookEvent;
use crate::provider;
use crate::Server;
use crate::orchestration::AgentEvent;
use crate::provider::AgentKind;
use crate::threads::MessageStatus;
use crate::threads::Role;
use crate::logging::{AGENT_LOG, Data, Source};
use crate::settings::LogLevel;
use crate::orchestration::FollowUpInput;
use crate::orchestration::changes::checkpoint;
use crate::orchestration::goal::clear_command;
use crate::orchestration::output::stream::publish;
use crate::orchestration::turn::start::{Turn, TurnRequest};

/// Ends a turn whose agent has exited: forgets it, completes each of its messages, then starts
/// the turn that drops its goal or answers the steering messages it never took. When the agent lost
/// the session the turn resumed (`session_lost`), the turn runs again in a new session instead,
/// given the conversation so far as a transcript.
/// How an agent's turn ended, as `finish` needs to record it.
pub(in crate::orchestration) struct TurnEnd {
    pub(in crate::orchestration) status: MessageStatus,
    pub(in crate::orchestration) error: Option<String>,
    pub(in crate::orchestration) cancelled: bool,
    pub(in crate::orchestration) clear_in_agent: Option<&'static str>,
    pub(in crate::orchestration) session_lost: bool,
}

/// Pairs the turn's replies with the project's snapshots from before and after the turn, so their
/// changed files keep showing what the turn did once the changes are committed or edited again.
pub(in crate::orchestration) fn record_turn(turn: &Turn, replies: &[String]) {
    let Some(first) = replies.first() else { return };
    let Some(thread) = turn.server.store.thread(&turn.thread_id) else {
        return;
    };
    if thread.project_id.is_none() {
        return;
    }
    // The snapshot before the turn belongs to the user message the first reply answers.
    let Some(at) = thread.messages.iter().position(|m| &m.id == first) else {
        return;
    };
    let Some(asked) = thread.messages[..at]
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
    else {
        return;
    };
    if let Some(trees) = checkpoint::record_end(&turn.root, &turn.thread_id, &asked.id, replies) {
        for id in replies {
            turn.publish(AgentEvent::TurnRecorded {
                message_id: id.clone(),
                turn: trees.clone(),
            });
        }
    }
}

/// Holds the steers a stopped turn never answered as queued messages, so they wait to be resumed
/// instead of staying in the thread as sent. They were handed to the agent, but its turn is gone,
/// so they can be taken back again.
pub(in crate::orchestration) fn hold_unanswered(turn: &Turn, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let saved = turn.server.store.update_thread(&turn.thread_id, |t| {
        t.messages
            .iter_mut()
            .filter(|m| ids.contains(&m.id))
            .for_each(|m| {
                m.steer = false;
                m.queued = true;
            });
    });
    if let Err(e) = saved {
        log::warn!("failed to hold the unanswered steering messages: {e}");
        return;
    }
    turn.server
        .runner
        .handed_over
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|id| !ids.contains(id));
    ids.iter().for_each(|id| {
        publish(
            &turn.server,
            &turn.thread_id,
            AgentEvent::MessageQueued {
                message_id: id.clone(),
                queued: true,
            },
        );
    });
}

pub(in crate::orchestration) fn finish(turn: Turn, segments: &[String], end: TurnEnd) {
    let TurnEnd {
        status,
        error,
        cancelled,
        clear_in_agent,
        session_lost,
    } = end;
    // Forget the turn before announcing it. Queued messages remain persisted in the
    // thread until the user explicitly resumes them; unanswered steers were already
    // delivered to the agent and continue automatically if needed.
    let mut unanswered = {
        let mut turns = turn
            .server
            .runner
            .turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // A retired idle turn may already have been replaced by a new one.
        if turns
            .get(&turn.thread_id)
            .is_some_and(|t| Arc::ptr_eq(&t.exited, &turn.exited))
        {
            turns.remove(&turn.thread_id);
        }
        // Queued messages stay in the thread as queued until the turn has finished.
        // Steering messages the agent never answered continue automatically unless
        // the turn was stopped; a stopped turn holds them as queued messages instead.
        let unanswered = turn
            .steers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect::<Vec<_>>();
        let queued =
            std::mem::take(&mut *turn.queue.lock().unwrap_or_else(PoisonError::into_inner));
        let mut handed_over = turn
            .server
            .runner
            .handed_over
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        queued.iter().for_each(|message| {
            handed_over.remove(&message.message_id);
        });
        unanswered
    };
    if cancelled {
        hold_unanswered(
            &turn,
            &unanswered
                .iter()
                .map(|f| f.message_id.clone())
                .collect::<Vec<_>>(),
        );
        unanswered.clear();
    }
    if session_lost && unanswered.is_empty() {
        let server = &turn.server;
        let reset = server.store.update_thread(&turn.thread_id, |t| {
            t.session_id = None;
            t.context_pending = true;
        });
        if let Err(e) = reset {
            log::warn!("failed to drop a lost session: {e}");
        } else {
            turn.publish(AgentEvent::MessageRemoved {
                message_id: turn.message_id.clone(),
            });
            let (thread_id, text) = (turn.thread_id.clone(), turn.text.clone());
            let (attachments, images) = (turn.attachments.clone(), turn.images.clone());
            let started = server.runner.start_turn(
                server,
                TurnRequest {
                    thread_id,
                    text,
                    attachments,
                    images,
                },
            );
            match started {
                Ok(()) => return,
                Err(e) => log::warn!("failed to rerun the turn in a new session: {e}"),
            }
        }
    }
    record_turn(&turn, segments);
    // Earlier messages of the turn were answered in full before a steer moved it on. None are
    // left when the agent went idle after its last reply, which was completed then.
    if let Some((last, earlier)) = segments.split_last() {
        earlier.iter().for_each(|id| {
            turn.publish(AgentEvent::TurnCompleted {
                message_id: id.clone(),
                status: MessageStatus::Complete,
                error: None,
            });
        });
        turn.publish(AgentEvent::TurnCompleted {
            message_id: last.clone(),
            status,
            error,
        });
        // The project's own checks, once the reply reads as finished. Not every reply deserves
        // them: a stopped turn, or one that changed nothing, leaves the project as it was.
        if crate::checks::worth_checking(&turn.server.store, &turn.thread_id, last, status) {
            turn.server.check_after_turn(&turn.thread_id, last);
        }
    }
    // On a thread of its own, so a slow hook does not hold up the next turn.
    {
        let (server, thread_id, root) = (
            turn.server.clone(),
            turn.thread_id.clone(),
            turn.root.clone(),
        );
        let (agent, ended) = (
            turn.agent,
            if cancelled {
                HookEvent::Interrupt
            } else {
                HookEvent::Stop
            },
        );
        std::thread::spawn(move || {
            for event in [ended, HookEvent::SessionEnd] {
                plugin_hooks(&server, event, &thread_id, &root, agent, Some(status));
            }
        });
    }

    // The agent drops the goal in a turn of its own; unanswered steers wait for that
    // system turn instead, and queued messages for the end of it.
    if let Some(command) = clear_in_agent {
        let server = &turn.server;
        match server.send_message(turn.thread_id.clone(), command.into(), vec![], false) {
            Ok(_) => {
                let runner = &server.runner;
                unanswered.iter().for_each(|f| {
                    let _ = runner.enqueue(
                        &turn.thread_id,
                        FollowUpInput {
                            message_id: &f.message_id,
                            text: &f.text,
                            attachments: &f.attachments,
                            images: &f.images,
                        },
                    );
                });
                return;
            }
            Err(e) => log::warn!("failed to clear the goal in the agent's session: {e}"),
        }
    }
    // Unanswered steers have already been delivered, so clear their pending UI state
    // before continuing them in a new turn.
    unanswered.iter().for_each(|f| {
        publish(
            &turn.server,
            &turn.thread_id,
            AgentEvent::MessageQueued {
                message_id: f.message_id.clone(),
                queued: false,
            },
        );
    });
    if !unanswered.is_empty() {
        let text = unanswered
            .iter()
            .map(|f| f.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let (attachments, images) = unanswered
            .into_iter()
            .map(|f| (f.attachments, f.images))
            .fold((vec![], vec![]), |(mut a, mut i), (fa, fi)| {
                a.extend(fa);
                i.extend(fi);
                (a, i)
            });
        let server = &turn.server;
        if let Err(e) = server.runner.start_turn(
            server,
            TurnRequest {
                thread_id: turn.thread_id.clone(),
                text,
                attachments,
                images,
            },
        ) {
            log::warn!("failed to start the turn for queued messages: {e}");
        }
    } else if !cancelled
        && status == MessageStatus::Complete
        && turn.text != clear_command(turn.agent)
    {
        // Queued messages go once the turn has finished; after Stop or an error they wait for
        // the user to send them. The turn that only drops the goal after a Stop is not a turn
        // the user ended, so the messages it held keep waiting too.
        turn.server.send_queued_later(&turn.thread_id);
    }
}

/// Runs the enabled plugins' hooks for `event` and logs the ones that failed.
pub(in crate::orchestration) fn plugin_hooks(
    server: &Server,
    event: HookEvent,
    thread_id: &str,
    root: &str,
    agent: AgentKind,
    status: Option<MessageStatus>,
) {
    let name = |value: serde_json::Result<Value>| {
        value
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    };
    let agent = name(serde_json::to_value(agent));
    let status = status.map(|status| name(serde_json::to_value(status)));
    let context = crate::plugins::HookContext {
        event,
        thread_id,
        root,
        agent: &agent,
        status: status.as_deref(),
    };
    for failure in crate::plugins::run_hooks(&server.store.settings(), &context) {
        log::warn!("plugin hook {} {failure}", event.name());
    }
}

/// A background command's window, apart from its call's, which has the same id.
pub(in crate::orchestration) fn background_id(tool_id: &str) -> String {
    format!("background:{tool_id}")
}

/// Logs the agent's CLI version once per run of the app. A thread asks, since running the CLI
/// takes a moment the turn should not wait for.
pub(in crate::orchestration) fn log_cli_version(agent: AgentKind) {
    static LOGGED: Mutex<Vec<AgentKind>> = Mutex::new(Vec::new());
    if agent == AgentKind::Api {
        return;
    }
    {
        let mut logged = LOGGED.lock().unwrap_or_else(PoisonError::into_inner);
        if logged.contains(&agent) {
            return;
        }
        logged.push(agent);
    }
    std::thread::spawn(move || {
        let version = provider::cli_version(agent).unwrap_or_else(|| "unknown".into());
        let source = Source {
            agent: Some(agent),
            ..Source::default()
        };
        AGENT_LOG.write(
            LogLevel::Info,
            "cli",
            &source,
            Data::Text(format!("{agent:?} {version}")),
        );
    });
}
