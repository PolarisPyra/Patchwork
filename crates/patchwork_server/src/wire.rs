//! The core's chats and messages in the shapes the protocol has.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;

use patchwork_protocol::chats as client;
use patchwork_protocol::messages::{self as wire, ApprovalState, PlanStepStatus, ToolStatus};
use serde_json::Value;

use patchwork_core::Core;
use patchwork_core::chats::{ApprovalStatus, Chat, Message, MessageStatus, Part, StepStatus};
use patchwork_core::provider::AgentKind;

/// An agent's id as the web client has it: its serde name.
pub(super) fn agent_id(agent: AgentKind) -> String {
    match serde_json::to_value(agent) {
        Ok(Value::String(id)) => id,
        _ => String::new(),
    }
}

/// A chat as the web client's `Chat` type has it.
pub(super) fn client_chat(core: &Core, chat: &Chat) -> client::Chat {
    let running = core.turn_live(&chat.id);
    client::Chat {
        id: chat.id.clone(),
        project_id: chat.project_id.clone(),
        title: chat.title.clone(),
        agent: agent_id(chat.agent),
        model: chat.model.clone(),
        access_mode: chat.access_mode,
        session_id: chat.session_id.clone(),
        effort: chat.effort.clone(),
        status: chat.client_status(running),
        position: chat.position,
        created_at: chat.created_at,
        updated_at: patchwork_core::chats::tools::last_active(chat),
        archived_at: chat.archived_at,
        pinned: chat.pinned,
        worktree_path: None,
        branch: None,
        checkpoint_ref: None,
    }
}

/// A message as the web client's `Message` type has it.
pub(super) fn client_message(chat_id: &str, m: &Message) -> wire::Message {
    let status = if m.queued {
        wire::MessageStatus::Queued
    } else {
        match m.status {
            MessageStatus::Streaming => wire::MessageStatus::Streaming,
            MessageStatus::Complete => wire::MessageStatus::Complete,
            MessageStatus::Error => wire::MessageStatus::Failed,
            MessageStatus::Cancelled | MessageStatus::Interrupted => wire::MessageStatus::Stopped,
        }
    };
    wire::Message {
        id: m.id.clone(),
        chat_id: chat_id.to_string(),
        role: m.role,
        parts: parts(m),
        status,
        files: m.files.clone(),
        created_at: m.created_at,
        ended_at: m.finished_at,
        usage: m.usage.map(|u| wire::Usage {
            input: u.input,
            output: u.output,
            cache_read: u.cache_read,
            cache_write: u.cache_write,
            context: u.context,
            window: u.context_window,
        }),
        interrupted: m.status == MessageStatus::Interrupted,
    }
}

/// A message's parts in the web client's kinds. Messages saved before parts were kept, and the
/// person's own, have only their text, tools and approvals.
pub(super) fn parts(m: &Message) -> Vec<wire::Part> {
    let tool = |id: &str| {
        m.tools
            .iter()
            .find(|t| t.id == id)
            .map(|t| wire::Part::Tool {
                id: t.id.clone(),
                name: t.name.clone(),
                summary: t.summary.clone(),
                status: match t.status {
                    StepStatus::Pending | StepStatus::Running => ToolStatus::Running,
                    StepStatus::Done => ToolStatus::Done,
                    StepStatus::Error => ToolStatus::Failed,
                },
                output: t.output.clone().unwrap_or_default(),
            })
    };
    let approval = |id: &str| {
        m.approvals
            .iter()
            .find(|a| a.id == id)
            .map(|a| wire::Part::Approval {
                id: a.id.clone(),
                title: a.title.clone(),
                detail: a.detail.clone(),
                questions: a.questions.clone(),
                state: match a.status {
                    ApprovalStatus::Pending => ApprovalState::Pending,
                    ApprovalStatus::Allowed => ApprovalState::Allowed,
                    ApprovalStatus::Denied => ApprovalState::Denied,
                },
                answers: a
                    .answers
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<BTreeMap<_, _>>(),
            })
    };
    let mut parts = Vec::new();
    if m.parts.is_empty() {
        if !m.text.is_empty() {
            parts.push(wire::Part::Text {
                text: m.text.clone(),
            });
        }
        parts.extend(m.tools.iter().filter_map(|t| tool(&t.id)));
        parts.extend(m.approvals.iter().filter_map(|a| approval(&a.id)));
    }
    for part in &m.parts {
        match part {
            Part::Text { text } | Part::Plan { text } => {
                parts.push(wire::Part::Text { text: text.clone() });
            }
            Part::Thinking { text, .. } => {
                parts.push(wire::Part::Reasoning { text: text.clone() });
            }
            Part::Tool { id } => parts.extend(tool(id)),
            Part::Approval { id } => parts.extend(approval(id)),
            Part::Compaction { .. } => {}
        }
    }
    if !m.plan.is_empty() {
        let steps = m
            .plan
            .iter()
            .map(|s| wire::PlanStep {
                text: s.text.clone(),
                status: match s.status {
                    StepStatus::Pending | StepStatus::Error => PlanStepStatus::Pending,
                    StepStatus::Running => PlanStepStatus::InProgress,
                    StepStatus::Done => PlanStepStatus::Completed,
                },
            })
            .collect();
        parts.push(wire::Part::Plan { steps });
    }
    if let Some(error) = &m.error {
        parts.push(wire::Part::Error {
            text: error.clone(),
        });
    }
    parts
}
