//! The rules a scope holds and changing them: adding, moving and editing a rule.

// Rust guideline compliant 2026-10-09

use gpui::{Div, FontWeight, div, prelude::*, px};
use patchwork_server::permissions::{
    Effect, PermissionKind, PermissionRule, PermissionScope, PermissionSettings, ScopeRules,
};
use patchwork_server::projects::Project;
use patchwork_server::system::new_id;
use crate::settings::SettingsDialog;
use crate::ui::theme::{FS_SM, Theme};

/// The kinds of request the pane offers rules for (`PERMISSION_KINDS`): its label and an example.
pub(super) const PERMISSION_KINDS: [(PermissionKind, &str, &str); 1] =
    [(PermissionKind::Command, "Run commands", "git status")];

pub(super) const EFFECTS: [(Effect, &str); 3] = [
    (Effect::Allow, "Allow"),
    (Effect::Ask, "Ask"),
    (Effect::Deny, "Deny"),
];

/// The field a new rule's pattern is typed in.
pub(super) const ADD_FIELD: &str = "perm-add";

pub struct PermissionsState {
    pub(super) scope: PermissionScope,
    /// The kind and pattern of the rule being added.
    pub(super) kind: PermissionKind,
    pub(super) pattern: String,
    /// The rule an approval opened the pane on, shown picked out.
    pub(super) picked: Option<String>,
}

impl Default for PermissionsState {
    fn default() -> Self {
        Self {
            scope: PermissionScope::Everywhere,
            kind: PermissionKind::Command,
            pattern: String::new(),
            picked: None,
        }
    }
}

impl SettingsDialog {
    /// Opens the pane on `scope`, with `rule_id` picked out.
    pub fn focus_rule(&mut self, scope: PermissionScope, rule_id: Option<String>) {
        self.permissions.scope = scope;
        self.permissions.picked = rule_id;
    }
}

pub(super) fn effect_label(effect: Effect) -> &'static str {
    EFFECTS
        .iter()
        .find(|(e, _)| *e == effect)
        .map_or("", |(_, label)| label)
}

pub(crate) fn kind_label(kind: PermissionKind) -> &'static str {
    PERMISSION_KINDS
        .iter()
        .find(|(k, ..)| *k == kind)
        .map_or("", |(_, label, _)| label)
}

pub(crate) fn scope_name(scope: &PermissionScope, projects: &[Project]) -> String {
    match scope {
        PermissionScope::Everywhere => "Everywhere".into(),
        PermissionScope::Chats => "Chats".into(),
        PermissionScope::Project(id) => projects
            .iter()
            .find(|p| p.id == *id)
            .map_or_else(|| "Removed project".into(), |p| p.name.clone()),
    }
}

pub(super) fn scope_rules(permissions: &PermissionSettings, scope: &PermissionScope) -> ScopeRules {
    match scope {
        PermissionScope::Everywhere => permissions.everywhere.clone(),
        PermissionScope::Chats => permissions.chats.clone(),
        PermissionScope::Project(id) => permissions.projects.get(id).cloned().unwrap_or_default(),
    }
}

pub(super) fn scope_rules_mut<'a>(
    permissions: &'a mut PermissionSettings,
    scope: &PermissionScope,
) -> &'a mut ScopeRules {
    match scope {
        PermissionScope::Everywhere => &mut permissions.everywhere,
        PermissionScope::Chats => &mut permissions.chats,
        PermissionScope::Project(id) => permissions.projects.entry(id.clone()).or_default(),
    }
}

/// `rules` with the starting rules put back as they first were: deleted ones added again and
/// changed ones reset. The user's own rules stay, and a starting rule is not added again where one
/// of them already covers the same pattern.
pub(super) fn with_default_rules(
    rules: &[PermissionRule],
    defaults: &[PermissionRule],
) -> Vec<PermissionRule> {
    let own: Vec<_> = rules
        .iter()
        .filter(|r| !defaults.iter().any(|d| d.id == r.id))
        .cloned()
        .collect();
    let restored = defaults
        .iter()
        .filter(|d| {
            !own.iter()
                .any(|r| r.kind == d.kind && r.pattern.trim() == d.pattern)
        })
        .cloned();
    restored.chain(own.iter().cloned()).collect()
}

/// Whether restoring the starting rules would change `rules`.
pub(super) fn defaults_changed(rules: &[PermissionRule], defaults: &[PermissionRule]) -> bool {
    let restored = with_default_rules(rules, defaults);
    restored.len() != rules.len() || restored.iter().any(|r| !rules.contains(r))
}

/// The pane's headings (`HEADING`).
pub(super) fn heading(text: &'static str, t: &Theme) -> Div {
    div()
        .mt(px(16.))
        .mb(px(2.))
        .text_size(px(FS_SM))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.text_muted)
        .child(text)
}

impl SettingsDialog {
    /// Saves the rules of `scope` as `change` leaves them.
    pub(super) fn change_scope(
        &mut self,
        scope: &PermissionScope,
        change: impl FnOnce(&mut ScopeRules),
        cx: &mut Context<Self>,
    ) {
        self.update(|s| change(scope_rules_mut(&mut s.permissions, scope)), cx);
    }

    pub(super) fn change_rule(
        &mut self,
        scope: &PermissionScope,
        id: &str,
        change: impl FnOnce(&mut PermissionRule),
        cx: &mut Context<Self>,
    ) {
        self.change_scope(
            scope,
            |s| {
                if let Some(rule) = s.rules.iter_mut().find(|r| r.id == id) {
                    change(rule);
                }
            },
            cx,
        );
    }

    pub(super) fn move_rule(
        &mut self,
        from: &PermissionScope,
        to: &PermissionScope,
        id: &str,
        cx: &mut Context<Self>,
    ) {
        self.update(
            |s| {
                let rules = &mut scope_rules_mut(&mut s.permissions, from).rules;
                let Some(at) = rules.iter().position(|r| r.id == id) else {
                    return;
                };
                let rule = rules.remove(at);
                scope_rules_mut(&mut s.permissions, to).rules.push(rule);
            },
            cx,
        );
    }

    /// Adds an allow rule for the pattern typed, then empties the field.
    pub(super) fn add_rule(&mut self, cx: &mut Context<Self>) {
        let pattern = self.permissions.pattern.trim().to_string();
        if pattern.is_empty() {
            return;
        }
        let (scope, kind) = (self.permissions.scope.clone(), self.permissions.kind);
        self.change_scope(
            &scope,
            |s| {
                s.rules.push(PermissionRule {
                    id: new_id(),
                    kind,
                    pattern,
                    effect: Effect::Allow,
                });
            },
            cx,
        );
        self.permissions.pattern.clear();
        if let Some(area) = self.field_area(ADD_FIELD) {
            area.update(cx, |a, cx| a.set_text("", cx));
        }
    }
}
