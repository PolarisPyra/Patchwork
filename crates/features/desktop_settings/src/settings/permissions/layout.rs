//! The Permissions pane.

// Rust guideline compliant 2026-10-09

use super::rows::add_rule_row;
use super::rows::rule_row;
use super::rules::EFFECTS;
use super::rules::effect_label;
use super::rules::PERMISSION_KINDS;
use super::rules::with_default_rules;
use super::rules::scope_name;
use super::rules::defaults_changed;
use super::rules::scope_rules;
use gpui::{AnyElement, FontWeight, Window, div, prelude::*, px};
use patchwork_client::local::core::permissions::{
    Effect, PermissionKind, PermissionScope, default_rules,
};
use crate::settings::SettingsDialog;
use crate::settings::controls::{Section, choices, segmented, select_box, switch, token_row};
use patchwork_design_theme::theme;
use patchwork_design_components::{BtnKind, btn, tooltip};
use patchwork_design_icons::{IconName, icon};

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
    let scope_chip = select_box(
        scope_menu,
        format!("Rules for {name}"),
        name,
        d.menu_open(scope_menu),
        &t,
    )
    .on_click(choices(
        scope_menu,
        "Rule scope",
        scopes
            .iter()
            .map(|s| {
                let count = scope_rules(&permissions, s).rules.len();
                let name = scope_name(s, &projects);
                let label = if count > 0 {
                    format!("{name} · {count}")
                } else {
                    name
                };
                (s.clone(), label.into())
            })
            .collect(),
        d.permissions.scope.clone(),
        |d, pick, cx| {
            d.permissions.scope = pick;
            cx.notify();
        },
        cx,
    ));
    let header = div()
        .flex()
        .items_center()
        .gap(px(8.))
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
                .px(px(10.))
                .gap(px(6.))
                .child(icon(IconName::ArrowCounterClockwise, 14.))
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
        token_row(label, &t).child(segmented(
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
                .px(px(16.))
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
    let computer_use = token_row("Let the agent use the screen", &t).child(
        switch("perm-computer-use", "Computer use", screen_on, &t).on_click(cx.listener(
            move |d, _, _, cx| {
                d.update(
                    |s| {
                        s.computer_use = !screen_on;
                        // Using the screen needs seeing it, so seeing follows this switch both
                        // ways; it can still be turned on or off by itself.
                        s.see_screen = !screen_on;
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

    // Apart from computer use: seeing the screen lets no agent click or type.
    let seeing = d.settings.see_screen;
    let see_screen = token_row("Let the agent see the screen", &t).child(
        switch("perm-see-screen", "See the screen", seeing, &t)
            .on_click(cx.listener(move |d, _, _, cx| d.update(|s| s.see_screen = !seeing, cx))),
    );

    let android_on = d.settings.android_use;
    let android_use = token_row("Let agents use Android devices", &t).child(
        switch("perm-android-use", "Android devices", android_on, &t).on_click(
            cx.listener(move |d, _, _, cx| d.update(|s| s.android_use = !android_on, cx)),
        ),
    );

    div()
        .flex()
        .flex_col()
        .child(
            Section::new("Defaults")
                .action(header)
                .children(default_rows),
        )
        .child(Section::new("Rules").children(rule_groups).child(add))
        .child(
            Section::new("Computer use")
                .child(computer_use)
                .child(see_screen)
                .child(android_use),
        )
        .into_any_element()
}
