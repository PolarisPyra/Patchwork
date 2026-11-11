//! Answering a request: the requests waiting, the scope a rule is saved in, and sending the answer.

// Rust guideline compliant 2026-10-09

use super::card::AllowChoice;
use super::card::suggested_rules;
use std::collections::HashMap;
use gpui::prelude::*;
use patchwork_server::permissions::{PermissionKind, PermissionRule, PermissionScope, RuleRef};
use patchwork_server::threads::{Approval, ApprovalStatus};
use crate::workspace::Workspace;

impl Workspace {
    /// The permission requests the open thread's agent is waiting on, oldest first.
    pub(super) fn pending_permissions(&self) -> Vec<&Approval> {
        self.thread.as_ref().map_or_else(Vec::new, |th| {
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

    /// The open thread's own scope, and its name: its project's, or "Chats".
    pub(super) fn thread_scope(&self) -> (PermissionScope, String) {
        let project = self.thread.as_ref().and_then(|th| th.project_id.clone());
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
    /// answer, so the server answers the other waiting requests they cover rather than this one.
    pub(super) fn respond_permission(
        &mut self,
        approval_id: &str,
        allow: bool,
        always: Option<PermissionScope>,
        cx: &mut Context<Self>,
    ) {
        let Some(thread_id) = self.thread.as_ref().map(|th| th.id.clone()) else {
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
            self.server
                .respond_approval(&thread_id, &approval.id, allow, HashMap::new(), rule)
        {
            log::warn!(error:% = e; "could not answer the approval: {{error}}");
            return;
        }
        if let Some(scope) = always.filter(|_| !rules.is_empty()) {
            // A rule with the same pattern is replaced, so saving one twice keeps one.
            self.save_rules(
                &scope,
                |to| {
                    to.retain(|r| {
                        !rules
                            .iter()
                            .any(|n| n.kind == r.kind && n.pattern == r.pattern)
                    });
                    to.extend(rules);
                },
                cx,
            );
        }
        cx.notify();
    }

    /// Changes the rules of `scope` with `change` and saves them.
    pub(super) fn save_rules(
        &mut self,
        scope: &PermissionScope,
        change: impl FnOnce(&mut Vec<PermissionRule>),
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.server.store.settings();
        let p = &mut settings.permissions;
        let to = match scope {
            PermissionScope::Everywhere => &mut p.everywhere,
            PermissionScope::Chats => &mut p.chats,
            PermissionScope::Project(id) => p.projects.entry(id.clone()).or_default(),
        };
        change(&mut to.rules);
        if let Err(e) = self.server.save_settings(settings) {
            log::warn!(error:% = e; "could not save the rule: {{error}}");
        }
        self.settings = self.server.store.settings();
        cx.notify();
    }

    pub(super) fn allow(
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
            AllowChoice::Local => Some(self.thread_scope().0),
            AllowChoice::Everywhere => Some(PermissionScope::Everywhere),
        };
        self.respond_permission(approval_id, true, always, cx);
    }
}
