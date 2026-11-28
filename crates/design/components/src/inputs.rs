//! Buttons, checkboxes, badges, dropdown chips and the input box.
//!
//! They share the focus ring drawn here.

// Rust guideline compliant 2026-10-09

use gpui::component::button::{Button, ButtonVariants};
use gpui::component::Disableable;
use gpui::component::checkbox::Checkbox;
use gpui::component::tag::Tag;
use gpui::{
    App, BoxShadow, Div, ElementId, FontWeight, Hsla, Role, SharedString, Stateful,
    StyleRefinement, div, point, prelude::*, px,
};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{fs_md, fs_sm, radius_sm, Theme, theme};
use patchwork_design_icons::icon;

/// `:focus-visible`: the 2px outline a control shows while the keyboard, not the mouse, focused it.
/// Drawn just inside its edge, as a shadow outside would fill a control with no background.
#[must_use]
pub fn focus_ring(s: StyleRefinement) -> StyleRefinement {
    s.shadow(vec![BoxShadow {
        color: patchwork_design_theme::border_focus(),
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

/// `.btn-icon`: gpui-component's ghost icon button at 28px square, muted until hovered.
pub fn icon_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    name: IconName,
    size: f32,
    cx: &App,
) -> Button {
    let t = theme(cx);
    // The kit's ghost variant owns the hover; a second `hover` panics in debug builds.
    Button::new(id)
        .ghost()
        .accessibility_label(label)
        .size(px(28.))
        // The icon is a child, so the kit pads it as a labelled button; that padding would clip it.
        .p_0()
        .rounded(px(radius_sm()))
        .text_color(t.text_muted)
        .child(icon(name, size))
}

/// gpui-component's checkbox and its label; the row toggles it.
pub fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
) -> Checkbox {
    Checkbox::new(id).label(label.into()).checked(checked)
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

/// `.btn`: gpui-component's button at 28px, its label at `--fs-md`.
///
/// It draws in `t`, which over a backdrop is that backdrop's theme. A disabled one is faded and
/// does not react to the pointer. Assistive tech reads `label`; the caller adds what it shows.
pub fn btn(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: BtnKind,
    disabled: bool,
    t: &Theme,
) -> Button {
    let (bg, border, fg) = match kind {
        BtnKind::Plain => (t.bg_editor, t.border_strong, t.text),
        BtnKind::Primary => (t.accent, t.accent, t.text_on_accent),
        BtnKind::Success => (t.success, t.success, t.text_on_accent),
    };
    // The kit owns the hover, in the window theme's `button_*_hover` colours.
    Button::new(id)
        .accessibility_label(label)
        .map(|b| match kind {
            BtnKind::Plain => b,
            BtnKind::Primary => b.primary(),
            BtnKind::Success => b.success(),
        })
        .disabled(disabled)
        .flex_none()
        .h(px(28.))
        .gap(px(6.))
        .px(px(14.))
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_size(px(fs_md()))
        .whitespace_nowrap()
}

/// `.badge`: a small count or tag, on gpui-component's tag.
pub fn badge(text: impl Into<SharedString>, t: &Theme) -> Tag {
    Tag::custom(t.bg_selected_muted, t.text_muted, t.bg_selected_muted)
        .border_0()
        .flex_none()
        .h(px(20.))
        .px(px(6.))
        .py_0()
        .rounded(px(radius_sm()))
        .text_size(px(patchwork_design_theme::fs_xs()))
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
