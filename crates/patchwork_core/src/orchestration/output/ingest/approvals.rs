//! The calls an agent asks to make: those its access mode runs without asking, the tools Patchwork
//! serves it, and the approvals the user answers.

// Rust guideline compliant 2026-10-09

use crate::chats::{ApprovalExt};
use crate::system::Locked;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde_json::Value;

use crate::events::ChatEvent;
use crate::orchestration::decide;
use crate::orchestration::output::stream::Unsaved;
use crate::orchestration::tasks::jobs;
use crate::orchestration::turn::process::write_line;
use crate::orchestration::turn::start::{PendingApproval, Turn};
use crate::orchestration::unasked;
use crate::permissions;
use crate::provider::{self, Parsed, ProviderKind, antigravity};
use crate::chats::{Approval, ApprovalStatus, Question};

/// A call the access mode runs without asking runs unless a rule refuses it or asks. Returns the
/// item to handle: the approval to ask for in place of such a call, or None once it has run.
pub(super) fn screen(turn: &Turn, item: Parsed) -> Option<Parsed> {
    let Parsed::Check {
        title,
        detail,
        reply,
        target,
    } = item
    else {
        return Some(item);
    };
    // After Stop, cancel incoming permission checks (including swarm worker requests) instead of
    // auto-approving them.
    if turn.cancelled.load(Ordering::SeqCst) {
        let mut cancel_reply = reply;
        cancel_reply["cancel"] = Value::Bool(true);
        write_line(
            &turn.stdin,
            &provider::approval_reply(turn.agent, &cancel_reply, false, &HashMap::new()),
        );
        return None;
    }
    let asks = *turn.mode.locked() != crate::chats::AccessMode::FullAccess;
    // A tool the agent blocks goes on to be refused as an approval, where the chat shows why.
    let blocked = turn
        .core
        .agent_refuses(&turn.chat_id, Path::new(&turn.root), &target);
    if !blocked
        && unasked(&turn.core, &turn.chat_id, &turn.root, &target, asks)
            == permissions::Unasked::Run
    {
        write_line(
            &turn.stdin,
            &provider::approval_reply(turn.agent, &reply, true, &HashMap::new()),
        );
        return None;
    }
    Some(Parsed::Approval {
        title,
        detail,
        reply,
        questions: Vec::new(),
        target: Some(target),
    })
}

/// Runs a call to an MCP tool off the reading thread and writes its result back to the agent.
pub(super) fn call_tool(turn: &Turn, id: Value, server: String, tool: String, arguments: Value) {
    let stdin = Arc::clone(&turn.stdin);
    let (cancelled, exited) = (Arc::clone(&turn.cancelled), Arc::clone(&turn.exited));
    let chat_id = turn.chat_id.clone();
    // Patchwork's own agent has its commands run here, so they can outlive it.
    let job = (turn.agent.capabilities().patchwork_runs_commands
        && server == provider::mcp::OWN_SERVER
        && jobs::TOOLS.contains(&tool.as_str()))
    .then(|| {
        (
            Arc::clone(&turn.core),
            jobs::Origin {
                chat_id: turn.chat_id.clone(),
                message_id: turn.message_id.clone(),
                tool_id: id.as_str().unwrap_or_default().to_string(),
                root: turn.root.clone(),
            },
        )
    });
    std::thread::spawn(move || {
        let stopped = || cancelled.load(Ordering::SeqCst) || exited.load(Ordering::SeqCst);
        let result = if let Some((app, from)) = job {
            jobs::call(&app, &from, &tool, &arguments, &stopped)
        } else if server == provider::mcp::OWN_SERVER {
            provider::mcp::call_own(&chat_id, &tool, &arguments, &stopped)
        } else {
            provider::mcp::call(&server, &tool, &arguments, &stopped)
        };
        write_line(&stdin, &provider::api::mcp_reply(&id, result));
    });
}

/// Why a hook refuses the call a request asks to make, if one does.
///
/// An agent with no hooks of its own asks Patchwork before these calls, so Patchwork runs the
/// user's hooks for before a tool call here, as the other agents run them themselves.
fn refused_by_hook(turn: &Turn, target: &permissions::Target) -> Option<String> {
    let event = crate::extensions::hooks::HookEvent::PreToolUse;
    if !turn.agent.capabilities().patchwork_runs_hooks {
        return None;
    }
    let hooks = crate::extensions::hooks::agent_hooks(&turn.core.store.settings());
    let root = crate::system::ssh::here(&turn.root);
    target.resources.iter().find_map(|resource| {
        let (tool, input) = provider::hooks::opencode_call(target.kind, resource)?;
        let call = serde_json::json!({
            "session_id": turn.session_id,
            "cwd": root,
            "hook_event_name": event.cli_name(),
            "tool_name": tool,
            "tool_input": input,
        });
        hooks
            .iter()
            .filter(|hook| hook.event == event && hook.matches(&tool))
            .find_map(|hook| crate::extensions::hooks::run_tool_hook(hook, &call, &turn.root))
    })
}

/// Asks the user to approve a call, unless a hook, a grant from a permission hook, or a
/// permission rule answers it first.
pub(super) fn request_approval(
    turn: &Turn,
    unsaved: &mut Unsaved,
    title: String,
    detail: String,
    reply: Value,
    questions: Vec<Question>,
    target: Option<permissions::Target>,
) {
    // A question the agent ended its reply on outlives the reply: no turn holds it, so answering
    // it sends the answer as the next message (see `answer_in_new_turn`). There is nothing to
    // write back to the agent, so a Stop leaves it unasked.
    if reply.get(antigravity::BY_MESSAGE).is_some() {
        if !turn.cancelled.load(Ordering::SeqCst) {
            turn.save_text(unsaved);
            turn.publish(ChatEvent::ApprovalRequested {
                message_id: turn.message_id.clone(),
                approval: Approval {
                    questions,
                    by_message: true,
                    ..Approval::pending(title, detail)
                },
            });
        }
        return;
    }
    if turn.cancelled.load(Ordering::SeqCst) {
        let mut cancel_reply = reply;
        cancel_reply["cancel"] = Value::Bool(true);
        write_line(
            &turn.stdin,
            &provider::approval_reply(turn.agent, &cancel_reply, false, &HashMap::new()),
        );
        return;
    }
    let mut approval = Approval {
        questions,
        kind: target.as_ref().map(|t| t.kind),
        suggestions: target
            .as_ref()
            .map(|t| permissions::suggest(t, Path::new(&turn.root)))
            .unwrap_or_default(),
        ..Approval::pending(title, detail)
    };
    // The agent blocks it, or a hook refuses it, whatever the rules and the user would say.
    let refused = target.as_ref().and_then(|t| {
        if turn
            .core
            .agent_refuses(&turn.chat_id, Path::new(&turn.root), t)
        {
            Some(crate::agents::BLOCKED_REASON.to_string())
        } else {
            refused_by_hook(turn, t)
        }
    });
    if let Some(reason) = refused {
        let mut reply = reply;
        reply["message"] = Value::from(reason.as_str());
        write_line(
            &turn.stdin,
            &provider::approval_reply(turn.agent, &reply, false, &HashMap::new()),
        );
        approval.detail = format!("{reason}: {}", approval.detail);
        approval.status = ApprovalStatus::Denied;
        approval.auto = true;
        turn.save_text(unsaved);
        turn.publish(ChatEvent::ApprovalRequested {
            message_id: turn.message_id.clone(),
            approval,
        });
        return;
    }
    // The user already allowed it through a permission hook.
    if target.as_ref().is_some_and(|t| {
        turn.core
            .runner
            .take_grant(&turn.chat_id, t, Path::new(&turn.root))
    }) {
        write_line(
            &turn.stdin,
            &provider::approval_reply(turn.agent, &reply, true, &HashMap::new()),
        );
        return;
    }
    // A permission rule answers it without asking.
    if let Some(decision) = target
        .as_ref()
        .and_then(|t| decide(&turn.core, &turn.chat_id, &turn.root, t))
    {
        write_line(
            &turn.stdin,
            &provider::approval_reply(turn.agent, &reply, decision.allow, &HashMap::new()),
        );
        approval.status = if decision.allow {
            ApprovalStatus::Allowed
        } else {
            ApprovalStatus::Denied
        };
        approval.rule = Some(decision.rule);
        approval.auto = true;
        turn.save_text(unsaved);
        turn.publish(ChatEvent::ApprovalRequested {
            message_id: turn.message_id.clone(),
            approval,
        });
        return;
    }
    turn.pending.locked().insert(
        approval.id.clone(),
        PendingApproval {
            message_id: turn.message_id.clone(),
            reply,
            target,
            root: turn.root.clone(),
        },
    );
    turn.save_text(unsaved);
    turn.publish(ChatEvent::ApprovalRequested {
        message_id: turn.message_id.clone(),
        approval,
    });
}
