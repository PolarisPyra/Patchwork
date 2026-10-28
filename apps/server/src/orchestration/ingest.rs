//! Reads an agent's output for one turn and hands each event to the modules that handle it.
use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::Path;
use std::process::Child;
use std::process::ChildStdout;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::PoisonError;
use std::thread::JoinHandle;

use serde_json::Value;

use crate::provider::GoalUpdate;
use crate::permissions;
use crate::provider::Parsed;
use crate::provider;
use crate::scope;
use crate::types::AgentEvent;
use crate::types::AgentKind;
use crate::types::Approval;
use crate::types::ApprovalStatus;
use crate::types::BackgroundTask;

/// What kind of output a line is, as its `type` (or JSON-RPC `method`) and any `subtype`.
fn line_kind(value: &Value) -> String {
    let kind = value["type"].as_str().or_else(|| value["method"].as_str()).unwrap_or("untyped");
    match value["subtype"].as_str() {
        Some(subtype) => format!("{kind}/{subtype}"),
        None => kind.into(),
    }
}

/// Publishes a subagent's events to its own thread, with its replies a paragraph apart.
fn publish_child(turn: &Turn, child: &mut SubagentThread, events: Vec<AgentEvent>) {
    for mut event in events {
        if let AgentEvent::TextDelta { text, .. } = &mut event {
            if child.has_text {
                text.insert_str(0, "\n\n");
            }
            child.has_text = true;
        }
        publish(&turn.server, &child.thread_id, event);
    }
}

#[cfg(test)]
use crate::types::FileChange;
use crate::types::Goal;
use crate::types::GoalStatus;
use crate::types::MessageStatus;
use crate::types::new_id;
use crate::debug_log::{AGENTS, Data, Source};
use crate::types::LogLevel;
use crate::workspace;

use super::attribution;
#[cfg(test)]
use super::attribution::changed_paths;
use super::goal::end_input;
use super::goal;
use super::process::exit_error;
use super::process::last_lines;
use super::process::next_line;
use super::process::TURN_TIMEOUT;
use super::process::terminate;
use super::stuck::{LOOP_LIMIT, Repeats};
use super::process::write_line;
use super::stream::publish;
use super::stream::Unsaved;
use super::subagents::finish_child;
use super::subagents::open_child;
use super::subagents::SubagentThread;
use super::turn::{TurnEnd, finish, record_turn};
use super::turn::{PendingApproval, Turn};
use super::decide;
use super::unasked;
use super::usage::message_usage;
use super::usage::record_request;
use super::usage::Request;


/// Reads the agent's stdout until it closes, then records the final status.
pub(super) fn ingest(mut turn: Turn, mut child: Child, stdout: ChildStdout, stderr_tail: JoinHandle<String>) {
    let pid = child.id();
    let mut has_text = false;
    // Antigravity streams one text block as many deltas; other agents send whole blocks.
    let mut last_was_text = false;
    // The reply opened for a steer has nothing in it yet, so a steer right after it shares it.
    let mut steer_reply_empty = false;
    // Tool id → the message it was started in, which a steer may since have moved on from.
    let mut tool_owner: HashMap<String, String> = HashMap::new();
    // The threads of the subagents still running, by the call that started each.
    let mut children: HashMap<String, SubagentThread> = HashMap::new();
    // Subagents still running, and whether the call of one sent to the background, which
    // returns at once, failed; it is reported finished when the subagent ends.
    let mut running_tasks: HashMap<String, Option<bool>> = HashMap::new();
    // Background shell commands the agent started itself: tool call id → the message they are shown in.
    let mut shell_tasks: HashMap<String, String> = HashMap::new();
    // Every message of the turn: the first reply, then one per steering message taken.
    let mut segments = vec![turn.message_id.clone()];
    // How many of `segments` were completed when the agent went idle.
    let mut completed = 0;
    let mut stream_error: Option<String> = None;
    let mut repeats = Repeats::default();
    // The agent could not find the session the turn resumed, so the turn runs again without it.
    let mut session_lost = false;
    let mut goal = turn
        .server
        .store
        .thread(&turn.thread_id)
        .and_then(|t| t.goal);
    // The agent reported its turn finished; Codex may start another for an active goal.
    let mut ended = false;
    // Codex reported this run's goal active, so it starts further turns toward it.
    let mut goal_run = false;
    // Claude ended a reply while pursuing the goal, so its evaluator checked that reply.
    let mut goal_checked = false;
    // Bumped whenever a turn starts, so a grace window that is still waiting does not close stdin.
    let goal_round = Arc::new(AtomicU64::new(0));
    // Text streamed to the frontend but not yet stored; saving every delta
    // stalls the reader, so it is stored before the next event and at the end.
    let mut unsaved = Unsaved::default();
    // Every model request the turn has made; each reply's usage is summed from these.
    let mut requests: Vec<Request> = Vec::new();

    // Kinds of output the parser had no use for, logged once each per turn, so a CLI that
    // changes how it reports something shows up in the agent log.
    let mut logged_kinds: HashSet<String> = HashSet::new();

    let mut stdout = BufReader::new(stdout);
    while let Some(line) = next_line(&mut stdout) {
        let source =
            |turn: &Turn| Source::agent(turn.agent, pid, &turn.thread_id, Some(&turn.message_id));
        if AGENTS.logs(LogLevel::Error) {
            AGENTS.line("out", &source(&turn), &line);
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            if logged_kinds.insert("unparsed".into()) {
                AGENTS.write(LogLevel::Warning, "unparsed", &source(&turn), Data::Outline(line));
            }
            continue;
        };
        // A message Claude took after it had already ended its reply (while a background
        // subagent kept it running) is answered from here on, below that message. So is
        // a reply an idle Claude starts on its own, such as once a background command ends.
        let starts_reply = value["type"] == "system" && value["subtype"] == "init";
        if ended
            && turn.agent == AgentKind::Claude
            && ((starts_reply && turn.idle.swap(false, Ordering::SeqCst))
                || !turn
                    .steers
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_empty())
        {
            turn.steers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clear();
            turn.next_segment(&mut unsaved);
            segments.push(turn.message_id.clone());
            (has_text, last_was_text) = (false, false);
            ended = false;
        }
        // A subagent's frames name the call that started it; they also go to its own thread.
        if turn.agent == AgentKind::Claude
            && let Some(child) = value["parent_tool_use_id"]
                .as_str()
                .and_then(|id| children.get_mut(id))
        {
            let events = provider::claude::subagent_events(&value, &turn.root, &child.message_id);
            publish_child(&turn, child, events);
        }
        // Codex sends a subagent's notifications on the same stream, under the subagent's own
        // thread. They go to its thread alone: its finished turn is not this one's. Its requests,
        // such as approvals, are answered here like any other.
        if turn.agent == AgentKind::Codex
            && value.get("id").is_none()
            && let Some(thread) = value["params"]["threadId"].as_str()
            && turn.session_id.as_deref().is_some_and(|own| own != thread)
        {
            if let Some(child) = children.get_mut(thread) {
                let events =
                    provider::codex::subagent_events(&value, &turn.spec(), &child.message_id);
                publish_child(&turn, child, events);
            }
            continue;
        }
        let parsed = provider::parse_line(turn.agent, &value, &turn.spec(), &turn.message_id);
        // A subagent's frames went to its own thread above.
        let routed = turn.agent == AgentKind::Claude && value["parent_tool_use_id"].is_string();
        if parsed.is_empty() && !routed && AGENTS.logs(LogLevel::Warning) {
            let kind = line_kind(&value);
            if logged_kinds.insert(kind.clone()) {
                AGENTS.write(LogLevel::Warning, "ignored", &source(&turn), Data::Text(kind));
            }
        }
        for item in parsed {
            // A call the access mode runs without asking runs unless a rule refuses it or asks.
            let item = match item {
                Parsed::Check { title, detail, reply, target } => {
                    let asks = turn.mode != crate::types::AccessMode::FullAccess;
                    if unasked(&turn.server, &turn.thread_id, &turn.root, &target, asks) == permissions::Unasked::Run {
                        write_line(&turn.stdin, &provider::approval_reply(turn.agent, &reply, true, &HashMap::new()));
                        continue;
                    }
                    Parsed::Approval { title, detail, reply, questions: Vec::new(), target: Some(target) }
                }
                item => item,
            };
            match item {
                Parsed::Send(line) => {
                    write_line(&turn.stdin, &line);
                }
                Parsed::McpCall { id, server, tool, arguments } => {
                    let stdin = turn.stdin.clone();
                    let (cancelled, exited) = (turn.cancelled.clone(), turn.exited.clone());
                    std::thread::spawn(move || {
                        let stopped = || cancelled.load(Ordering::SeqCst) || exited.load(Ordering::SeqCst);
                        let result = provider::mcp::call(&server, &tool, &arguments, &stopped);
                        write_line(&stdin, &provider::api::mcp_reply(&id, result));
                    });
                }
                Parsed::Approval {
                    title,
                    detail,
                    reply,
                    questions,
                    target,
                } => {
                    let mut approval = Approval {
                        id: new_id(),
                        title,
                        detail,
                        status: ApprovalStatus::Pending,
                        questions,
                        answers: HashMap::new(),
                        kind: target.as_ref().map(|t| t.kind),
                        suggestions: target
                            .as_ref()
                            .map(|t| permissions::suggest(t, Path::new(&turn.root)))
                            .unwrap_or_default(),
                        rule: None,
                        auto: false,
                    };
                    // The user already allowed it through a permission hook.
                    if target.as_ref().is_some_and(|t| turn.server.runner.take_grant(&turn.thread_id, t, Path::new(&turn.root))) {
                        write_line(&turn.stdin, &provider::approval_reply(turn.agent, &reply, true, &HashMap::new()));
                        continue;
                    }
                    // A permission rule answers it without asking.
                    if let Some(decision) = target.as_ref().and_then(|t| decide(&turn.server, &turn.thread_id, &turn.root, t)) {
                        write_line(
                            &turn.stdin,
                            &provider::approval_reply(turn.agent, &reply, decision.allow, &HashMap::new()),
                        );
                        approval.status = if decision.allow { ApprovalStatus::Allowed } else { ApprovalStatus::Denied };
                        approval.rule = Some(decision.rule);
                        approval.auto = true;
                        turn.save_text(&mut unsaved);
                        turn.publish(AgentEvent::ApprovalRequested {
                            message_id: turn.message_id.clone(),
                            approval,
                        });
                        continue;
                    }
                    turn.pending.lock().unwrap_or_else(PoisonError::into_inner).insert(
                        approval.id.clone(),
                        PendingApproval {
                            message_id: turn.message_id.clone(),
                            reply,
                            target,
                            root: turn.root.clone(),
                        },
                    );
                    turn.save_text(&mut unsaved);
                    turn.publish(AgentEvent::ApprovalRequested {
                        message_id: turn.message_id.clone(),
                        approval,
                    });
                }
                // Answered above.
                Parsed::Check { .. } => {}
                Parsed::Finished => {
                    ended = true;
                    let steered = !turn
                        .steers
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .is_empty();
                    // Claude ends its reply to take the steering messages, then answers them.
                    if steered && turn.agent == AgentKind::Claude {
                        turn.steers
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .clear();
                        turn.next_segment(&mut unsaved);
                        segments.push(turn.message_id.clone());
                        (has_text, last_was_text) = (false, false);
                        // Its answer ends with another result; stdin stays open for more.
                        ended = false;
                        continue;
                    }
                    // The finished turn can no longer be steered; until Codex starts the
                    // next one, messages wait for the run to end.
                    *turn
                        .native_turn
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = None;
                    if stream_error.is_some() {
                        goal_run = false;
                    }
                    // Claude stays up idle for the next message, so its session and connectors
                    // are not set up again for each one. It also stays up while a subagent it
                    // sent to the background works, and takes messages meanwhile.
                    let background = running_tasks.values().any(Option::is_some);
                    if can_idle(&turn, &running_tasks, &shell_tasks, &goal, &stream_error) {
                        go_idle(&mut turn, &segments, &mut completed, &mut unsaved);
                    } else if !background {
                        end_input(&turn, goal_run, &goal_round);
                    }
                }
                Parsed::ControlResponse(id) => {
                    let waiting = turn
                        .controls
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(&id);
                    if let Some(waiting) = waiting {
                        let _ = waiting.send(());
                    }
                }
                Parsed::Steered => {
                    turn.steers
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .pop_front();
                    if !steer_reply_empty {
                        turn.next_segment(&mut unsaved);
                        segments.push(turn.message_id.clone());
                        (has_text, last_was_text) = (false, false);
                        steer_reply_empty = true;
                    }
                    if ended && !goal_run {
                        turn.close_unless_steered();
                    }
                }
                Parsed::SteerRejected(message) => {
                    log::warn!("the agent did not take the steering message: {message}");
                    let refused = turn
                        .steers
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .pop_front();
                    turn.queue
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .extend(refused);
                    if ended && !goal_run {
                        turn.close_unless_steered();
                    }
                }
                Parsed::Goal(GoalUpdate::Checking) => {
                    if !turn.cancelled.load(Ordering::SeqCst)
                        && goal.as_ref().is_some_and(|g| goal::pursued(turn.agent, g))
                    {
                        goal_checked = true;
                        turn.save_text(&mut unsaved);
                        turn.publish(AgentEvent::GoalChecking {
                            message_id: turn.message_id.clone(),
                        });
                    }
                }
                Parsed::Goal(update) => {
                    // Once stopped, the agent's last word on the goal no longer applies.
                    if turn.cancelled.load(Ordering::SeqCst) {
                        continue;
                    }
                    if goal::update_is_dismissed(&turn.server.store, &turn.thread_id, &update)
                        || goal::update_releases_held(
                            &turn.server.store,
                            &turn.thread_id,
                            goal.as_ref(),
                            &update,
                        )
                    {
                        continue;
                    }
                    // Clearing from the banner drops the stored goal before the agent replies,
                    // so the clear is published even when this turn started without a goal.
                    let cleared = matches!(update, GoalUpdate::Cleared);
                    let next = goal::next_goal(goal.as_ref(), update);
                    if next != goal || cleared {
                        goal = next;
                        turn.save_text(&mut unsaved);
                        turn.publish(AgentEvent::GoalUpdated { goal: goal.clone() });
                    }
                    // Codex goes on toward an active goal in turns it starts itself.
                    goal_run = turn.agent == AgentKind::Codex
                        && goal
                            .as_ref()
                            .is_some_and(|g| g.status == GoalStatus::Active);
                    if ended && can_idle(&turn, &running_tasks, &shell_tasks, &goal, &stream_error) {
                        if completed < segments.len() {
                            go_idle(&mut turn, &segments, &mut completed, &mut unsaved);
                        }
                    } else if ended {
                        end_input(&turn, goal_run, &goal_round);
                    }
                }
                Parsed::SessionId(id) => {
                    let store = &turn.server.store;
                    if let Err(e) =
                        store.update_thread(&turn.thread_id, |t| t.session_id = Some(id.clone()))
                    {
                        log::warn!("failed to save session id: {e}");
                    }
                    turn.session_id = Some(id);
                }
                Parsed::SlashCommands(commands) => {
                    turn.publish(AgentEvent::SlashCommandsUpdated { commands });
                }
                Parsed::TaskStarted { tool_id, task_id } => {
                    running_tasks.insert(tool_id.clone(), None);
                    turn.tasks
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(tool_id, task_id);
                }
                Parsed::BackgroundTask {
                    tool_id,
                    description,
                } => {
                    shell_tasks.insert(tool_id.clone(), turn.message_id.clone());
                    turn.publish(AgentEvent::BackgroundTaskStarted {
                        message_id: turn.message_id.clone(),
                        task: BackgroundTask {
                            tool_id,
                            description,
                        },
                    });
                }
                Parsed::Usage(windows) => turn.server.usage.record(turn.agent, windows),
                Parsed::RequestUsage {
                    request_id,
                    usage,
                    top_level,
                } => {
                    let message_id = record_request(
                        &mut requests,
                        request_id,
                        usage,
                        top_level,
                        &turn.message_id,
                    );
                    turn.save_text(&mut unsaved);
                    turn.publish(AgentEvent::Usage {
                        usage: message_usage(&requests, &message_id),
                        message_id,
                    });
                }
                Parsed::TaskEnded { tool_id, failed } => {
                    let shell = shell_tasks.remove(&tool_id);
                    if let Some(message_id) = &shell {
                        turn.publish(AgentEvent::BackgroundTaskEnded {
                            message_id: message_id.clone(),
                            tool_id: tool_id.clone(),
                        });
                    }
                    // A shell command's call was shown finished when it returned.
                    if let Some(Some(call_failed)) = running_tasks.remove(&tool_id)
                        && shell.is_none()
                        && let Some(owner) = tool_owner.get(&tool_id)
                    {
                        turn.save_text(&mut unsaved);
                        turn.publish(AgentEvent::ToolFinished {
                            message_id: owner.clone(),
                            tool_id: tool_id.clone(),
                            is_error: failed || call_failed,
                            output: None,
                        });
                        finish_child(&turn, &mut children, &tool_id, failed || call_failed);
                    }
                    let background = running_tasks.values().any(Option::is_some);
                    if ended && can_idle(&turn, &running_tasks, &shell_tasks, &goal, &stream_error) {
                        if completed < segments.len() {
                            go_idle(&mut turn, &segments, &mut completed, &mut unsaved);
                        }
                    } else if ended && !background {
                        end_input(&turn, goal_run, &goal_round);
                    }
                }
                Parsed::TurnStarted(id) => {
                    ended = false;
                    goal_round.fetch_add(1, Ordering::SeqCst);
                    *turn
                        .native_turn
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = Some(id);
                }
                Parsed::Retrying { reason, attempt, max_attempts } => {
                    turn.emit(AgentEvent::Retrying {
                        message_id: turn.message_id.clone(),
                        reason,
                        attempt,
                        max_attempts,
                    });
                }
                Parsed::Error(message) => {
                    stream_error.get_or_insert(message);
                }
                Parsed::SessionLost => session_lost = true,
                Parsed::Event(mut event) => {
                    steer_reply_empty = false;
                    if let AgentEvent::Thinking { .. } = &event {
                        turn.stream_thinking(event, &mut unsaved);
                        continue;
                    }
                    match &mut event {
                        // Separate successive text blocks so they do not run together. Antigravity,
                        // Claude, and OpenCode's server stream a block as chunks, so chunks after the
                        // first stay together.
                        AgentEvent::TextDelta { text, .. } => {
                            let streams = match turn.agent {
                                AgentKind::Antigravity | AgentKind::Claude | AgentKind::Api => true,
                                AgentKind::OpenCode => provider::opencode::uses_serve(),
                                AgentKind::Codex => false,
                            };
                            let continues = streams && last_was_text;
                            if has_text && !continues {
                                text.insert_str(0, "\n\n");
                            }
                            has_text = true;
                        }
                        // Codex reports a tool when it starts and again when it completes;
                        // only the first ToolStarted for an id is kept.
                        AgentEvent::ToolStarted { tool, message_id } => {
                            repeats.started(tool);
                            if tool_owner.contains_key(&tool.id) {
                                continue;
                            }
                            tool_owner.insert(tool.id.clone(), message_id.clone());
                            if matches!(turn.agent, AgentKind::Claude | AgentKind::Codex)
                                && tool.parent.is_none()
                                && let Some(child) = open_child(&turn, tool)
                            {
                                tool.child_thread_id = Some(child.thread_id.clone());
                                children.insert(tool.id.clone(), child);
                            }
                        }
                        AgentEvent::ToolFinished {
                            tool_id,
                            message_id,
                            is_error,
                            output,
                        } => {
                            if let Some(name) = repeats.finished(tool_id, *is_error, output.as_deref())
                                && stream_error.is_none()
                            {
                                stream_error = Some(format!(
                                    "Stopped: the agent called {name} {LOOP_LIMIT} times in a row without getting anywhere."
                                ));
                                terminate(pid, turn.exited.clone());
                            }
                            if let Some(owner) = tool_owner.get(tool_id) {
                                message_id.clone_from(owner);
                            }
                            if let Some(held) = running_tasks.get_mut(tool_id.as_str()) {
                                *held = Some(*is_error);
                                // A backgrounded shell command's call has returned; the command
                                // itself is shown among the background tasks.
                                if !shell_tasks.contains_key(tool_id.as_str()) {
                                    continue;
                                }
                            }
                        }
                        _ => {}
                    }
                    last_was_text = matches!(event, AgentEvent::TextDelta { .. });
                    if last_was_text {
                        turn.stream_text(event, &mut unsaved);
                    } else {
                        let finished = match &event {
                            AgentEvent::ToolFinished {
                                tool_id, is_error, ..
                            } => Some((tool_id.clone(), *is_error)),
                            _ => None,
                        };
                        turn.save_text(&mut unsaved);
                        turn.publish(event);
                        if let Some((tool_id, failed)) = finished {
                            finish_child(&turn, &mut children, &tool_id, failed);
                        }
                    }
                }
            }
        }
    }
    // The stream has closed, whether the turn finished, failed, or was cancelled.
    turn.save_text(&mut unsaved);
    // An idle agent changes files only through shell commands it left running; other changes
    // since its last reply are the user's own.
    let idle_untouched = turn.idle.load(Ordering::SeqCst) && shell_tasks.is_empty();
    // The agent process is gone, and the background commands it started with it.
    shell_tasks.drain().for_each(|(tool_id, message_id)| {
        turn.publish(AgentEvent::BackgroundTaskEnded {
            message_id,
            tool_id,
        });
    });

    let exit = child.wait().ok();
    let status = exit.map_or("unknown".into(), |s| s.to_string());
    let level = if exit.is_some_and(|s| s.success()) { LogLevel::Info } else { LogLevel::Error };
    let source = Source::agent(turn.agent, pid, &turn.thread_id, Some(&turn.message_id));
    AGENTS.write(level, "exit", &source, Data::Text(status));
    // Whatever the agent left running in its group or scope (MCP servers, background
    // commands) would otherwise outlive the turn and keep its memory in the agents' slice.
    workspace::signal_process_group(pid, workspace::SIGKILL);
    if let Some(unit) = &turn.scope {
        scope::stop(unit);
    }
    turn.exited.store(true, Ordering::SeqCst);
    let tail = stderr_tail.join().unwrap_or_default();
    let cancelled = turn.cancelled.load(Ordering::SeqCst);
    let (status, error) = if cancelled {
        (MessageStatus::Cancelled, None)
    } else if turn.timed_out.load(Ordering::SeqCst) {
        let hours = TURN_TIMEOUT.as_secs() / 3600;
        (
            MessageStatus::Error,
            Some(format!(
                "The agent ran longer than {hours} hours and was stopped."
            )),
        )
    } else if stream_error.is_some() || !exit.is_some_and(|s| s.success()) {
        let error = stream_error
            .or_else(|| last_lines(&tail))
            .unwrap_or_else(|| exit_error(exit));
        (MessageStatus::Error, Some(error))
    } else {
        (MessageStatus::Complete, None)
    };
    // Subagents end with the agent that ran them.
    children.drain().for_each(|(_, child)| {
        publish(
            &turn.server,
            &child.thread_id,
            AgentEvent::TurnCompleted {
                message_id: child.message_id,
                status,
                error: None,
            },
        );
    });

    if !idle_untouched {
        let after = workspace::git_changes(&turn.root).unwrap_or_default();
        let changed = attribution::lock(&turn.attribution).settle(after);
        changed
            .into_iter()
            .for_each(|path| {
                turn.publish(AgentEvent::FileTouched {
                    message_id: turn.message_id.clone(),
                    path,
                });
            });
    }

    // An agent that was idle held no goal, and a turn that replaced it may have set one since.
    let clear_in_agent = if turn.idle.load(Ordering::SeqCst) {
        None
    } else {
        goal::settle(&turn, status, cancelled, goal_checked)
    };

    let session_lost = session_lost && !cancelled;
    finish(
        turn,
        &segments[completed..],
        TurnEnd { status, error, cancelled, clear_in_agent, session_lost },
    );
}

/// Whether Claude, having ended its reply cleanly, can stay up for the next message: nothing
/// it sent to the background but shell commands still runs, and it has no goal to pursue.
fn can_idle(
    turn: &Turn,
    running_tasks: &HashMap<String, Option<bool>>,
    shell_tasks: &HashMap<String, String>,
    goal: &Option<Goal>,
    stream_error: &Option<String>,
) -> bool {
    turn.agent == AgentKind::Claude
        && stream_error.is_none()
        && !turn.cancelled.load(Ordering::SeqCst)
        && !goal
            .as_ref()
            .is_some_and(|g| matches!(g.status, GoalStatus::Active | GoalStatus::Paused))
        && running_tasks
            .iter()
            .filter(|(_, call)| call.is_some())
            .all(|(id, _)| shell_tasks.contains_key(id))
}

/// Completes the replies not yet completed, with the files they changed, and marks the turn
/// idle: the agent stays up and takes the next message as a new reply, starting with the
/// messages queued meanwhile.
fn go_idle(turn: &mut Turn, segments: &[String], completed: &mut usize, unsaved: &mut Unsaved) {
    turn.save_text(unsaved);
    let after = workspace::git_changes(&turn.root).unwrap_or_default();
    let changed = attribution::lock(&turn.attribution).settle(after);
    changed.into_iter().for_each(|path| {
        turn.publish(AgentEvent::FileTouched {
            message_id: turn.message_id.clone(),
            path,
        });
    });
    let queued = std::mem::take(&mut *turn.queue.lock().unwrap_or_else(PoisonError::into_inner));
    let mut handed_over = turn.server.runner.handed_over.lock().unwrap_or_else(PoisonError::into_inner);
    queued.iter().for_each(|message| {
        handed_over.remove(&message.message_id);
    });
    drop(handed_over);
    record_turn(turn, &segments[*completed..]);
    segments[*completed..].iter().for_each(|id| {
        turn.publish(AgentEvent::TurnCompleted {
            message_id: id.clone(),
            status: MessageStatus::Complete,
            error: None,
        });
    });
    *completed = segments.len();
    turn.idle.store(true, Ordering::SeqCst);
    turn.server.send_queued_later(&turn.thread_id);
}

#[cfg(test)]
mod tests {
    use std::io::BufRead;
    use crate::types::AccessMode;
    use std::sync::Mutex;
    use crate::orchestration::process::AgentStdin;
    use crate::orchestration::process::Stdin;
    use std::process::Stdio;
    use crate::provider::TurnSpec;
    use super::*;

    pub(super) fn change(path: &str, status: &str, additions: u32) -> FileChange {
        FileChange {
            path: path.into(),
            status: status.into(),
            additions,
            deletions: 0,
            staged: false,
        }
    }

    #[test]
    pub(super) fn changed_paths_finds_new_and_modified_files() {
        let before = [change("a.rs", "modified", 1), change("b.rs", "modified", 2)];
        let after = [
            change("a.rs", "modified", 1),
            change("b.rs", "modified", 5),
            change("c.rs", "untracked", 3),
        ];
        assert_eq!(changed_paths(&before, &after), ["b.rs", "c.rs"]);
    }

    /// Runs a real turn through the installed CLI, allowing every approval, and
    /// checks the parsed stream. Needs the agent installed and signed in:
    /// `cargo test -- --ignored`.
    pub(super) fn live_turn(agent: AgentKind, mode: AccessMode) {
        let root = std::env::temp_dir().join(new_id());
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap();
        let ok = crate::host_command("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(ok.success());

        let spec = TurnSpec {
            root: root_str,
            session_id: None,
            model: (agent == AgentKind::Claude).then_some("haiku"),
            text: "Create a file named hello.txt whose only content is the two letters hi (no period). Do nothing else.",
            mode,
            plan: false,
            effort: None,
            attachments: &[],
            images: &[],
            instructions: None,
            skills_dir: None,
            guarded: &[],
            agents: &[],
        };
        let baseline = workspace::git_changes(root_str).unwrap();
        let input = provider::turn_input(agent, &spec);
        let mut child = provider::agent_command(agent)
            .unwrap()
            .args(provider::turn_args(agent, &spec))
            .current_dir(&root)
            .stdin(if input.is_empty() {
                Stdio::null()
            } else {
                Stdio::piped()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin: Stdin = Arc::new(Mutex::new(AgentStdin::take(&mut child, agent, "test")));
        for line in &input {
            write_line(&stdin, line);
        }

        let mut parsed = Vec::new();
        for line in BufReader::new(child.stdout.take().unwrap()).lines() {
            let Ok(value) = serde_json::from_str::<Value>(&line.unwrap()) else {
                continue;
            };
            for item in provider::parse_line(agent, &value, &spec, "m") {
                match &item {
                    Parsed::Send(line) => {
                        write_line(&stdin, line);
                    }
                    Parsed::Approval { reply, .. } => {
                        write_line(
                            &stdin,
                            &provider::approval_reply(agent, reply, true, &HashMap::new()),
                        );
                    }
                    Parsed::Finished => drop(stdin.lock().unwrap().take()),
                    _ => {}
                }
                parsed.push(item);
            }
        }
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(parsed.iter().any(|p| matches!(p, Parsed::SessionId(_))));
        assert!(
            !parsed.iter().any(|p| matches!(p, Parsed::Error(_))),
            "{parsed:?}"
        );
        if mode == AccessMode::Supervised && agent != AgentKind::Antigravity {
            assert!(parsed.iter().any(|p| matches!(p, Parsed::Approval { .. })));
        }
        let changed = changed_paths(&baseline, &workspace::git_changes(root_str).unwrap());
        assert_eq!(changed, ["hello.txt"]);
        let content = std::fs::read_to_string(root.join("hello.txt")).unwrap();
        assert_eq!(content.trim(), "hi");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[ignore = "runs the real Claude Code CLI"]
    pub(super) fn live_claude_supervised_turn() {
        live_turn(AgentKind::Claude, AccessMode::Supervised);
    }

    #[test]
    #[ignore = "runs the real Codex CLI"]
    pub(super) fn live_codex_supervised_turn() {
        live_turn(AgentKind::Codex, AccessMode::Supervised);
    }

    #[test]
    #[ignore = "runs the real Claude Code CLI"]
    pub(super) fn live_claude_turn() {
        live_turn(AgentKind::Claude, AccessMode::FullAccess);
    }

    #[test]
    #[ignore = "runs the real Codex CLI"]
    pub(super) fn live_codex_turn() {
        live_turn(AgentKind::Codex, AccessMode::FullAccess);
    }

    #[test]
    #[ignore = "runs the real Antigravity CLI"]
    pub(super) fn live_antigravity_turn() {
        live_turn(AgentKind::Antigravity, AccessMode::AutoAcceptEdits);
    }
}
