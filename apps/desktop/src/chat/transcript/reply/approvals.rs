//! A reply's approvals and questions. The banner for the request the agent waits on is
//! `above_composer/approval_banner.rs`.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    Anchor, AnyElement, Div, FontWeight, MouseButton, Role, SharedString, anchored, deferred, div,
    point, prelude::*, px,
};
use patchwork_backend::permissions::{Effect, PermissionRule, PermissionScope, PermissionKind, RuleRef};
use patchwork_platform::new_id;
use patchwork_backend::chats::{Approval, ApprovalStatus, Message};
use patchwork_desktop_settings::settings::{kind_label, scope_name};
use patchwork_ui_theme::{IconName, font_mono, fs_sm, fs_xs, radius, radius_sm, Theme};
use patchwork_ui_components::{button, popup};
use patchwork_ui_theme::icon;
use patchwork_backend::Core;

use crate::chat::above_composer::secret_request::secret_card;
use crate::workspace::Workspace;

/// How the last request of a kind was allowed, which the Allow button offers first.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AllowChoice {
    Once,
    Local,
    Everywhere,
}

/// A reply's approvals in order, each run of the same resolved request answered the same way
/// folded into its first one and counted. A pending one is answered in the banner, and a secret in
/// its own card here.
pub fn approvals(
    w: &Workspace,
    m: &Message,
    ids: &[&str],
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    let mut groups: Vec<(&Approval, usize)> = Vec::new();
    for a in ids
        .iter()
        .filter_map(|id| m.approvals.iter().find(|a| a.id == *id))
    {
        let same = groups.last().is_some_and(|(last, _)| {
            a.status != ApprovalStatus::Pending
                && a.questions.is_empty()
                && a.secret.is_none()
                && last.status == a.status
                && last.title == a.title
                && last.detail == a.detail
                && last.auto == a.auto
        });
        match groups.last_mut() {
            Some((_, count)) if same => *count += 1,
            _ => groups.push((a, 1)),
        }
    }
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .pb(px(8.))
        .children(groups.into_iter().map(|(a, count)| {
            if a.secret.is_some() {
                secret_card(w, a, t, cx)
            } else {
                approval(w, a, count, t, cx).into_any_element()
            }
        }))
}

pub(in crate::chat) fn approval(
    w: &Workspace,
    a: &Approval,
    count: usize,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    if a.status == ApprovalStatus::Pending {
        let text = if a.questions.is_empty() && a.secret.is_none() {
            format!("Waiting for approval: {}", a.title)
        } else {
            "Waiting for your answer".to_string()
        };
        return div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(4.))
            .text_size(px(fs_sm()))
            .text_color(t.warning)
            .child(icon(IconName::ShieldWarning, 15.))
            .child(text);
    }
    let card = || {
        div()
            .flex()
            .flex_col()
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .px(px(12.))
            .py(px(8.))
            .text_size(px(fs_sm()))
    };
    if !a.questions.is_empty() {
        return card().gap(px(6.)).children(a.questions.iter().map(|q| {
            let answer = if a.status == ApprovalStatus::Denied {
                "Not answered".to_string()
            } else {
                a.answers
                    .get(&q.id)
                    .map(|v| v.join(", "))
                    .unwrap_or_default()
            };
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.text)
                        .child(q.question.clone()),
                )
                .child(div().text_color(t.text_muted).child(answer))
        }));
    }
    let allowed = a.status == ApprovalStatus::Allowed;
    let mark = if allowed {
        icon(IconName::CheckCircle, 15.).text_color(t.success)
    } else {
        icon(IconName::XCircle, 15.).text_color(t.danger)
    };
    let outcome = match (a.auto, allowed, a.rule.is_some()) {
        (true, true, _) => "Auto-allowed",
        (true, false, _) => "Auto-denied",
        (false, false, _) => "Denied",
        (false, true, true) => "Always allowed",
        (false, true, false) => "Allowed",
    };
    let label = if count > 1 {
        format!("{outcome} ×{count}")
    } else {
        outcome.to_string()
    };
    let label = rule_menu(w, a, label, t, cx);
    if a.auto {
        return div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .px(px(4.))
            .text_size(px(fs_sm()))
            .text_color(t.text_muted)
            .child(mark)
            .child(div().flex_none().text_color(t.text).child(a.title.clone()))
            .when(!a.detail.is_empty(), |row| {
                row.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(font_mono())
                        .text_size(px(fs_xs()))
                        .child(a.detail.clone()),
                )
            })
            .child(div().ml_auto().flex_none().child(label));
    }
    card()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(mark)
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.text)
                        .child(a.title.clone()),
                )
                .child(div().ml_auto().text_color(t.text_muted).child(label)),
        )
        .when(!a.detail.is_empty(), |c| c.child(code_block(&a.detail, t)))
}

/// An approval's outcome, which opens what can be done about the rule behind it: change or remove
/// the rule that answered it, or save one so the next request like it is not asked.
pub(in crate::chat) fn rule_menu(
    w: &Workspace,
    a: &Approval,
    label: String,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let allowed = a.status == ApprovalStatus::Allowed;
    let saveable = allowed && a.rule.is_none() && a.kind.is_some() && !a.suggestions.is_empty();
    if a.rule.is_none() && !saveable {
        return label.into_any_element();
    }
    let open = w.rule_menu.as_deref() == Some(a.id.as_str());
    let permissions = &w.settings.permissions;
    let found = a.rule.as_ref().and_then(|r| {
        let rules = match &r.scope {
            PermissionScope::Everywhere => &permissions.everywhere,
            PermissionScope::Chats => &permissions.chats,
            PermissionScope::Project(id) => permissions.projects.get(id)?,
        };
        let id = r.rule_id.as_deref()?;
        rules.rules.iter().find(|rule| rule.id == id).cloned()
    });
    let where_ = a
        .rule
        .as_ref()
        .map(|r| scope_name(&r.scope, &w.settings.projects))
        .unwrap_or_default();
    let hover = t.bg_hover;
    let item = |id: &str, label: String| {
        div()
            .id(SharedString::from(format!("rule-{id}")))
            .role(Role::MenuItem)
            .aria_label(label.clone())
            .tab_index(0)
            .focus_visible(patchwork_ui_components::focus_ring)
            .flex()
            .items_center()
            .rounded(px(radius_sm()))
            .px(px(8.))
            .py(px(5.))
            .whitespace_nowrap()
            .cursor_pointer()
            .text_color(t.text)
            .hover(move |s| s.bg(hover))
            .child(label)
    };
    let menu = open.then(|| {
        let info = a.rule.as_ref().map(|r| {
            let text = match (&r.rule_id, &found) {
                (Some(_), Some(rule)) => format!(
                    "{}: {} · {where_}",
                    kind_label(rule.kind),
                    shown_pattern(&rule.pattern)
                ),
                (Some(_), None) => format!("The rule was removed · {where_}"),
                (None, _) => format!(
                    "Default for {} · {where_}",
                    a.kind.map_or("this kind", kind_label)
                ),
            };
            div()
                .max_w(px(360.))
                .truncate()
                .px(px(8.))
                .py(px(4.))
                .text_size(px(fs_xs()))
                .text_color(t.text_muted)
                .child(text)
        });
        let change = a.rule.as_ref().map(|r| {
            let (scope, rule_id) = (r.scope.clone(), found.as_ref().map(|f| f.id.clone()));
            item("change", "Change in Settings…".into()).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.rule_menu = None;
                    this.open_rule(scope.clone(), rule_id.clone(), window, cx);
                },
            ))
        });
        let ask = a.rule.as_ref().zip(found.as_ref()).map(|(r, rule)| {
            let (scope, rule_id) = (r.scope.clone(), rule.id.clone());
            item("ask", "Ask next time".into()).on_click(cx.listener(move |this, _, _, cx| {
                this.rule_menu = None;
                this.save_rules(|s| s.remove_permission_rule(&scope, &rule_id), cx);
            }))
        });
        let saves = saveable.then(|| {
            let (local, local_name) = w.chat_scope();
            [
                (local, format!("Always allow in {local_name}")),
                (
                    PermissionScope::Everywhere,
                    "Always allow everywhere".to_string(),
                ),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (scope, label))| {
                let approval = a.clone();
                item(&format!("save-{i}"), label).on_click(cx.listener(move |this, _, _, cx| {
                    this.rule_menu = None;
                    let rules = suggested_rules(&approval);
                    this.save_rules(|s| s.save_permission_rules(&scope, rules), cx);
                }))
            })
            .collect::<Vec<_>>()
        });
        div().absolute().top_0().right_0().child(
            deferred(
                anchored()
                    .anchor(Anchor::BottomRight)
                    .offset(point(px(0.), px(-4.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        popup(t)
                            .id("rule-menu")
                            .role(Role::Menu)
                            .aria_label("Approval rule")
                            .occlude()
                            .text_size(px(fs_sm()))
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.rule_menu = None;
                                cx.notify();
                            }))
                            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| {
                                if e.keystroke.key == "escape" {
                                    this.rule_menu = None;
                                    cx.notify();
                                }
                            }))
                            .children(info)
                            .children(change)
                            .children(ask)
                            .children(saves.into_iter().flatten()),
                    ),
            )
            .with_priority(1),
        )
    });
    let id = a.id.clone();
    div()
        .relative()
        .flex()
        .child(
            button(
                SharedString::from(format!("rule-menu-{}", a.id)),
                label.clone(),
            )
            .aria_expanded(open)
            .gap(px(2.))
            .hover(gpui::Styled::underline)
            .child(label)
            .child(icon(IconName::CaretDown, 11.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.rule_menu = match this.rule_menu {
                    Some(_) => None,
                    None => Some(id.clone()),
                };
                cx.notify();
            })),
        )
        .children(menu)
        .into_any_element()
}

/// The rules "Always allow" saves for an approval, one allow rule per suggested pattern.
pub(in crate::chat) fn suggested_rules(a: &Approval) -> Vec<PermissionRule> {
    let Some(kind) = a.kind else {
        return Vec::new();
    };
    a.suggestions
        .iter()
        .map(|pattern| PermissionRule {
            id: new_id(),
            kind,
            pattern: pattern.clone(),
            effect: Effect::Allow,
        })
        .collect()
}

/// The command or paths being approved, in a sunken mono box.
pub(in crate::chat) fn code_block(text: &str, t: &Theme) -> Div {
    div()
        .rounded(px(radius_sm()))
        .bg(t.bg_editor)
        .px(px(8.))
        .py(px(6.))
        .font_family(font_mono())
        .text_color(t.text)
        .child(text.to_string())
}

/// How a pattern reads in the menu: `*` is anything, `.` the chat's own folder.
pub(in crate::chat) fn shown_pattern(pattern: &str) -> &str {
    match pattern {
        "*" => "Anything",
        "." => "Its own folder",
        p => p,
    }
}

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
                .chats()
                .answer(&chat_id, &approval.id, allow, HashMap::new(), rule)
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
        change: impl FnOnce(&Arc<Core>) -> Result<(), patchwork_backend::Error>,
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
