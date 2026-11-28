//! Popups, menus' rows and tooltips.

// Rust guideline compliant 2026-10-09

use gpui::{
    Anchor, App, BoxShadow, Div, ElementId, Role, SharedString, Stateful, Window, anchored,
    deferred, div, point, prelude::*, px,
};
use patchwork_design_theme::{fs_md, radius, radius_sm, Theme, theme};

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

/// The shadow under popovers, menus, selects and toasts: `0 4px 12px`. The surfaces sit close to
/// the window, so the shadow is kept short; a longer blur reads as a glow round the edge.
#[must_use]
pub fn popup_shadow(t: &Theme) -> Vec<BoxShadow> {
    drop_shadow(t, 4., 12.)
}

/// The shadow under the composer's slash and mention menus: `0 6px 16px`, a step deeper than a
/// popover's, as the menu floats over the transcript rather than beside its trigger.
#[must_use]
pub fn suggestion_shadow(t: &Theme) -> Vec<BoxShadow> {
    drop_shadow(t, 6., 16.)
}

/// The shadow under the search palette and the orchestrator's dropdown: `0 8px 20px`.
#[must_use]
pub fn palette_shadow(t: &Theme) -> Vec<BoxShadow> {
    drop_shadow(t, 8., 20.)
}

/// The shadow under dialogs and the Settings window: `0 10px 28px`, the deepest, as a dialog
/// stands over everything else in the window.
#[must_use]
pub fn dialog_shadow(t: &Theme) -> Vec<BoxShadow> {
    drop_shadow(t, 10., 28.)
}

/// A shadow in the theme's shadow colour, `y` below the element and blurred `blur`.
fn drop_shadow(t: &Theme, y: f32, blur: f32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: t.shadow,
        offset: point(px(0.), px(y)),
        blur_radius: px(blur),
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
        .py(px(patchwork_design_theme::roomy(4.)))
        .min_h(px(patchwork_design_theme::roomy(26.)))
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
            .text_size(px(patchwork_design_theme::fs_sm()))
            .line_height(px(16.))
            .text_color(t.text)
            .shadow(popup_shadow(t))
            .children(self.0.split('\n').map(|line| div().child(line.to_string())))
    }
}
