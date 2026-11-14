//! Sub agents on another agent than the chat's: the chat's agent calls Patchwork's
//! `run_sub_agent` tool (see `crate::threads::delegate`), and the work runs as a turn in a chat of
//! the other agent's, whose last reply is the tool's result.

// Rust guideline compliant 2026-10-09

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::orchestration::Runner;
use super::wait::Unanswered;
use crate::orchestration::turn::start::Turn;
use crate::system::{new_id, now_ms};
use crate::provider::AgentKind;
use crate::threads::delegate::{self, Ask, RESULT};
use crate::threads::{
    AgentPick, Message, MessageStatus, SubAgent, Thread, ThreadParent, ToolCall, sub_agent_of,
};
use crate::{Error, Server};

/// How often a call that has just arrived looks for its card in the caller's reply.
const LINK_POLL: Duration = Duration::from_millis(50);

/// How long a call waits for its card to show in the caller's reply. The CLI reports the call as
/// it makes it, so the two arrive together; past this the sub agent runs without a card.
const LINK_WAIT: Duration = Duration::from_secs(3);

/// The longest a sub agent runs. Under the 65 minutes a CLI waits on one of Patchwork's own
/// tools (`OWN_TIMEOUT_SECS`), so the caller hears that it ran out rather than a dropped call.
const MAX_RUN: Duration = Duration::from_secs(60 * 60);

/// A call to the tool as the caller's reply shows it, until the sub agent it starts takes it up.
pub(in crate::orchestration) struct Called {
    tool_id: String,
    /// The thread the call's card opens, which the sub agent is then made as.
    child_id: String,
    summary: String,
}

/// Makes a call to the tool a sub agent's card as it is first shown, opening a thread that the
/// sub agent is about to be made as.
pub(in crate::orchestration) fn mark_call(turn: &Turn, tool: &mut ToolCall) {
    if tool.parent.is_some() || !delegate::is_call(&tool.name, &tool.summary) {
        return;
    }
    let store = &turn.server.store;
    let Some(pick) = store
        .thread(&turn.thread_id)
        .and_then(|thread| sub_agent_of(store, &store.settings(), &thread))
    else {
        return;
    };
    let child_id = new_id();
    tool.subagent = Some(SubAgent {
        kind: format!("{:?}", pick.agent),
        description: delegate::shown(&tool.summary).to_string(),
        model: pick.model,
        prompt: None,
    });
    tool.child_thread_id = Some(child_id.clone());
    turn.server
        .runner
        .delegated
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(turn.thread_id.clone())
        .or_default()
        .push(Called {
            tool_id: tool.id.clone(),
            child_id,
            summary: tool.summary.clone(),
        });
}

/// The sub agent's own thread: read-only, and opened from the call that started it.
fn child_thread(parent: Thread, pick: AgentPick, ask: &Ask, call: Option<Called>) -> Thread {
    let (tool_id, id) = call.map_or_else(
        || (String::new(), new_id()),
        |call| (call.tool_id, call.child_id),
    );
    let title = if ask.description.is_empty() {
        "Sub agent".to_string()
    } else {
        ask.description.clone()
    };
    Thread {
        id,
        project_id: parent.project_id,
        title,
        agent: pick.agent,
        model: pick.model,
        access_mode: parent.access_mode,
        effort: pick.effort,
        compact_window: None,
        background: parent.background,
        session_id: None,
        created_at: now_ms(),
        messages: Vec::new(),
        goal: None,
        position: 0,
        folder_id: None,
        archived_at: None,
        parent: Some(ThreadParent {
            thread_id: parent.id,
            tool_id,
        }),
        plan_mode: false,
        slash_commands: vec![],
        imported_from: None,
        context_pending: false,
        role: None,
        role_pending: false,
        favorite: false,
        pinned: false,
    }
}

/// The tool's result from the reply the sub agent's turn ended with.
fn result(reply: Message) -> Result<String, Error> {
    let text = reply.text.trim();
    match reply.status {
        MessageStatus::Complete if text.is_empty() => {
            Ok("The sub agent finished without a reply.".into())
        }
        MessageStatus::Complete => Ok(text.to_string()),
        MessageStatus::Error => Err(reply
            .error
            .unwrap_or_else(|| "The sub agent's turn failed".into())
            .into()),
        _ => Err("The sub agent was stopped before it finished".into()),
    }
}

impl Runner {
    /// Takes the card of a call that has just arrived: the one whose summary holds
    /// `description`, else the oldest waiting. None when none shows within `LINK_WAIT`.
    fn claim_call(
        &self,
        thread_id: &str,
        description: &str,
        stopped: &dyn Fn() -> bool,
    ) -> Option<Called> {
        let deadline = Instant::now() + LINK_WAIT;
        loop {
            {
                let mut waiting = self
                    .delegated
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if let Some(calls) = waiting.get_mut(thread_id).filter(|c| !c.is_empty()) {
                    let at = calls
                        .iter()
                        .position(|c| !description.is_empty() && c.summary.contains(description))
                        .unwrap_or(0);
                    let call = calls.remove(at);
                    if calls.is_empty() {
                        waiting.remove(thread_id);
                    }
                    return Some(call);
                }
            }
            if stopped() || Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(LINK_POLL);
        }
    }

    /// Runs a sub agent for the thread's agent on the agent its chat names for them, and waits
    /// for its answer: until its turn ends, the caller's turn ends or `stopped`, or `MAX_RUN`.
    ///
    /// # Errors
    ///
    /// Returns an error when the arguments give no prompt, the chat's sub agents run on its own
    /// agent, the sub agent cannot be started, or it ends without finishing its work.
    pub(crate) fn run_sub_agent(
        &self,
        server: &Arc<Server>,
        thread_id: &str,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let ask = match Ask::parse(args) {
            Ok(ask) => ask,
            Err(e) => {
                // Its card is taken all the same, or the next call would be shown as this one.
                self.claim_call(thread_id, "", stopped);
                return Err(e);
            }
        };
        let parent = server
            .store
            .thread(thread_id)
            .ok_or("This chat is no longer here")?;
        let pick = sub_agent_of(&server.store, &server.store.settings(), &parent)
            .ok_or("This chat's sub agents run on its own agent; start one with your own tool")?;
        // Only Patchwork's own agent is offered the background, and the tool that ends it.
        let background = ask.background && parent.agent == AgentKind::Api;
        let flags = self.turn_flags(thread_id)?;
        let gone = || stopped() || flags.iter().any(|f| f.load(Ordering::SeqCst));
        let call = self.claim_call(thread_id, &ask.description, &gone);
        let thread = child_thread(parent, pick, &ask, call);
        let child_id = thread.id.clone();
        server.store.insert_threads(vec![thread.clone()])?;
        server.emit(AgentEventEnvelope {
            thread_id: child_id.clone(),
            event: AgentEvent::ThreadCreated { thread },
        });
        let sent = server.send_message(child_id.clone(), ask.prompt, Vec::new(), false)?;
        if !background {
            return self.await_child(server, &child_id, &sent.id, &gone);
        }
        // It outlives the call, and the turn when that ends on its own: only stopping the turn
        // stops it. Its answer is read from its chat by `sub_agent_result`.
        let (server, child, [cancelled, _]) = (Arc::clone(server), child_id.clone(), flags);
        std::thread::spawn(move || {
            let gone = || cancelled.load(Ordering::SeqCst);
            let _ = server.runner.await_child(&server, &child, &sent.id, &gone);
        });
        Ok(format!(
            "Started sub agent {child_id}. Carry on with your own work, and call {RESULT} with this id for its answer."
        ))
    }

    /// Waits for the answer of a sub agent the thread's agent started in the background, while
    /// the caller's turn runs. The sub agent is left running when the caller stops waiting.
    ///
    /// # Errors
    ///
    /// Returns an error when the arguments name no sub agent of this chat, the caller's turn
    /// ends first, or the sub agent ends without finishing its work.
    pub(crate) fn sub_agent_result(
        &self,
        server: &Arc<Server>,
        thread_id: &str,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let id = args["sub_agent_id"]
            .as_str()
            .map(str::trim)
            .unwrap_or_default();
        let child = server
            .store
            .thread(id)
            .filter(|t| t.parent.as_ref().is_some_and(|p| p.thread_id == thread_id))
            .ok_or("No sub agent with that id in this chat")?;
        let flags = self.turn_flags(thread_id)?;
        let gone = || stopped() || flags.iter().any(|f| f.load(Ordering::SeqCst));
        self.wait_for_reply(server, &child.id, None, &gone, MAX_RUN)
            .map_err(|why| {
                Error::from(match why {
                    Unanswered::Stopped => "Stopped waiting with this turn",
                    Unanswered::TimedOut => "The sub agent was still working after an hour",
                    Unanswered::NotStarted => "The sub agent did not start",
                })
            })
            .and_then(result)
    }

    /// The flags of the turn running in the thread: stopped by the user, and its agent exited.
    fn turn_flags(&self, thread_id: &str) -> Result<[Arc<AtomicBool>; 2], Error> {
        let turns = self.turns.lock().unwrap_or_else(PoisonError::into_inner);
        let turn = turns
            .get(thread_id)
            .ok_or("This turn is no longer running")?;
        Ok([Arc::clone(&turn.cancelled), Arc::clone(&turn.exited)])
    }

    /// Waits for the sub agent's turn in `child_id` to end and returns its answer to the message
    /// `sent_id`. The turn is stopped when `gone` or at `MAX_RUN`.
    fn await_child(
        &self,
        server: &Arc<Server>,
        child_id: &str,
        sent_id: &str,
        gone: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let reply = self
            .wait_for_reply(server, child_id, Some(sent_id), gone, MAX_RUN)
            .map_err(|why| {
                if why != Unanswered::NotStarted {
                    server.stop_turn(child_id, false);
                }
                Error::from(match why {
                    Unanswered::Stopped => {
                        "The sub agent was stopped with the turn that started it"
                    }
                    Unanswered::TimedOut => {
                        "The sub agent ran for an hour without finishing, and was stopped"
                    }
                    Unanswered::NotStarted => "The sub agent did not start",
                })
            });
        // Its work is done, so the agent is not kept up for a message that will not come.
        self.retire_idle(child_id);
        reply.and_then(result)
    }
}
