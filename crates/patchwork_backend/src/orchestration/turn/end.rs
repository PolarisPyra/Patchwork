//! How a turn ends: its final status saved and published, the turn hooks, and the next queued message started.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::sync::Arc;
use std::sync::Mutex;
use serde_json::Value;
use crate::extensions::hooks::HookEvent;
use crate::provider;
use crate::Core;
use crate::events::ChatEvent;
use crate::provider::AgentKind;
use crate::chats::MessageStatus;
use crate::chats::Role;
use crate::logging::{AGENT_LOG, Data, Source};
use crate::settings::LogLevel;
use crate::orchestration::messages::queue::FollowUp;
use crate::orchestration::changes::checkpoint;
use crate::orchestration::goal;
use crate::orchestration::output::stream::publish;
use crate::orchestration::turn::start::{Turn, TurnRequest};

/// How an agent's turn ended, as `finish` needs to record it.
pub(in crate::orchestration) struct TurnEnd {
    pub(in crate::orchestration) status: MessageStatus,
    pub(in crate::orchestration) error: Option<String>,
    pub(in crate::orchestration) cancelled: bool,
    /// The goal goes on in another turn once this one ends.
    pub(in crate::orchestration) goal_continues: bool,
    pub(in crate::orchestration) session_lost: bool,
}

/// Pairs the turn's replies with the project's snapshots from before and after the turn, so their
/// changed files keep showing what the turn did once the changes are committed or edited again.
pub(in crate::orchestration) fn record_turn(turn: &Turn, replies: &[String]) {
    let Some(first) = replies.first() else { return };
    let Some(chat) = turn.core.store.chat(&turn.chat_id) else {
        return;
    };
    if chat.project_id.is_none() {
        return;
    }
    // The snapshot before the turn belongs to the user message the first reply answers.
    let Some(at) = chat.messages.iter().position(|m| &m.id == first) else {
        return;
    };
    let Some(asked) = chat.messages[..at]
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
    else {
        return;
    };
    if let Some(trees) = checkpoint::record_end(&turn.root, &turn.chat_id, &asked.id, replies) {
        for id in replies {
            turn.publish(ChatEvent::TurnRecorded {
                message_id: id.clone(),
                turn: trees.clone(),
            });
        }
    }
}

/// Holds messages a turn took but never answered as queued messages: the steers a stopped turn
/// never answered, so they wait to be resumed instead of staying in the chat as sent, and the
/// messages taken from its queue. They were handed to the agent, but its turn is gone, so they
/// can be taken back again.
pub(in crate::orchestration) fn hold_unanswered(turn: &Turn, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let saved = turn.core.store.update_chat(&turn.chat_id, |t| {
        t.messages
            .iter_mut()
            .filter(|m| ids.contains(&m.id))
            .for_each(|m| {
                let m = Arc::make_mut(m);
                m.steer = false;
                m.queued = true;
            });
    });
    if let Err(e) = saved {
        log::warn!(error:% = e; "failed to hold the unanswered steering messages: {{error}}");
        return;
    }
    turn.core.runner.with_run(&turn.chat_id, |run| {
        run.handed_over.retain(|id| !ids.contains(id));
    });
    for id in ids {
        publish(
            &turn.core,
            &turn.chat_id,
            ChatEvent::MessageQueued {
                message_id: id.clone(),
                queued: true,
            },
        );
    }
}

/// Ends a turn whose agent has exited.
///
/// Forgets the turn, completes each of its messages, then starts the turn that answers the
/// steering messages it never took, sends the queued ones, or carries the goal on. When the agent lost the session the turn resumed
/// (`session_lost`), the turn runs again in a new session instead, given the conversation so far as
/// a transcript.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the turn ends here, so its handles are dropped as it finishes"
)]
pub(in crate::orchestration) fn finish(turn: Turn, segments: &[String], end: TurnEnd) {
    let TurnEnd {
        status,
        error,
        cancelled,
        goal_continues,
        session_lost,
    } = end;
    // Forget the turn before announcing it. Queued messages remain persisted in the
    // chat until the user explicitly resumes them; unanswered steers were already
    // delivered to the agent and continue automatically if needed.
    let (mut unanswered, queued) = turn.core.runner.with_run(&turn.chat_id, |run| {
        // A retired idle turn may already have been replaced by a new one.
        if run
            .turn
            .as_ref()
            .is_some_and(|t| Arc::ptr_eq(&t.exited, &turn.exited))
        {
            run.turn = None;
            // A call the user allowed through a hook was allowed for this turn only.
            run.grants.clear();
        }
        // Queued messages stay in the chat as queued until the turn has finished.
        // Steering messages the agent never answered continue automatically unless
        // the turn was stopped; a stopped turn holds them as queued messages instead.
        let unanswered = turn.steers.locked().drain(..).collect::<Vec<_>>();
        let queued = std::mem::take(&mut *turn.queue.locked());
        for message in &queued {
            run.handed_over.remove(&message.message_id);
        }
        // With no turn left in the chat, nothing it was handed or allowed is kept.
        run.forget_ended_turn();
        (unanswered, queued)
    });
    // Taken from the queue, so marked queued here: the sender may not have saved the flag yet.
    hold_unanswered(
        &turn,
        &queued.iter().flat_map(FollowUp::ids).collect::<Vec<_>>(),
    );
    if cancelled {
        hold_unanswered(
            &turn,
            &unanswered
                .iter()
                .flat_map(FollowUp::ids)
                .collect::<Vec<_>>(),
        );
        unanswered.clear();
    }
    if session_lost && unanswered.is_empty() {
        let core = &turn.core;
        let reset = core.store.update_chat(&turn.chat_id, |t| {
            t.session_id = None;
            t.context_pending = true;
        });
        if let Err(e) = reset {
            log::warn!(error:% = e; "failed to drop a lost session: {{error}}");
        } else {
            // Kept so the reply can go back in the chat, with its error, if the rerun never starts.
            let reply = core.store.chat(&turn.chat_id).and_then(|t| {
                t.messages
                    .iter()
                    .find(|m| m.id == turn.message_id)
                    .map(|m| (**m).clone())
            });
            turn.publish(ChatEvent::MessageRemoved {
                message_id: turn.message_id.clone(),
            });
            let (chat_id, text) = (turn.chat_id.clone(), turn.text.clone());
            let (attachments, images) = (turn.attachments.clone(), turn.images.clone());
            let started = core.runner.start_turn(
                core,
                TurnRequest {
                    chat_id,
                    text,
                    attachments,
                    images,
                    resumes: turn.resumes,
                },
            );
            match started {
                Ok(()) => return,
                Err(e) => {
                    log::warn!(error:% = e; "failed to rerun the turn in a new session: {{error}}");
                    if let Some(message) = reply {
                        turn.publish(ChatEvent::MessageStarted { message });
                    }
                }
            }
        }
    }
    record_turn(&turn, segments);
    // Earlier messages of the turn were answered in full before a steer moved it on. None are
    // left when the agent went idle after its last reply, which was completed then.
    if let Some((last, earlier)) = segments.split_last() {
        for id in earlier {
            turn.publish(ChatEvent::TurnCompleted {
                message_id: id.clone(),
                status: MessageStatus::Complete,
                error: None,
            });
        }
        turn.publish(ChatEvent::TurnCompleted {
            message_id: last.clone(),
            status,
            error,
        });
        // A reply that failed on its agent's usage limit warns of it until the agent replies in
        // the chat again.
        if !cancelled && status == MessageStatus::Error {
            turn.core
                .note_limit_failure(&turn.chat_id, last, turn.agent);
        } else if !cancelled && status == MessageStatus::Complete {
            turn.core.clear_limit_failures(&turn.chat_id, turn.agent);
        }
        // The project's own checks, once the reply reads as finished. Not every reply deserves
        // them: a stopped turn, or one that changed nothing, leaves the project as it was.
        if crate::checks::worth_checking(&turn.core.store, &turn.chat_id, last, status) {
            turn.core.check_after_turn(&turn.chat_id, last);
        }
    }
    // On a thread of its own, so a slow hook does not hold up the next turn.
    {
        let (core, chat_id, root) = (
            Arc::clone(&turn.core),
            turn.chat_id.clone(),
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
                turn_hooks(&core, event, &chat_id, &root, agent, Some(status));
            }
        })
    };

    // An agent that was sent the conversation and gave nothing back before it failed never took
    // it, and has no session that holds it: the next turn sends it again.
    if status == MessageStatus::Error && turn.sent_context {
        let saved = turn.core.store.update_chat(&turn.chat_id, |t| {
            let silent = t
                .messages
                .iter()
                .find(|m| m.id == turn.message_id)
                .is_none_or(|m| m.text.is_empty() && m.tools.is_empty());
            if t.session_id.is_none() && silent {
                t.context_pending = true;
            }
        });
        if let Err(e) = saved {
            log::warn!(error:% = e; "could not keep the conversation for the next turn: {{error}}");
        }
    }

    // Unanswered steers have already been delivered, so clear their pending UI state
    // before continuing them in a new turn.
    for f in &unanswered {
        publish(
            &turn.core,
            &turn.chat_id,
            ChatEvent::MessageQueued {
                message_id: f.message_id.clone(),
                queued: false,
            },
        );
    }
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
        let core = &turn.core;
        if let Err(e) = core.runner.start_turn(
            core,
            TurnRequest {
                chat_id: turn.chat_id.clone(),
                text,
                attachments,
                images,
                resumes: false,
            },
        ) {
            log::warn!(error:% = e; "failed to start the turn for queued messages: {{error}}");
        }
    } else if goal_continues {
        // Messages queued meanwhile go first; the goal carries on after them.
        goal::carry_on_later(&turn.core, &turn.chat_id);
    } else if !cancelled && status == MessageStatus::Complete {
        // Queued messages go once the turn has finished; after Stop or an error they wait for
        // the user to send them.
        turn.core.send_queued_later(&turn.chat_id);
    }
}

/// Runs the enabled hooks for `event` and logs the ones that failed. A chat app's dispatcher
/// runs none.
pub(in crate::orchestration) fn turn_hooks(
    core: &Core,
    event: HookEvent,
    chat_id: &str,
    root: &str,
    agent: AgentKind,
    status: Option<MessageStatus>,
) {
    // A dispatcher runs nothing on the computer, the user's hooks included.
    if core.is_dispatcher(chat_id) {
        return;
    }
    let name = |value: serde_json::Result<Value>| {
        value
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    };
    let agent = name(serde_json::to_value(agent));
    let status = status.map(|status| name(serde_json::to_value(status)));
    let context = crate::extensions::hooks::HookContext {
        event,
        chat_id,
        root,
        agent: &agent,
        status: status.as_deref(),
    };
    for failure in crate::extensions::hooks::run_hooks(&core.store.settings(), &context) {
        log::warn!(event = event.name(), failure:%; "hook {{event}} {{failure}}");
    }
}

/// A background command's window, apart from its call's, which has the same id.
pub(in crate::orchestration) fn background_id(tool_id: &str) -> String {
    format!("background:{tool_id}")
}

/// Logs the agent's CLI version once per run of the app. A chat asks, since running the CLI
/// takes a moment the turn should not wait for.
pub(in crate::orchestration) fn log_cli_version(agent: AgentKind) {
    static LOGGED: Mutex<Vec<AgentKind>> = Mutex::new(Vec::new());
    if agent == AgentKind::Api {
        return;
    }
    {
        let mut logged = LOGGED.locked();
        if logged.contains(&agent) {
            return;
        }
        logged.push(agent);
    };
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
