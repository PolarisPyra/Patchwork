//! Buttons, checkboxes, badges, dropdown chips and the input box.
//!
//! They share the focus ring drawn here.

// Rust guideline compliant 2026-10-09

use gpui::{
    App, BoxShadow, Div, ElementId, FontWeight, Hsla, Role, SharedString, Stateful,
    StyleRefinement, div, point, prelude::*, px,
};
use patchwork_ui_theme::{IconName, fs_md, fs_sm, radius_sm, Theme, theme};
use patchwork_ui_theme::icon;

/// `:focus-visible`: the 2px outline a control shows while the keyboard, not the mouse, focused it.
/// Drawn just inside its edge, as a shadow outside would fill a control with no background.
#[must_use]
pub fn focus_ring(s: StyleRefinement) -> StyleRefinement {
    s.shadow(vec![BoxShadow {
        color: patchwork_ui_theme::border_focus(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(2.),
        inset: true,
    }])
}

/// A focusable button: Enter and Space click it, and assistive tech reads `label`.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .tab_index(0)
        .focus_visible(focus_ring)
        .cursor_pointer()
        .flex()
        .items_center()
}

/// `.btn-icon`: a 28px square icon button, muted until hovered.
pub fn icon_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    name: IconName,
    size: f32,
    cx: &App,
) -> Stateful<Div> {
    let t = theme(cx);
    let (hover, fg) = (t.bg_hover, t.text);
    button(id, label)
        .size(px(28.))
        .justify_center()
        .rounded(px(radius_sm()))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, size))
}

/// A checkbox and its label (`<label><input type="checkbox">…</label>`); the row toggles it.
pub fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    t: &Theme,
) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::CheckBox)
        .aria_label(label.clone())
        .aria_toggled(checked.into())
        .tab_index(0)
        .focus_visible(focus_ring)
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(13.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border_1()
                .map(|b| {
                    if checked {
                        b.border_color(t.accent)
                            .bg(t.accent)
                            .text_color(t.text_on_accent)
                            .child(icon(IconName::Check, 10.))
                    } else {
                        b.border_color(t.border_strong).bg(t.bg_editor)
                    }
                }),
        )
        .child(label)
}

/// `color-mix(in srgb, a pct%, b)`.
#[must_use]
pub fn mix(a: Hsla, b: Hsla, pct: f32) -> Hsla {
    let (a, b) = (gpui::Rgba::from(a), gpui::Rgba::from(b));
    let p = pct / 100.;
    gpui::Rgba {
        r: a.r * p + b.r * (1. - p),
        g: a.g * p + b.g * (1. - p),
        b: a.b * p + b.b * (1. - p),
        a: a.a * p + b.a * (1. - p),
    }
    .into()
}

/// `.btn`'s variants: the plain button and the coloured ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtnKind {
    Plain,
    Primary,
    Success,
}

/// `.btn`: a 28px button with a border, its label at `--fs-md`. A disabled one is faded and does
/// not react to the pointer; its owner leaves out the click handler.
pub fn btn(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: BtnKind,
    disabled: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (bg, border, fg, hover) = match kind {
        BtnKind::Plain => (t.bg_editor, t.border_strong, t.text, t.bg_hover),
        BtnKind::Primary => (t.accent, t.accent, t.text_on_accent, t.accent_hover),
        BtnKind::Success => (t.success, t.success, t.text_on_accent, darken(t.success)),
    };
    let base = button(id, label)
        .flex_none()
        .h(px(28.))
        .justify_center()
        .gap(px(6.))
        .px(px(14.))
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_size(px(fs_md()))
        .whitespace_nowrap();
    if disabled {
        base.opacity(0.5).cursor_default()
    } else {
        base.hover(move |s| s.bg(hover))
    }
}

/// `filter: brightness(0.92)`.
pub(crate) fn darken(c: Hsla) -> Hsla {
    Hsla { l: c.l * 0.92, ..c }
}

/// `.badge`: a small count or tag.
pub fn badge(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .flex_none()
        .h(px(20.))
        .flex()
        .items_center()
        .px(px(6.))
        .rounded(px(radius_sm()))
        .bg(t.bg_selected_muted)
        .text_size(px(patchwork_ui_theme::fs_xs()))
        .text_color(t.text_muted)
        .child(text.into())
}

/// A chip that shows what is picked and opens a menu of the choices (`LogLevelPicker` and the
/// other dropdowns): `.chip`, a borderless button with its value and a caret.
pub fn dropdown_chip(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    current: impl Into<SharedString>,
    open: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(id.into(), label)
        .aria_expanded(open)
        .flex_none()
        .h(px(28.))
        .gap(px(6.))
        .rounded(px(radius_sm()))
        .px(px(6.))
        .text_size(px(fs_md()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(div().max_w(px(180.)).truncate().child(current.into()))
        .child(icon(IconName::CaretDown, 14.))
}

/// The input every form shares: one height, border and focus colour.
#[must_use]
pub fn input_box(focused: bool, t: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(28.))
        .min_w_0()
        .overflow_hidden()
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(if focused { t.accent } else { t.border_strong })
        .bg(t.bg_editor)
        .px(px(8.))
        .text_size(px(fs_sm()))
        .text_color(t.text)
}
