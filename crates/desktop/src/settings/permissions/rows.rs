//! A rule drawn as a row, and the row that adds one.

// Rust guideline compliant 2026-10-09

use super::state::ADD_FIELD;
use super::state::PERMISSION_KINDS;
use super::state::EFFECTS;
use super::state::kind_label;
use gpui::{ClickEvent, Div, SharedString, Window, div, prelude::*, px};
use patchwork_server::permissions::{PermissionRule, PermissionScope};
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, choice, segmented, select_box};
use crate::ui::theme::{FONT_MONO, RADIUS_SM, theme};
use crate::ui::{BtnKind, btn, icon_button, tooltip};

pub(super) fn rule_row(
    d: &mut SettingsDialog,
    rule: &PermissionRule,
    placeholder: &str,
    targets: &[(PermissionScope, String)],
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Div {
    let t = *theme(cx);
    let scope = d.permissions.scope.clone();
    let id = rule.id.clone();
    let picked = d.permissions.picked.as_deref() == Some(id.as_str());
    let field = d.field(
        format!("perm-rule-{id}"),
        format!("{} rule", kind_label(rule.kind)),
        &rule.pattern,
        placeholder,
        FieldKind {
            mono: true,
            ..Default::default()
        },
        {
            let (scope, id) = (scope.clone(), id.clone());
            move |d, text, cx| {
                let pattern = text.trim().to_string();
                if !pattern.is_empty() {
                    d.change_rule(&scope, &id, |r| r.pattern = pattern, cx);
                }
            }
        },
        window,
        cx,
    );
    let effect = segmented(
        &format!("perm-effect-{id}"),
        format!("What {} gets", rule.pattern),
        rule.effect,
        &EFFECTS,
        &t,
        {
            let (scope, id) = (scope.clone(), id.clone());
            move |d, effect, cx| d.change_rule(&scope, &id, |r| r.effect = effect, cx)
        },
        cx,
    );
    let menu_key = SharedString::from(format!("perm-move-{id}"));
    let open = d.menu_open(&menu_key);
    let targets = targets.to_vec();
    let move_button = icon_button(
        menu_key.clone(),
        format!("Move {}", rule.pattern),
        "dots-three",
        14.,
        cx,
    )
    .aria_expanded(open)
    .text_color(t.text_muted)
    .tooltip(tooltip("Move to…"))
    .on_click(cx.listener({
        let (scope, id) = (scope.clone(), id.clone());
        move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let items = targets
                .iter()
                .map(|(to, name)| {
                    let (from, to, id) = (scope.clone(), to.clone(), id.clone());
                    choice(
                        weak.clone(),
                        format!("Move to {name}"),
                        false,
                        move |d, cx| {
                            d.menu = None;
                            d.move_rule(&from, &to, &id, cx);
                        },
                    )
                })
                .collect();
            d.toggle_menu(
                menu_key.clone(),
                "Move rule",
                items,
                e.position(),
                window,
                cx,
            );
        }
    }));
    let delete = icon_button(
        SharedString::from(format!("perm-delete-{id}")),
        format!("Delete {}", rule.pattern),
        "x",
        14.,
        cx,
    )
    .on_click(cx.listener(move |d, _, _, cx| {
        d.change_scope(&scope, |s| s.rules.retain(|r| r.id != id), cx);
    }));
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .mx(px(-6.))
        .rounded(px(RADIUS_SM))
        .px(px(6.))
        .py(px(2.))
        .when(picked, |row| row.bg(t.bg_selected_muted))
        .child(field.flex_1())
        .child(effect)
        .child(move_button)
        .child(delete)
}

/// A row that adds an allow rule (`AddRule`).
pub(super) fn add_rule_row(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> Div {
    let t = *theme(cx);
    let kind = d.permissions.kind;
    let placeholder = PERMISSION_KINDS
        .iter()
        .find(|(k, ..)| *k == kind)
        .map_or("", |(.., p)| p);
    let pattern = d.permissions.pattern.clone();
    let field = d.field(
        ADD_FIELD,
        "Pattern",
        &pattern,
        placeholder,
        FieldKind {
            live: true,
            mono: true,
            ..Default::default()
        },
        |d, text, cx| {
            d.permissions.pattern = text;
            cx.notify();
        },
        window,
        cx,
    );
    d.on_submit(ADD_FIELD, |d, _, cx| d.add_rule(cx));
    let key = "perm-kind";
    let open = d.menu_open(key);
    let select = select_box(key, "Kind of rule", kind_label(kind), open, &t).on_click(cx.listener(
        move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let items = PERMISSION_KINDS
                .iter()
                .map(|&(k, label, _)| {
                    choice(
                        weak.clone(),
                        label,
                        k == d.permissions.kind,
                        move |d, cx| {
                            d.menu = None;
                            d.permissions.kind = k;
                            cx.notify();
                        },
                    )
                })
                .collect();
            d.toggle_menu(key, "Kind of rule", items, e.position(), window, cx);
        },
    ));
    let empty = pattern.trim().is_empty();
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .py(px(10.))
        .child(select)
        .child(field.flex_1().font_family(FONT_MONO))
        .child(
            btn("perm-add-rule", "Add rule", BtnKind::Plain, empty, &t)
                .child("Add rule")
                .when(!empty, |b| {
                    b.on_click(cx.listener(|d, _, _, cx| d.add_rule(cx)))
                }),
        )
}
