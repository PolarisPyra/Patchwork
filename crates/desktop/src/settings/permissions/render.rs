//! The Permissions pane.

// Rust guideline compliant 2026-10-09

use super::state::heading;
use super::rows::add_rule_row;
use super::rows::rule_row;
use super::state::EFFECTS;
use super::state::effect_label;
use super::state::PERMISSION_KINDS;
use super::state::with_default_rules;
use super::state::scope_name;
use super::state::defaults_changed;
use super::state::scope_rules;
use gpui::{AnyElement, ClickEvent, FontWeight, Window, div, prelude::*, px};
use patchwork_server::permissions::{Effect, PermissionKind, PermissionScope, default_rules};
use crate::settings::SettingsDialog;
use crate::settings::controls::{choice, segmented, switch, token_row};
use crate::ui::theme::theme;
use crate::ui::{BtnKind, btn, dropdown_chip, icon, tooltip};

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let scope = d.permissions.scope.clone();
    let permissions = d.settings.permissions.clone();
    let projects = d.settings.projects.clone();
    let own = scope_rules(&permissions, &scope);
    let everywhere = scope == PermissionScope::Everywhere;
    let defaults = default_rules();
    let restorable = everywhere && defaults_changed(&own.rules, &defaults);
    let scopes: Vec<PermissionScope> = [PermissionScope::Everywhere, PermissionScope::Chats]
        .into_iter()
        .chain(
            projects
                .iter()
                .map(|p| PermissionScope::Project(p.id.clone())),
        )
        .collect();

    let name = scope_name(&scope, &projects);
    let scope_menu = "perm-scope";
    let scope_chip = dropdown_chip(
        scope_menu,
        format!("Rules for {name}"),
        name,
        d.menu_open(scope_menu),
        &t,
    )
    .on_click(cx.listener({
        let (scopes, permissions, projects) =
            (scopes.clone(), permissions.clone(), projects.clone());
        move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let items = scopes
                .iter()
                .map(|s| {
                    let count = scope_rules(&permissions, s).rules.len();
                    let name = scope_name(s, &projects);
                    let label = if count > 0 {
                        format!("{name} · {count}")
                    } else {
                        name
                    };
                    let pick = s.clone();
                    choice(
                        weak.clone(),
                        label,
                        *s == d.permissions.scope,
                        move |d, cx| {
                            d.menu = None;
                            d.permissions.scope = pick.clone();
                            cx.notify();
                        },
                    )
                })
                .collect();
            d.toggle_menu(scope_menu, "Rule scope", items, e.position(), window, cx);
        }
    }));
    let header = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .border_b_1()
        .border_color(t.border)
        .pb(px(10.))
        .child(scope_chip)
        .when(everywhere, |row| {
            row.child(
                btn(
                    "perm-restore",
                    "Restore defaults",
                    BtnKind::Plain,
                    !restorable,
                    &t,
                )
                .ml_auto()
                .px(px(10.))
                .gap(px(6.))
                .child(icon("arrow-counter-clockwise", 14.))
                .child("Restore defaults")
                .map(|b| {
                    if restorable {
                        b.on_click(cx.listener(move |d, _, _, cx| {
                            let defaults = defaults.clone();
                            d.change_scope(
                                &PermissionScope::Everywhere,
                                |s| s.rules = with_default_rules(&s.rules, &defaults),
                                cx,
                            );
                        }))
                    } else {
                        b.tooltip(tooltip("The starting rules are all in place"))
                    }
                }),
            )
        });

    let default_rows = PERMISSION_KINDS.iter().map(|&(kind, label, _)| {
        let inherited = permissions
            .everywhere
            .defaults
            .get(&kind)
            .copied()
            .unwrap_or(Effect::Ask);
        let value = own
            .defaults
            .get(&kind)
            .copied()
            .or(everywhere.then_some(Effect::Ask));
        let mut options: Vec<(Option<Effect>, String)> = Vec::new();
        if !everywhere {
            options.push((None, format!("Inherit · {}", effect_label(inherited))));
        }
        options.extend(EFFECTS.iter().map(|&(e, l)| (Some(e), l.to_string())));
        let scope = scope.clone();
        token_row(label, None, &t).child(segmented(
            &format!("perm-default-{label}"),
            label,
            value,
            &options,
            &t,
            move |d, next, cx| {
                d.change_scope(
                    &scope,
                    |s| match next {
                        Some(effect) => {
                            s.defaults.insert(kind, effect);
                        }
                        None => {
                            s.defaults.remove(&kind);
                        }
                    },
                    cx,
                );
            },
            cx,
        ))
    });
    let default_rows: Vec<_> = default_rows.collect();

    let targets: Vec<(PermissionScope, String)> = scopes
        .iter()
        .filter(|s| **s != scope)
        .map(|s| (s.clone(), scope_name(s, &projects)))
        .collect();
    let mut rule_groups = Vec::new();
    for &(kind, label, placeholder) in &PERMISSION_KINDS {
        let rules: Vec<_> = own.rules.iter().filter(|r| r.kind == kind).collect();
        if rules.is_empty() {
            continue;
        }
        let rows: Vec<_> = rules
            .into_iter()
            .map(|rule| rule_row(d, rule, placeholder, &targets, window, cx))
            .collect();
        rule_groups.push(
            div()
                .border_b_1()
                .border_color(t.border)
                .py(px(8.))
                .child(
                    div()
                        .mb(px(4.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(label),
                )
                .children(rows),
        );
    }
    let add = add_rule_row(d, window, cx);

    let screen_on = d.settings.computer_use
        && permissions.everywhere.defaults.get(&PermissionKind::Screen) == Some(&Effect::Allow);
    let computer_use = token_row(
        "Let the agent use the screen",
        Some("Adds screenshot, click, type, key and scroll tools to Patchwork's own agent."),
        &t,
    )
    .child(
        switch("perm-computer-use", "Computer use", screen_on, &t).on_click(cx.listener(
            move |d, _, _, cx| {
                d.update(
                    |s| {
                        s.computer_use = !screen_on;
                        let defaults = &mut s.permissions.everywhere.defaults;
                        if screen_on {
                            defaults.remove(&PermissionKind::Screen);
                        } else {
                            defaults.insert(PermissionKind::Screen, Effect::Allow);
                        }
                    },
                    cx,
                );
            },
        )),
    );

    div()
        .flex()
        .flex_col()
        .child(header)
        .child(heading("Defaults", &t))
        .children(default_rows)
        .child(heading("Rules", &t))
        .children(rule_groups)
        .child(add)
        .child(heading("Computer use", &t))
        .child(computer_use)
        .into_any_element()
}
