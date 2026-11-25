//! Popups, menus' rows and tooltips.

// Rust guideline compliant 2026-10-09

use gpui::{
    Anchor, App, BoxShadow, Div, ElementId, Role, SharedString, Stateful, Window, anchored,
    deferred, div, point, prelude::*, px,
};
use crate::theme::{fs_md, radius, radius_sm, Theme, theme};

/// `Dropdown`'s menu: a raised box with a strong border and the popup shadow.
#[must_use]
pub fn popup(t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w(px(200.))
        .rounded(px(radius()))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_elevated)
        .p(px(4.))
        .text_size(px(fs_md()))
        .shadow(popup_shadow(t))
}

/// `--shadow-popup`: `0 6px 24px`.
#[must_use]
pub fn popup_shadow(t: &Theme) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: t.shadow,
        offset: point(px(0.), px(6.)),
        blur_radius: px(24.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// Lays `content` over the window with its bottom-left corner 4px above the top-left of the
/// element this is a child of, which must be `relative`: a menu that opens up from its chip.
pub fn above(content: impl IntoElement) -> Div {
    div().absolute().top_0().left_0().child(
        deferred(
            anchored()
                .anchor(Anchor::BottomLeft)
                .offset(point(px(0.), px(-4.)))
                .snap_to_window_with_margin(px(8.))
                .child(content),
        )
        .with_priority(1),
    )
}

/// A menu row that picks one value of several (`role="menuitemradio"`): the picked one shaded, and
/// the one under the keyboard highlighted as the pointer would.
pub fn menu_item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    highlighted: bool,
    t: &Theme,
) -> Stateful<Div> {
    let hover = t.bg_hover;
    div()
        .id(id)
        .role(Role::MenuItemRadio)
        .aria_label(label)
        .aria_toggled(checked.into())
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(6.))
        .min_w(px(160.))
        .rounded(px(radius_sm()))
        .px(px(8.))
        .h(px(crate::theme::roomy(26.)))
        .whitespace_nowrap()
        .text_color(t.text)
        .when(checked, |row| row.bg(t.bg_selected_muted))
        .when(highlighted && !checked, |row| row.bg(hover))
        .hover(move |s| s.bg(hover))
}

/// A tooltip: a small box with `text` beside the pointer.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView {
    let text = text.into();
    move |_, cx| {
        let text = text.clone();
        cx.new(|_| Tooltip(text)).into()
    }
}

pub(crate) struct Tooltip(pub(crate) SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        div()
            .max_w(px(360.))
            .rounded(px(radius_sm()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_elevated)
            .px(px(6.))
            .py(px(2.))
            .text_size(px(crate::theme::fs_sm()))
            .line_height(px(16.))
            .text_color(t.text)
            .shadow(popup_shadow(t))
            .children(self.0.split('\n').map(|line| div().child(line.to_string())))
    }
}
