//! Watching a started agent: telling the chat when it goes quiet, and noticing when it exits.

// Rust guideline compliant 2026-11-23

use crate::system::Locked;
use super::running::Pending;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::atomic::Ordering;
use crate::system::processes::{running, scope};
use crate::Core;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::chats::MessageStatus;
use crate::system::now_ms;
use crate::chats::Role;
use crate::orchestration::turn::process::IDLE_LIMIT;
use crate::orchestration::turn::process::WATCH_EVERY;
use crate::orchestration::turn::process::QUIET_AFTER;
use crate::orchestration::turn::process::{STALL_LIMIT, terminate};
use crate::provider::AgentKind;

/// What `watch` checks one agent process by.
pub(super) struct Watched {
    pub(super) core: Arc<Core>,
    pub(super) chat_id: String,
    pub(super) pid: u32,
    pub(super) scope: Option<String>,
    pub(super) exited: Arc<AtomicBool>,
    pub(super) idle: Arc<AtomicBool>,
    pub(super) alone: Arc<AtomicBool>,
    pub(super) pending: Pending,
    pub(super) last_output: Arc<AtomicU64>,
    pub(super) agent: AgentKind,
    pub(super) calls_open: Arc<AtomicBool>,
    pub(super) stalled: Arc<AtomicBool>,
}

/// Watches one agent process until it ends. It retires the agent once it has been idle for
/// `IDLE_LIMIT` with no shell commands of its own still running; tells the user when a reply has
/// said nothing for `QUIET_AFTER`; stops an Antigravity reply that has said nothing for
/// `STALL_LIMIT` with nothing running; and ends the turn of an agent that exited while something it
/// started still holds its output open. A reply runs as long as it needs.
pub(super) fn watch(w: &Watched) {
    loop {
        std::thread::sleep(WATCH_EVERY);
        if w.exited.load(Ordering::SeqCst) {
            return;
        }
        if agent_exited(w.pid) {
            // The reader waits for the output to close, which a process the agent left behind
            // can hold open; stopping what is left closes it, and the turn ends with the exit.
            log::warn!(pid = w.pid; "agent exited with its output still open; stopping what it left");
            running::signal_process_group(w.pid, running::SIGKILL);
            if let Some(unit) = &w.scope {
                scope::stop(unit);
            }
            return;
        }
        if !w.idle.load(Ordering::SeqCst) {
            tell_quiet(w);
            if stalled(w) {
                log::warn!(pid = w.pid; "Antigravity said nothing for the stall limit; stopping it");
                w.stalled.store(true, Ordering::SeqCst);
                terminate(w.pid, Arc::clone(&w.exited));
                return;
            }
            continue;
        }
        // Antigravity at work by itself is not idle, though no message waits on it.
        if w.alone.load(Ordering::SeqCst) {
            continue;
        }
        // Idle time counts from the last reply, as idle compaction counts it.
        let retire = w.core.store.chat(&w.chat_id).is_some_and(|t| {
            let finished = t.messages.last().and_then(|m| m.finished_at).unwrap_or(0);
            let idle_ms = u64::try_from(IDLE_LIMIT.as_millis()).unwrap_or(u64::MAX);
            now_ms().saturating_sub(finished) >= idle_ms
                && t.messages
                    .iter()
                    .all(|m| m.background.iter().all(|task| !task.status.is_live()))
        });
        if retire {
            w.core.runner.retire_idle(&w.chat_id);
            return;
        }
    }
}

/// Tells the user the reply under way has said nothing for `QUIET_AFTER`, unless it waits on
/// them to answer an approval.
fn tell_quiet(w: &Watched) {
    let since = w.last_output.load(Ordering::Relaxed);
    let quiet_ms = u64::try_from(QUIET_AFTER.as_millis()).unwrap_or(u64::MAX);
    if now_ms().saturating_sub(since) < quiet_ms || !w.pending.locked().is_empty() {
        return;
    }
    let Some(chat) = w.core.store.chat(&w.chat_id) else {
        return;
    };
    let Some(reply) = chat
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::Assistant && m.status == MessageStatus::Streaming)
    else {
        return;
    };
    // A reply that started after the last output has been quiet only since it started.
    let since = since.max(reply.created_at);
    if now_ms().saturating_sub(since) < quiet_ms {
        return;
    }
    w.core.emit(AgentEventEnvelope {
        chat_id: w.chat_id.clone(),
        event: AgentEvent::Quiet {
            message_id: reply.id.clone(),
            since,
        },
    });
}

/// Whether the Antigravity reply under way has said nothing for `STALL_LIMIT` while nothing
/// it waits on runs: no call, no task in the background, and no approval for the user.
fn stalled(w: &Watched) -> bool {
    let stall_ms = u64::try_from(STALL_LIMIT.as_millis()).unwrap_or(u64::MAX);
    let since = w.last_output.load(Ordering::Relaxed);
    if w.agent != AgentKind::Antigravity
        || now_ms().saturating_sub(since) < stall_ms
        || w.calls_open.load(Ordering::Relaxed)
        || !w.pending.locked().is_empty()
    {
        return false;
    }
    w.core.store.chat(&w.chat_id).is_some_and(|chat| {
        let reply = chat
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant && m.status == MessageStatus::Streaming);
        reply.is_some_and(|reply| now_ms().saturating_sub(since.max(reply.created_at)) >= stall_ms)
            && chat
                .messages
                .iter()
                .all(|m| m.background.iter().all(|task| !task.status.is_live()))
    })
}

/// Whether the agent `pid` has exited and waits to be reaped.
#[cfg(target_os = "linux")]
fn agent_exited(pid: u32) -> bool {
    // The state follows the command name in parentheses, which may itself hold any character.
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(')')
            .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
    })
}

/// Whether the agent `pid` has exited and waits to be reaped; only Linux is checked.
#[cfg(not(target_os = "linux"))]
fn agent_exited(_pid: u32) -> bool {
    false
}
