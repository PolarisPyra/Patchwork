//! The replies a report counts: those in the chats there are, and those the ledger kept as their
//! chat was deleted or the reply undone.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};

use crate::Core;
use crate::provider::AgentKind;
use crate::chats::Chat;

use super::prices::turn_model;
use super::types::ReplyCost;

/// Resolves the model a chat ran on, asking each agent's configuration once.
struct Models {
    defaults: HashMap<AgentKind, String>,
    configured: HashMap<AgentKind, Option<String>>,
}

impl Models {
    fn of(&mut self, chat: &Chat) -> Option<String> {
        if let Some(model) = &chat.model {
            return Some(model.clone());
        }
        if let Some(model) = self.defaults.get(&chat.agent) {
            return Some(model.clone());
        }
        self.configured
            .entry(chat.agent)
            .or_insert_with(|| turn_model(chat.agent, None))
            .clone()
    }
}

impl Core {
    fn models(&self) -> Models {
        Models {
            defaults: self.store.settings().tokens.default_models,
            configured: HashMap::new(),
        }
    }

    fn project_names(&self) -> HashMap<String, String> {
        self.store
            .settings()
            .projects
            .into_iter()
            .map(|p| (p.id, p.name))
            .collect()
    }

    /// Every reply with token counts, live or kept, and the ids of the chats there still are.
    pub(super) fn counted_replies(&self) -> (Vec<ReplyCost>, HashSet<String>) {
        let names = self.project_names();
        let mut models = self.models();
        let (mut replies, open) = self.store.with_chats(|chats| {
            let open: HashSet<String> = chats.iter().map(|t| t.id.clone()).collect();
            let replies: Vec<ReplyCost> = chats
                .iter()
                .flat_map(|t| replies_of(t, 0, &names, &mut models))
                .collect();
            (replies, open)
        });
        let live: HashSet<&str> = replies.iter().map(|r| r.message_id.as_str()).collect();
        let kept: Vec<ReplyCost> = self
            .store
            .reply_costs()
            .into_iter()
            .filter(|r| !live.contains(r.message_id.as_str()))
            .collect();
        replies.extend(kept);
        (replies, open)
    }

    /// Keeps the costs of a chat's replies from the `from`th message on, before they are
    /// deleted, so the totals still count them.
    pub(crate) fn keep_reply_costs(&self, chat_id: &str, from: usize) {
        let Some(chat) = self.store.chat(chat_id) else {
            return;
        };
        let rows = replies_of(&chat, from, &self.project_names(), &mut self.models());
        if rows.is_empty() {
            return;
        }
        if let Err(e) = self.store.save_reply_costs(&rows) {
            log::warn!(error:% = e; "could not keep the costs of deleted replies: {{error}}");
        }
    }
}

fn replies_of(
    chat: &Chat,
    from: usize,
    names: &HashMap<String, String>,
    models: &mut Models,
) -> Vec<ReplyCost> {
    let with_usage = chat
        .messages
        .iter()
        .skip(from)
        .filter(|m| m.usage.is_some());
    if with_usage.clone().next().is_none() {
        return Vec::new();
    }
    let model = models.of(chat);
    let project = chat
        .project_id
        .as_ref()
        .and_then(|id| names.get(id))
        .cloned();
    with_usage
        .filter_map(|m| {
            Some(ReplyCost {
                message_id: m.id.clone(),
                chat_id: chat.id.clone(),
                title: chat.title.clone(),
                project: project.clone(),
                agent: chat.agent,
                model: model.clone(),
                at: m.finished_at.unwrap_or(m.created_at),
                usage: m.usage?,
            })
        })
        .collect()
}
