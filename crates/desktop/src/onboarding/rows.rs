//! The rows the setup steps share: headings, choices and ticks.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Div, FontWeight, Role, SharedString, Stateful, Toggled, div, prelude::*, px};
use crate::ui::theme::{fs_lg, fs_md, fs_sm, radius, radius_sm, Theme};
use crate::ui::{icon, mix};

/// A step's small heading.
pub(super) fn heading(text: &'static str) -> Div {
    div()
        .text_size(px(fs_md()))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

/// A picked setup choice: a plain, slightly lighter fill and a stronger border, no colour.
pub(super) fn picked(row: Stateful<Div>, t: &Theme) -> Stateful<Div> {
    row.border_color(t.border_strong)
        .bg(mix(t.text, t.bg_editor, 6.))
}

/// A row to tick (`AgentRow`, and a provider to import from): a logo tile, and a name with a line
/// under it. Its owner adds what sits at the end with `row_end`. Unless `enabled`, it can't be
/// ticked.
pub(super) fn choice_row(
    id: SharedString,
    name: &str,
    detail: String,
    logo: AnyElement,
    checked: bool,
    enabled: bool,
    t: &Theme,
) -> Stateful<Div> {
    let hover = t.bg_hover;
    let row = div()
        .id(id)
        .role(Role::CheckBox)
        .aria_label(SharedString::from(name.to_string()))
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .flex()
        .w_full()
        .items_center()
        .gap(px(16.))
        .rounded(px(radius()))
        .border_1()
        .p(px(16.))
        .text_color(t.text)
        .child(
            div()
                .size(px(44.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius()))
                .bg(t.bg_selected_muted)
                .child(logo),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(fs_lg()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name.to_string()),
                )
                .child(
                    div()
                        .text_size(px(fs_sm()))
                        .text_color(t.text_muted)
                        .child(detail),
                ),
        );
    let row = if checked {
        picked(row, t)
    } else {
        row.border_color(t.border)
    };
    if enabled {
        let row = row.tab_index(0).cursor_pointer();
        if checked {
            row
        } else {
            row.hover(move |s| s.bg(hover))
        }
    } else {
        row
    }
}

/// What sits at a choice row's end: its badges and tick.
pub(super) fn row_end(end: Vec<AnyElement>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .children(end)
}

/// The square tick at a choice's end.
pub(super) fn tick(checked: bool, t: &Theme) -> AnyElement {
    div()
        .size(px(18.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius_sm()))
        .border_1()
        .text_color(t.text_on_accent)
        .map(|b| {
            if checked {
                b.border_color(t.accent)
                    .bg(t.accent)
                    .child(icon("check-bold", 12.))
            } else {
                b.border_color(t.border_strong)
            }
        })
        .into_any_element()
}

/// `SharedString` for an element id made of a prefix and a key.
pub(super) fn key_id(prefix: &str, key: impl std::fmt::Debug) -> SharedString {
    SharedString::from(format!("{prefix}-{key:?}"))
}
