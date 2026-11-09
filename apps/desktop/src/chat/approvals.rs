//! A reply's approvals and questions (`transcript/components/Approvals.tsx`), and the permission
//! request the agent is waiting on, above the composer (`prompts/PendingApprovalBanner.tsx`).

use std::collections::HashMap;

use gpui::{
    Anchor, AnyElement, Context, Div, FontWeight, MouseButton, Role, SharedString, anchored,
    deferred, div, point, prelude::*, px,
};
use patchwork_server::permissions::{Effect, PermissionKind, PermissionRule, PermissionScope, RuleRef};
use patchwork_server::system::new_id;
use patchwork_server::threads::{Approval, ApprovalStatus, Message};

use crate::chat::secret::secret_card;
use crate::settings::permissions::{kind_label, scope_name};
use crate::ui::theme::{FONT_MONO, FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme, theme};
use crate::ui::{button, icon, popup};
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
        .gap(px(6.))
        .pb(px(8.))
        .children(groups.into_iter().map(|(a, count)| {
            if a.secret.is_some() {
                secret_card(w, a, t, cx)
            } else {
                approval(w, a, count, t, cx).into_any_element()
            }
        }))
}

fn approval(
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
            .text_size(px(FS_SM))
            .text_color(t.warning)
            .child(icon("shield-warning", 15.))
            .child(text);
    }
    let card = || {
        div()
            .flex()
            .flex_col()
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .px(px(12.))
            .py(px(10.))
            .text_size(px(FS_SM))
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
        icon("check-circle", 15.).text_color(t.success)
    } else {
        icon("x-circle", 15.).text_color(t.danger)
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
            .text_size(px(FS_SM))
            .text_color(t.text_muted)
            .child(mark)
            .child(div().flex_none().text_color(t.text).child(a.title.clone()))
            .when(!a.detail.is_empty(), |row| {
                row.child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(FONT_MONO)
                        .text_size(px(FS_XS))
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
fn rule_menu(
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
            .flex()
            .items_center()
            .rounded(px(RADIUS_SM))
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
                .text_size(px(FS_XS))
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
                this.save_rules(&scope, |rules| rules.retain(|r| r.id != rule_id), cx);
            }))
        });
        let saves = saveable.then(|| {
            let (local, local_name) = w.thread_scope();
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
                    this.save_rules(
                        &scope,
                        |to| {
                            to.retain(|r| {
                                !rules
                                    .iter()
                                    .any(|n| n.kind == r.kind && n.pattern == r.pattern)
                            });
                            to.extend(rules.iter().cloned());
                        },
                        cx,
                    );
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
                            .text_size(px(FS_SM))
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.rule_menu = None;
                                cx.notify();
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
            .hover(|s| s.underline())
            .child(label)
            .child(icon("caret-down", 11.))
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
fn suggested_rules(a: &Approval) -> Vec<PermissionRule> {
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
fn code_block(text: &str, t: &Theme) -> Div {
    div()
        .rounded(px(RADIUS_SM))
        .bg(t.bg_editor)
        .px(px(8.))
        .py(px(6.))
        .font_family(FONT_MONO)
        .text_color(t.text)
        .child(text.to_string())
}

/// How a pattern reads in the menu: `*` is anything, `.` the thread's own folder.
fn shown_pattern(pattern: &str) -> &str {
    match pattern {
        "*" => "Anything",
        "." => "Its own folder",
        p => p,
    }
}

impl Workspace {
    /// The permission requests the open thread's agent is waiting on, oldest first.
    fn pending_permissions(&self) -> Vec<&Approval> {
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
    fn thread_scope(&self) -> (PermissionScope, String) {
        let project = self.thread.as_ref().and_then(|th| th.project_id.clone());
        match project {
            Some(id) => {
                let name = self
                    .settings
                    .projects
                    .iter()
                    .find(|p| p.id == id)
                    .map_or("Removed project".to_string(), |p| p.name.clone());
                (PermissionScope::Project(id), name)
            }
            None => (PermissionScope::Chats, "Chats".to_string()),
        }
    }

    /// Answers a permission request; `always` also saves its suggested rules there, after the
    /// answer, so the server answers the other waiting requests they cover rather than this one.
    fn respond_permission(
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
            log::warn!("could not answer the approval: {e}");
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
    fn save_rules(
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
            log::warn!("could not save the rule: {e}");
        }
        self.settings = self.server.store.settings();
        cx.notify();
    }

    fn allow(
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

    /// The first request waiting, with Deny and Allow. Allow can also save a rule for the
    /// thread's project (or chats) or for everywhere, and offers the choice made last for the
    /// same kind of request.
    pub fn approval_banner(&self, cx: &mut Context<Self>) -> Option<gpui::Stateful<Div>> {
        let pending = self.pending_permissions();
        let a = (*pending.first()?).clone();
        let t = *theme(cx);
        let can_save = !a.suggestions.is_empty();
        let scope_name = self.thread_scope().1;
        let choices = [
            (AllowChoice::Once, "Allow once".to_string()),
            (AllowChoice::Local, format!("Always allow in {scope_name}")),
            (
                AllowChoice::Everywhere,
                "Always allow everywhere".to_string(),
            ),
        ];
        let saved = a
            .kind
            .and_then(|k| self.allow_choices.get(&k).copied())
            .unwrap_or(AllowChoice::Once);
        let main = if can_save { saved } else { AllowChoice::Once };
        let main_label = match main {
            AllowChoice::Once => "Allow".to_string(),
            _ => choices
                .iter()
                .find(|(c, _)| *c == main)
                .map_or(String::new(), |(_, l)| l.clone()),
        };
        let patterns = a
            .suggestions
            .iter()
            .map(|p| shown_pattern(p))
            .collect::<Vec<_>>()
            .join(", ");
        let menu_open = self.allow_menu.as_deref() == Some(a.id.as_str());
        let (deny_id, allow_id, menu_id) = (a.id.clone(), a.id.clone(), a.id.clone());
        let kind = a.kind;

        let btn = |id: &str, label: SharedString| {
            button(SharedString::from(id.to_string()), label.clone())
                .h(px(28.))
                .px(px(14.))
                .justify_center()
                .rounded(px(RADIUS_SM))
                .border_1()
                .text_size(px(crate::ui::theme::FS_MD))
                .child(label)
        };
        let (hover, accent_hover) = (t.bg_hover, t.accent_hover);
        let deny = btn("approval-deny", "Deny".into())
            .border_color(t.border_strong)
            .bg(t.bg_editor)
            .text_color(t.text)
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.respond_permission(&deny_id, false, None, cx)
            }));
        let allow = btn("approval-allow", main_label.into())
            .border_color(t.accent)
            .bg(t.accent)
            .text_color(t.text_on_accent)
            .hover(move |s| s.bg(accent_hover))
            .when(can_save, |b| b.rounded_r_none())
            .on_click(cx.listener(move |this, _, _, cx| this.allow(&allow_id, kind, main, cx)));
        let mut divider = t.text_on_accent;
        divider.a = 0.3;
        let more = can_save.then(|| {
            let menu = menu_open.then(|| {
                let rows = choices.iter().map(|(choice, label)| {
                    let (choice, approval_id) = (*choice, a.id.clone());
                    div()
                        .id(SharedString::from(format!("allow-{label}")))
                        .role(Role::MenuItem)
                        .aria_label(label.clone())
                        .min_w(px(260.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .rounded(px(RADIUS_SM))
                        .px(px(8.))
                        .py(px(5.))
                        .cursor_pointer()
                        .text_color(t.text)
                        .when(choice == main, |r| r.bg(t.bg_selected_muted))
                        .when(choice != main, |r| r.hover(move |s| s.bg(hover)))
                        .child(label.clone())
                        .when(choice != AllowChoice::Once, |r| {
                            r.child(
                                div()
                                    .max_w(px(360.))
                                    .truncate()
                                    .font_family(FONT_MONO)
                                    .text_size(px(FS_XS))
                                    .text_color(t.text_muted)
                                    .child(patterns.clone()),
                            )
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.allow(&approval_id, kind, choice, cx)
                        }))
                });
                div().absolute().top_0().right_0().child(
                    deferred(
                        anchored()
                            .anchor(Anchor::BottomRight)
                            .offset(point(px(0.), px(-4.)))
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                popup(&t)
                                    .id("allow-menu")
                                    .role(Role::Menu)
                                    .aria_label("More ways to allow")
                                    .occlude()
                                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                        this.allow_menu = None;
                                        cx.notify();
                                    }))
                                    .children(rows),
                            ),
                    )
                    .with_priority(1),
                )
            });
            div()
                .relative()
                .child(
                    button("approval-more", "More ways to allow")
                        .aria_expanded(menu_open)
                        .h(px(28.))
                        .px(px(6.))
                        .justify_center()
                        .rounded_r(px(RADIUS_SM))
                        .relative()
                        .border_1()
                        .border_color(t.accent)
                        .bg(t.accent)
                        .text_color(t.text_on_accent)
                        // The split line between Allow and its menu.
                        .child(
                            div()
                                .absolute()
                                .left(px(-1.))
                                .top_0()
                                .bottom_0()
                                .w(px(1.))
                                .bg(divider),
                        )
                        .child(icon("caret-down", 12.))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.allow_menu = match this.allow_menu {
                                Some(_) => None,
                                None => Some(menu_id.clone()),
                            };
                            cx.notify();
                        })),
                )
                .children(menu)
        });

        Some(
            div()
                .id("approval-request")
                .role(Role::Group)
                .aria_label("Approval request")
                .mb(px(6.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .rounded(px(RADIUS))
                .border_1()
                .border_color(t.warning)
                .bg(t.bg_panel)
                .p(px(10.))
                .text_size(px(FS_SM))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_color(t.warning)
                        .child(icon("shield-warning", 15.))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(t.text)
                                .child(a.title.clone()),
                        )
                        .when(pending.len() > 1, |row| {
                            row.child(
                                div()
                                    .ml_auto()
                                    .flex_none()
                                    .text_color(t.text_muted)
                                    .child(format!("1/{}", pending.len())),
                            )
                        }),
                )
                .when(!a.detail.is_empty(), |c| {
                    c.child(
                        div()
                            .id("approval-detail")
                            .max_h(px(160.))
                            .overflow_y_scroll()
                            .child(code_block(&a.detail, &t)),
                    )
                })
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(6.))
                        .child(deny)
                        .child(div().flex().child(allow).children(more)),
                ),
        )
    }
}
