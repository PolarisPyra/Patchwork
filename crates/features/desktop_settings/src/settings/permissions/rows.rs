//! A rule drawn as a row, and the row that adds one.

// Rust guideline compliant 2026-10-09

use gpui::component::button::Button;
use gpui::component::button::ButtonVariants as _;
use gpui::component::Disableable as _;
use gpui::component::Sizable as _;
use super::rules::ADD_FIELD;
use super::rules::PERMISSION_KINDS;
use super::rules::EFFECTS;
use super::rules::kind_label;
use gpui::{Div, SharedString, Window, div, prelude::*, px};
use patchwork_model::permissions::{PermissionRule, PermissionScope};
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, choices, segmented, select_box};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{font_mono, radius_sm, theme};

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
    let move_button = Button::new(menu_key.clone())
        .ghost()
        .small()
        .icon(IconName::DotsThree)
        .accessibility_label(format!("Move {}", rule.pattern))
        .aria_expanded(open)
        .text_color(t.text_muted)
        .on_click(choices(
            menu_key,
            "Move rule",
            targets
                .iter()
                .map(|(to, name)| (Some(to.clone()), format!("Move to {name}").into()))
                .collect(),
            None,
            {
                let (scope, id) = (scope.clone(), id.clone());
                move |d, to, cx| {
                    if let Some(to) = to {
                        d.move_rule(&scope, &to, &id, cx);
                    }
                }
            },
            cx,
        ));
    let delete = Button::new(SharedString::from(format!("perm-delete-{id}")))
        .ghost()
        .small()
        .icon(IconName::X)
        .accessibility_label(format!("Delete {}", rule.pattern))
        .on_click(cx.listener(move |d, _, _, cx| {
            d.change_scope(&scope, |s| s.rules.retain(|r| r.id != id), cx);
        }));
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .mx(px(-6.))
        .rounded(px(radius_sm()))
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
    let options = PERMISSION_KINDS
        .iter()
        .map(|&(k, label, _)| (k, label.into()))
        .collect();
    let set = |d: &mut SettingsDialog, k, cx: &mut Context<SettingsDialog>| {
        d.permissions.kind = k;
        cx.notify();
    };
    let select = select_box(key, "Kind of rule", kind_label(kind), open, &t).on_click(choices(
        key,
        "Kind of rule",
        options,
        kind,
        set,
        cx,
    ));
    let empty = pattern.trim().is_empty();
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(16.))
        .py(px(10.))
        .child(select)
        .child(field.flex_1().font_family(font_mono()))
        .child(
            Button::new("perm-add-rule")
                .small()
                .disabled(empty)
                .label("Add rule")
                .when(!empty, |b| {
                    b.on_click(cx.listener(|d, _, _, cx| d.add_rule(cx)))
                }),
        )
}
