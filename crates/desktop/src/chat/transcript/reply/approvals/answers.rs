//! Answering a request: the requests waiting, the scope a rule is saved in, and sending the answer.

// Rust guideline compliant 2026-10-09

use super::card::AllowChoice;
use super::card::suggested_rules;
use std::collections::HashMap;
use std::sync::Arc;
use gpui::prelude::*;
use patchwork_core::permissions::{PermissionKind, PermissionScope, RuleRef};
use patchwork_core::Core;
use patchwork_core::chats::{Approval, ApprovalStatus};
use crate::workspace::Workspace;

impl Workspace {
    /// The permission requests the open chat's agent is waiting on, oldest first.
    pub(in crate::chat) fn pending_permissions(&self) -> Vec<&Approval> {
        self.chat.as_ref().map_or_else(Vec::new, |th| {
            th.messages
                .iter()
                .flat_map(|m| &m.approvals)
                .filter(|a| {
                    a.status == ApprovalStatus::Pending
                        && a.secret.is_none()
                        && a.questions.is_empty()
                })
                .collect()
        })
    }

    /// The open chat's own scope, and its name: its project's, or "Chats".
    pub(in crate::chat) fn chat_scope(&self) -> (PermissionScope, String) {
        let project = self.chat.as_ref().and_then(|th| th.project_id.clone());
        match project {
            Some(id) => {
                let name = self
                    .settings
                    .projects
                    .iter()
                    .find(|p| p.id == id)
                    .map_or_else(|| "Removed project".to_string(), |p| p.name.clone());
                (PermissionScope::Project(id), name)
            }
            None => (PermissionScope::Chats, "Chats".to_string()),
        }
    }

    /// Answers a permission request; `always` also saves its suggested rules there, after the
    /// answer, so the core answers the other waiting requests they cover rather than this one.
    pub(in crate::chat) fn respond_permission(
        &mut self,
        approval_id: &str,
        allow: bool,
        always: Option<PermissionScope>,
        cx: &mut Context<Self>,
    ) {
        let Some(chat_id) = self.chat.as_ref().map(|th| th.id.clone()) else {
            return;
        };
        let Some(approval) = self
            .pending_permissions()
            .into_iter()
            .find(|a| a.id == approval_id)
            .cloned()
        else {
            return;
        };
        let rules = match always {
            Some(_) => suggested_rules(&approval),
            None => Vec::new(),
        };
        let rule = always.clone().zip(rules.first()).map(|(scope, r)| RuleRef {
            scope,
            rule_id: Some(r.id.clone()),
        });
        self.allow_menu = None;
        if let Err(e) =
            self.core
                .respond_approval(&chat_id, &approval.id, allow, HashMap::new(), rule)
        {
            log::warn!(error:% = e; "could not answer the approval: {{error}}");
            return;
        }
        if let Some(scope) = always.filter(|_| !rules.is_empty()) {
            self.save_rules(|core| core.save_permission_rules(&scope, rules), cx);
        }
        cx.notify();
    }

    /// Saves a change to the permission rules, and shows the rules as saved.
    pub(in crate::chat) fn save_rules(
        &mut self,
        change: impl FnOnce(&Arc<Core>) -> Result<(), patchwork_core::Error>,
        cx: &mut Context<Self>,
    ) {
        if let Err(e) = change(&self.core) {
            log::warn!(error:% = e; "could not save the rule: {{error}}");
        }
        self.settings = self.core.settings();
        cx.notify();
    }

    pub(in crate::chat) fn allow(
        &mut self,
        approval_id: &str,
        kind: Option<PermissionKind>,
        choice: AllowChoice,
        cx: &mut Context<Self>,
    ) {
        if let Some(kind) = kind {
            self.allow_choices.insert(kind, choice);
        }
        let always = match choice {
            AllowChoice::Once => None,
            AllowChoice::Local => Some(self.chat_scope().0),
            AllowChoice::Everywhere => Some(PermissionScope::Everywhere),
        };
        self.respond_permission(approval_id, true, always, cx);
    }
}
