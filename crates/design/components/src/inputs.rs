//! The focusable base, the input box and the focus ring they share.
//!
//! They share the focus ring drawn here.

// Rust guideline compliant 2026-10-09

use gpui::{
    BoxShadow, Div, ElementId, Hsla, Role, SharedString, Stateful, StyleRefinement, div, point,
    prelude::*, px,
};
use patchwork_design_theme::{Theme, fs_sm, radius_sm};

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
