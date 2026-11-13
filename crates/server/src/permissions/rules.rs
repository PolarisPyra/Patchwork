//! Saving and removing rules from an approval: "Always allow", and "Ask next time" on a rule
//! that answered one.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use crate::{Error, Server};

use super::types::{PermissionRule, PermissionScope, PermissionSettings, ScopeRules};

impl Server {
    /// Adds `rules` to `scope`. A rule of the same kind and pattern is replaced, so saving one
    /// twice keeps one.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be saved.
    pub fn save_permission_rules(
        self: &Arc<Self>,
        scope: &PermissionScope,
        rules: Vec<PermissionRule>,
    ) -> Result<(), Error> {
        self.change_rules(scope, |to| {
            to.retain(|r| {
                !rules
                    .iter()
                    .any(|n| n.kind == r.kind && n.pattern == r.pattern)
            });
            to.extend(rules);
        })
    }

    /// Removes the rule `rule_id` from `scope`, so its requests are asked about again.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be saved.
    pub fn remove_permission_rule(
        self: &Arc<Self>,
        scope: &PermissionScope,
        rule_id: &str,
    ) -> Result<(), Error> {
        self.change_rules(scope, |to| to.retain(|r| r.id != rule_id))
    }

    fn change_rules(
        self: &Arc<Self>,
        scope: &PermissionScope,
        change: impl FnOnce(&mut Vec<PermissionRule>),
    ) -> Result<(), Error> {
        let mut settings = self.store.settings();
        change(&mut scope_rules(&mut settings.permissions, scope).rules);
        self.save_settings(settings)
    }
}

/// The rules `scope` keeps, made empty for a project that has none yet.
fn scope_rules<'a>(p: &'a mut PermissionSettings, scope: &PermissionScope) -> &'a mut ScopeRules {
    match scope {
        PermissionScope::Everywhere => &mut p.everywhere,
        PermissionScope::Chats => &mut p.chats,
        PermissionScope::Project(id) => p.projects.entry(id.clone()).or_default(),
    }
}
