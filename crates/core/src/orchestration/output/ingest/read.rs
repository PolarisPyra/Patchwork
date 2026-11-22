//! The loop that reads the agent's stdout for one turn and hands each event on.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::process::Child;
use std::process::ChildStdout;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

use serde_json::Value;

use crate::provider::Parsed;
use crate::provider;
use crate::system::processes::{running, scope};
use crate::events::AgentEvent;
use crate::provider::AgentKind;

/// What kind of output a line is, as its `type` (or JSON-RPC `method`) and any `subtype`.
fn line_kind(value: &Value) -> String {
    let kind = value["type"]
        .as_str()
        .or_else(|| value["method"].as_str())
        .unwrap_or("untyped");
    match value["subtype"].as_str() {
        Some(subtype) => format!("{kind}/{subtype}"),
        None => kind.into(),
    }
}

/// Publishes a subagent's events to its own thread, with its replies a paragraph apart.
fn publish_child(turn: &Turn, child: &mut SubagentChat, events: Vec<AgentEvent>) {
    for mut event in events {
        if let AgentEvent::TextDelta { text, .. } = &mut event {
            if child.has_text {
                text.insert_str(0, "\n\n");
            }
            child.has_text = true;
        }
        publish(&turn.core, &child.chat_id, event);
    }
}

use crate::chats::MessageStatus;
use crate::chats::TokenUsage;
use crate::logging::{AGENT_LOG, Data, Source};
use crate::settings::LogLevel;
use crate::source_control;
use crate::system::now_ms;

use crate::orchestration::tasks::background::Background;
use crate::orchestration::goal;
use crate::orchestration::turn::process::exit_error;
use crate::orchestration::turn::process::last_lines;
use crate::orchestration::turn::process::next_line;
use crate::orchestration::turn::process::terminate;
use crate::orchestration::turn::process::with_remedy;
use crate::orchestration::turn::stuck::{LOOP_LIMIT, Repeats};
use crate::orchestration::turn::process::write_line;
use crate::orchestration::output::stream::publish;
use crate::orchestration::output::stream::Unsaved;
use crate::orchestration::tasks::delegate::mark_call;
use crate::orchestration::tasks::subagents::finish_child;
use crate::orchestration::tasks::subagents::open_child;
use crate::orchestration::tasks::subagents::SubagentChat;
use crate::orchestration::turn::end::{TurnEnd, finish};
use crate::orchestration::turn::start::Turn;
use crate::orchestration::output::usage::message_usage;
use crate::orchestration::output::usage::record_request;
use crate::orchestration::output::usage::Request;
use crate::orchestration::output::usage::TURN_OUTPUT;
use super::approvals;
use super::idle::{can_idle, go_idle, stays_up};

/// Completes the replies a steer moved the turn on from, and counts them in `completed`, which is
/// how many of `segments` are done. Left streaming, the reply before the steer keeps its own
/// "Working" line while the answer to the steer has another, so the chat shows two turns running
/// at once. The end of the turn skips the replies counted here.
fn close_steered_segment(turn: &Turn, segments: &[String], completed: &mut usize) {
    for id in &segments[*completed..] {
        turn.publish(AgentEvent::TurnCompleted {
            message_id: id.clone(),
            status: MessageStatus::Complete,
            error: None,
        });
    }
    *completed = segments.len();
}

/// Reads the agent's stdout until it closes, then records the final status.
pub(in crate::orchestration) fn ingest(
    mut turn: Turn,
    mut child: Child,
    stdout: ChildStdout,
    stderr_tail: JoinHandle<String>,
) {
    let pid = child.id();
    let mut has_text = false;
    // Antigravity streams one text block as many deltas; other agents send whole blocks.
    let mut last_was_text = false;
    // The reply opened for a steer has nothing in it yet, so a steer right after it shares it.
    let mut steer_reply_empty = false;
    // Tool id → the message it was started in, which a steer may since have moved on from.
    let mut tool_owner: HashMap<String, String> = HashMap::new();
    // The chats of the subagents still running, by the call that started each.
    let mut children: HashMap<String, SubagentChat> = HashMap::new();
    // The tasks the agent started, and which of them are listed under the background.
    let mut background = Background::default();
    // Every message of the turn: the first reply, then one per steering message taken.
    let mut segments = vec![turn.message_id.clone()];
    // How many of `segments` were completed when the agent went idle.
    let mut completed = 0;
    let mut stream_error: Option<String> = None;
    let mut repeats = Repeats::default();
    // The agent could not find the session the turn resumed, so the turn runs again without it.
    let mut session_lost = false;
    let goal = turn.core.store.chat(&turn.chat_id).and_then(|t| t.goal);
    // The agent reported its turn finished.
    let mut ended = false;
    // Text streamed to the frontend but not yet stored; saving every delta
    // stalls the reader, so it is stored before the next event and at the end.
    let mut unsaved = Unsaved::default();
    // Every model request the turn has made; each reply's usage is summed from these.
    let mut requests: Vec<Request> = Vec::new();
    // The model a request runs on when the agent does not name it, looked up at the first one.
    let mut fallback_model: Option<Option<String>> = None;
    // Where this turn's requests start in `requests`; the agent may answer several turns.
    let mut turn_requests = 0;
    // Claude's task list, shown as the plan of whichever reply changes it.
    let mut claude_tasks = provider::claude::TaskList::default();
    // Antigravity's native ACP swarm tracker (`_antigravity/swarm` + `members.json` fallback).
    let mut antigravity_swarm = provider::antigravity::SwarmState::default();

    // Kinds of output the parser had no use for, logged once each per turn, so a CLI that
    // changes how it reports something shows up in the agent log.
    let mut logged_kinds: HashSet<String> = HashSet::new();

    let mut stdout = BufReader::new(stdout);
    while let Some(line) = next_line(&mut stdout) {
        let source =
            |turn: &Turn| Source::agent(turn.agent, pid, &turn.chat_id, Some(&turn.message_id));
        if AGENT_LOG.logs(LogLevel::Error) {
            AGENT_LOG.line("out", &source(&turn), &line);
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            if logged_kinds.insert("unparsed".into()) {
                AGENT_LOG.write(
                    LogLevel::Warning,
                    "unparsed",
                    &source(&turn),
                    Data::Outline(line),
                );
            }
            continue;
        };
        // A message Claude took after it had already ended its reply (while a background
        // subagent kept it running) is answered from here on, below that message. So is
        // a reply an idle Claude starts on its own, such as once a background command ends.
        // Claude's `tool_progress` only says a call is still running, which a stuck one is too.
        if value["type"] != "tool_progress" {
            turn.last_output.store(now_ms(), Ordering::Relaxed);
        }
        let starts_reply = value["type"] == "system" && value["subtype"] == "init";
        if ended
            && turn.agent == AgentKind::Claude
            && ((starts_reply && turn.idle.swap(false, Ordering::SeqCst))
                || !turn.steers.locked().is_empty())
        {
            turn.steers.locked().clear();
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
        // chat. They go to its chat alone: its finished turn is not this one's. Its requests,
        // such as approvals, are answered here like any other.
        if turn.agent == AgentKind::Codex
            && value.get("id").is_none()
            && let Some(chat) = value["params"]["threadId"].as_str()
            && turn.session_id.as_deref().is_some_and(|own| own != chat)
        {
            if let Some(child) = children.get_mut(chat) {
                let events =
                    provider::codex::subagent_events(&value, &turn.spec(), &child.message_id);
                publish_child(&turn, child, events);
            }
            continue;
        }
        // Antigravity ACP sends native swarm roster/board notifications on `_antigravity/swarm`,
        // and routes child worker `session/update` frames tagged with `childSessionId` /
        // `workerSessionId` to each worker's child thread.
        if turn.agent == AgentKind::Antigravity
            && value.get("id").is_none()
            && value["method"].as_str() == Some("session/update")
            && let Some(child_target) =
                provider::antigravity::extract_child_target(&value, turn.session_id.as_deref())
        {
            let key = format!(
                "{}{}",
                provider::antigravity::SWARM_TASK_PREFIX,
                child_target.child_session_id
            );
            let child_key = if children.contains_key(&key) {
                key.as_str()
            } else {
                child_target.child_session_id.as_str()
            };
            if let Some(child) = children.get_mut(child_key) {
                let events =
                    provider::antigravity::subagent_events(&value, &turn.spec(), &child.message_id);
                publish_child(&turn, child, events);
            }
            continue;
        }
        let mut parsed = if turn.agent == AgentKind::Antigravity
            && value.get("id").is_none()
            && value["method"].as_str() == Some("_antigravity/swarm")
        {
            antigravity_swarm.apply_notification(&value["params"], &turn.message_id)
        } else {
            provider::parse_line(turn.agent, &value, &turn.spec(), &turn.message_id)
        };
        if turn.agent == AgentKind::Antigravity
            && value["method"].as_str() == Some("session/update")
            && matches!(
                value["params"]["update"]["name"].as_str(),
                Some("swarm" | "post_message" | "read_board" | "swarm_wait")
            )
        {
            antigravity_swarm.note_swarm_tool();
            if matches!(
                value["params"]["update"]["status"].as_str(),
                Some("completed" | "failed")
            ) && let Some(sid) = turn.session_id.as_deref()
            {
                parsed.extend(antigravity_swarm.poll_disk_fallback(sid, &turn.message_id));
            }
        }
        if turn.agent == AgentKind::Antigravity
            && parsed.iter().any(|p| matches!(p, Parsed::Finished))
            && let Some(sid) = turn.session_id.as_deref()
        {
            // Run the disk fallback BEFORE `Parsed::Finished` decides whether to close stdin, so
            // released `agy` 1.5.x builds without `_antigravity/swarm` keep swarm workers alive.
            let fallback = antigravity_swarm.poll_disk_fallback(sid, &turn.message_id);
            if !fallback.is_empty() {
                let mut combined = fallback;
                combined.extend(parsed);
                parsed = combined;
            }
        }
        if turn.agent == AgentKind::Claude
            && let Some(steps) = claude_tasks.read(&value)
        {
            parsed.push(Parsed::Event(AgentEvent::PlanUpdated {
                message_id: turn.message_id.clone(),
                steps,
            }));
        }
        // A subagent's frames went to its own thread above.
        let routed = turn.agent == AgentKind::Claude && value["parent_tool_use_id"].is_string();
        if parsed.is_empty() && !routed && AGENT_LOG.logs(LogLevel::Debug) {
            let kind = line_kind(&value);
            if logged_kinds.insert(kind.clone()) {
                AGENT_LOG.write(LogLevel::Debug, "ignored", &source(&turn), Data::Text(kind));
            }
        }
        for item in parsed {
            let Some(item) = approvals::screen(&turn, item) else {
                continue;
            };
            match item {
                Parsed::Send(line) => {
                    write_line(&turn.stdin, &line);
                }
                Parsed::McpCall {
                    id,
                    server,
                    tool,
                    arguments,
                } => approvals::call_tool(&turn, id, server, tool, arguments),
                Parsed::Approval {
                    title,
                    detail,
                    reply,
                    questions,
                    target,
                } => approvals::request_approval(
                    &turn,
                    &mut unsaved,
                    title,
                    detail,
                    reply,
                    questions,
                    target,
                ),
                // Answered above.
                Parsed::Check { .. } => {}
                Parsed::Finished => {
                    ended = true;
                    // Stop interrupted the agent and it ended its reply. Claude stays up for the
                    // next message, as after any reply; Codex exits once its stdin closes. Until
                    // the agent is idle or has exited, Stop still sends SIGTERM if it hangs.
                    if turn.cancelled.load(Ordering::SeqCst) && turn.interrupts.waiting() {
                        if stays_up(&turn, goal.as_ref()) {
                            turn.interrupts.answer();
                            // An error in the stopped reply does not carry over to the next one.
                            stream_error = None;
                            go_idle(&turn, &segments, &mut completed, &mut unsaved, true, None);
                        } else if matches!(turn.agent, AgentKind::Codex | AgentKind::Antigravity) {
                            turn.stdin.locked().take();
                        } else {
                            // What it left running toward a goal or in the background stops too.
                            terminate(pid, Arc::clone(&turn.exited));
                        }
                        continue;
                    }
                    let steered = !turn.steers.locked().is_empty();
                    // Claude ends its reply to take the steering messages, then answers them.
                    if steered && turn.agent == AgentKind::Claude {
                        turn.steers.locked().clear();
                        close_steered_segment(&turn, &segments, &mut completed);
                        turn.next_segment(&mut unsaved);
                        segments.push(turn.message_id.clone());
                        (has_text, last_was_text) = (false, false);
                        // Its answer ends with another result; stdin stays open for more.
                        ended = false;
                        continue;
                    }
                    // The finished turn can no longer be steered; until Codex starts the
                    // next one, messages wait for the run to end.
                    *turn.native.locked() = None;
                    // Codex's calls still open are what its turn left running, so they are listed
                    // under the background now.
                    if turn.agent == AgentKind::Codex {
                        for event in background.list_unreported(&turn.message_id) {
                            publish_event(&turn, &mut children, &mut unsaved, event);
                        }
                    }
                    // Claude stays up idle for the next message, so its session and connectors
                    // are not set up again for each one. It takes messages meanwhile, including
                    // while a task it started keeps running.
                    if can_idle(&turn, goal.as_ref(), stream_error.as_ref()) {
                        go_idle(&turn, &segments, &mut completed, &mut unsaved, false, None);
                    } else if !background.keeps_stdin_open() {
                        turn.close_unless_steered();
                    } else if can_idle(&turn, goal.as_ref(), None) {
                        // A reply that failed while a command it started runs on ends now, with its
                        // error. Claude stays up with the command, as after a reply that did not fail,
                        // since the command would otherwise hold the reply open until it ends.
                        let error = stream_error.take().map(|e| with_remedy(turn.agent, e));
                        go_idle(
                            &turn,
                            &segments,
                            &mut completed,
                            &mut unsaved,
                            false,
                            error.as_deref(),
                        );
                    }
                }
                Parsed::ControlResponse(id) => {
                    let waiting = turn.controls.locked().remove(&id);
                    if let Some(waiting) = waiting {
                        let _ = waiting.send(());
                    }
                }
                Parsed::Steered => {
                    turn.steers.locked().pop_front();
                    if !steer_reply_empty {
                        // The reply the steer arrived during is over; the turn goes on below it.
                        close_steered_segment(&turn, &segments, &mut completed);
                        turn.next_segment(&mut unsaved);
                        segments.push(turn.message_id.clone());
                        (has_text, last_was_text) = (false, false);
                        steer_reply_empty = true;
                    }
                    if ended {
                        turn.close_unless_steered();
                    }
                }
                Parsed::SteerRejected(message) => {
                    log::warn!(
                        message:%;
                        "the agent did not take the steering message: {{message}}"
                    );
                    let refused = turn.steers.locked().pop_front();
                    if let Some(refused) = refused {
                        // Waits as a queued message, so the turn's end sends it on.
                        turn.publish(AgentEvent::MessageQueued {
                            message_id: refused.message_id.clone(),
                            queued: true,
                        });
                        turn.queue.locked().push(refused);
                    }
                    if ended {
                        turn.close_unless_steered();
                    }
                }
                Parsed::SessionId(id) => {
                    let store = &turn.core.store;
                    if let Err(e) =
                        store.update_chat(&turn.chat_id, |t| t.session_id = Some(id.clone()))
                    {
                        log::warn!(error:% = e; "failed to save session id: {{error}}");
                    }
                    // The session has gone on from an undo's or a fork's point, and a later
                    // turn resumes where it is now.
                    if turn.agent == AgentKind::Claude
                        && let Err(e) = turn.core.set_resume_point(&turn.chat_id, None)
                    {
                        log::warn!(error:% = e; "failed to forget a resume point: {{error}}");
                    }
                    turn.session_id = Some(id);
                }
                Parsed::SlashCommands(commands) => {
                    turn.publish(AgentEvent::SlashCommandsUpdated { commands });
                }
                Parsed::Task(event) => {
                    for event in background.apply(event, &turn.message_id) {
                        publish_event(&turn, &mut children, &mut unsaved, event);
                    }
                    // A reply that has ended goes idle, or closes stdin, once no task holds it open.
                    if ended && can_idle(&turn, goal.as_ref(), stream_error.as_ref()) {
                        if completed < segments.len() {
                            go_idle(&turn, &segments, &mut completed, &mut unsaved, false, None);
                        }
                    } else if ended && !background.keeps_stdin_open() {
                        turn.close_unless_steered();
                    }
                }
                Parsed::Usage(windows) => turn.core.usage.record(turn.agent, windows),
                Parsed::RequestUsage {
                    request_id,
                    mut usage,
                    top_level,
                    model,
                } => {
                    if usage.cost_micros.is_none() {
                        let model = model.or_else(|| {
                            fallback_model
                                .get_or_insert_with(|| {
                                    crate::costs::turn_model(turn.agent, turn.model.as_deref())
                                })
                                .clone()
                        });
                        usage.cost_micros = crate::costs::priced(model.as_deref(), &usage);
                    }
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
                Parsed::TurnOutput(total) => {
                    let counted: u64 = requests[turn_requests..]
                        .iter()
                        .filter(|r| r.top_level)
                        .map(|r| r.usage.output)
                        .sum();
                    if total > counted {
                        let mut usage = TokenUsage {
                            output: total - counted,
                            ..TokenUsage::default()
                        };
                        let model = fallback_model
                            .get_or_insert_with(|| {
                                crate::costs::turn_model(turn.agent, turn.model.as_deref())
                            })
                            .clone();
                        usage.cost_micros = crate::costs::priced(model.as_deref(), &usage);
                        let id = format!("{TURN_OUTPUT}{}", requests.len());
                        let message_id =
                            record_request(&mut requests, id, usage, false, &turn.message_id);
                        turn.publish(AgentEvent::Usage {
                            usage: message_usage(&requests, &message_id),
                            message_id,
                        });
                    }
                    turn_requests = requests.len();
                }
                Parsed::TurnStarted(id) => {
                    ended = false;
                    *turn.native.locked() = Some(id);
                }
                Parsed::Compacting => {
                    turn.save_text(&mut unsaved);
                    turn.publish(AgentEvent::Compacting {
                        message_id: turn.message_id.clone(),
                    });
                }
                Parsed::Compacted(summary) => {
                    turn.publish(AgentEvent::Compacted {
                        message_id: turn.message_id.clone(),
                        summary,
                    });
                }
                Parsed::Retrying {
                    reason,
                    attempt,
                    max_attempts,
                } => {
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
                // A reply Stop interrupted ends as stopped, not as cut short by a steering message.
                Parsed::Event(AgentEvent::MessageCutOff { .. })
                    if turn.cancelled.load(Ordering::SeqCst) => {}
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
                            if turn.agent == AgentKind::Codex
                                && let Some(task) = provider::codex::background_task(tool)
                            {
                                background.start_unreported(task);
                            }
                            if matches!(
                                turn.agent,
                                AgentKind::Claude | AgentKind::Codex | AgentKind::Antigravity
                            ) && tool.parent.is_none()
                                && let Some(child) = open_child(&turn, tool)
                            {
                                tool.child_chat_id = Some(child.chat_id.clone());
                                children.insert(tool.id.clone(), child);
                            }
                            mark_call(&turn, tool);
                        }
                        AgentEvent::ToolFinished {
                            tool_id,
                            message_id,
                            is_error,
                            output,
                        } => {
                            if let Some(name) =
                                repeats.finished(tool_id, *is_error, output.as_deref())
                                && stream_error.is_none()
                            {
                                stream_error = Some(format!(
                                    "Stopped: the agent called {name} {LOOP_LIMIT} times in a row without getting anywhere."
                                ));
                                terminate(pid, Arc::clone(&turn.exited));
                            }
                            if let Some(owner) = tool_owner.get(tool_id) {
                                message_id.clone_from(owner);
                            }
                            // A call whose task still runs is shown finished when the task ends.
                            if background.call_returned(tool_id, message_id, *is_error) {
                                continue;
                            }
                        }
                        AgentEvent::FileTouched { .. } => repeats.changed_file(),
                        _ => {}
                    }
                    last_was_text = matches!(event, AgentEvent::TextDelta { .. });
                    if last_was_text {
                        turn.stream_text(event, &mut unsaved);
                    } else {
                        let finished = match &event {
                            AgentEvent::ToolFinished {
                                tool_id, is_error, ..
                            } if turn.agent == AgentKind::Codex => {
                                Some((tool_id.clone(), *is_error))
                            }
                            _ => None,
                        };
                        publish_event(&turn, &mut children, &mut unsaved, event);
                        if let Some((tool_id, failed)) = finished {
                            for event in background.finish_unreported(&tool_id, failed) {
                                publish_event(&turn, &mut children, &mut unsaved, event);
                            }
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
    let idle_untouched = turn.idle.load(Ordering::SeqCst) && !background.any_listed_live();
    // If `agy acp` exited unexpectedly while swarm workers were still running, surface an error
    // instead of silently completing an unfinished swarm.
    if turn.agent == AgentKind::Antigravity
        && !turn.cancelled.load(Ordering::SeqCst)
        && stream_error.is_none()
        && antigravity_swarm.has_live_work()
    {
        stream_error = Some("Antigravity exited while swarm workers were still running.".into());
    }
    // The agent process is gone, and the tasks it started go with it.
    for event in background.stop_all() {
        publish_event(&turn, &mut children, &mut unsaved, event);
    }
    let exit = child.wait().ok();
    let status = exit.map_or_else(|| "unknown".into(), |s| s.to_string());
    // A turn the user stopped, or the app quit, ends by signal: that is not a failure.
    let stopped = turn.cancelled.load(Ordering::SeqCst);
    let level = if stopped || exit.is_some_and(|s| s.success()) {
        LogLevel::Info
    } else {
        LogLevel::Error
    };
    let source = Source::agent(turn.agent, pid, &turn.chat_id, Some(&turn.message_id));
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
            AgentEvent::TurnCompleted {
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
            turn.publish(AgentEvent::FileTouched {
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

/// Publishes one event of the turn. A subagent whose call finishes also closes its chat.
fn publish_event(
    turn: &Turn,
    children: &mut HashMap<String, SubagentChat>,
    unsaved: &mut Unsaved,
    event: AgentEvent,
) {
    turn.save_text(unsaved);
    let finished = match &event {
        AgentEvent::ToolFinished {
            tool_id, is_error, ..
        } => Some((tool_id.clone(), *is_error)),
        _ => None,
    };
    turn.publish(event);
    if let Some((tool_id, failed)) = finished {
        finish_child(turn, children, &tool_id, failed);
    }
}
