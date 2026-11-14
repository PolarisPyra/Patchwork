//! The sidebar drawn: its sections and their labels.

// Rust guideline compliant 2026-10-09

use super::archived::archived_view;
use gpui::{
    AnyElement, Div, ExternalPaths, FontWeight, MouseButton, MouseDownEvent, Role, div, prelude::*,
    px,
};
use crate::ui::theme::{FS_MD, FS_SM, radius_sm, Theme, theme};
use crate::ui::{button, icon};
use crate::workspace::{SIDEBAR_MAX, SIDEBAR_MIN, View, Workspace};
use super::tasks::tasks_view;
use super::usage;

pub fn render(
    w: &mut Workspace,
    view: View,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    let t = *theme(cx);
    let width = w.sidebar_width;
    let (border_strong, fg) = (t.border_strong, t.text);
    let body = match view {
        View::Tasks => tasks_view(w, cx).into_any_element(),
        View::Archived => archived_view(w, cx).into_any_element(),
        View::Changes => div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(w.changes_view.clone())
            .into_any_element(),
    };
    div()
        .id("sidebar")
        .role(Role::Complementary)
        .aria_label("Sidebar")
        .relative()
        .flex()
        .flex_col()
        .flex_none()
        .min_h_0()
        .w(px(width))
        .border_r_1()
        .border_color(t.border)
        .bg(t.bg_panel)
        .text_size(px(13.))
        // Folders held over the project list light it up; dropped, each becomes a project.
        .when(view == View::Tasks, |s| {
            s.drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(t.bg_hover))
                .on_drop(cx.listener(|this, files: &ExternalPaths, _, cx| {
                    this.add_dropped_folders(files.paths(), cx);
                    cx.stop_propagation();
                }))
        })
        .child(
            div()
                .id("sidebar-resize")
                .role(Role::Splitter)
                .aria_orientation(gpui::Orientation::Vertical)
                .aria_label("Resize sidebar")
                .aria_numeric_value(f64::from(width))
                .aria_min_numeric_value(f64::from(SIDEBAR_MIN))
                .aria_max_numeric_value(f64::from(SIDEBAR_MAX))
                .tab_index(0)
                .absolute()
                .top_0()
                .bottom_0()
                .right(px(-3.))
                .w(px(6.))
                .cursor_col_resize()
                .hover(move |s| s.bg(t.accent.opacity(0.4)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, e: &MouseDownEvent, _, cx| {
                        this.start_resize(f32::from(e.position.x));
                        cx.stop_propagation();
                    }),
                )
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| {
                    let step = match e.keystroke.key.as_str() {
                        "left" => -16.,
                        "right" => 16.,
                        _ => return,
                    };
                    this.sidebar_width =
                        (this.sidebar_width + step).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
                    cx.notify();
                }))
                .on_click(cx.listener(|this, e: &gpui::ClickEvent, _, cx| {
                    if e.click_count() == 2 {
                        this.sidebar_width = crate::workspace::SIDEBAR_WIDTH;
                        cx.notify();
                    }
                })),
        )
        .when(matches!(view, View::Tasks | View::Archived), |s| {
            s.child(
                button("search", "Search…")
                    .mx(px(8.))
                    .mt(px(12.))
                    .mb(px(2.))
                    .h(px(32.))
                    .flex_none()
                    .gap(px(6.))
                    .rounded(px(radius_sm()))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.bg_input)
                    .px(px(8.))
                    .text_size(px(FS_MD))
                    .text_color(t.text_muted)
                    .hover(move |s| s.border_color(border_strong).text_color(fg))
                    .child(icon("magnifying-glass", 18.))
                    .child("Search…")
                    .child(
                        div()
                            .ml_auto()
                            .min_w(px(20.))
                            .h(px(20.))
                            .px(px(5.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .bg(t.bg_selected_muted)
                            .text_color(t.text_muted)
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Ctrl K"),
                    )
                    .on_click(cx.listener(|w, _, window, cx| w.toggle_palette(window, cx))),
            )
        })
        .child(body)
        .child(usage::footer(w, &t, cx))
        .children(w.context_menu_view(cx))
}

pub(super) fn empty_row(text: &'static str, t: &Theme) -> Div {
    div()
        .h(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .px(px(6.))
        .text_size(px(FS_MD))
        .text_color(t.text_muted)
        .child(text)
}

/// `SectionLabel`: a small uppercase heading with its action on the right.
pub(super) fn section_label(text: &str, first: bool, action: Option<AnyElement>, t: &Theme) -> Div {
    div()
        .flex_none()
        .mr(px(-2.))
        .flex()
        .items_center()
        .justify_between()
        .pt(px(if first { 4. } else { 10. }))
        .pl(px(6.))
        .text_size(px(FS_SM))
        .text_color(t.text_muted)
        .child(div().child(text.to_uppercase()))
        .children(action)
}

/// The chevron of a project or folder, pointing right while it is closed.
pub(super) fn twistie(open: bool) -> impl IntoElement {
    div()
        .w(px(16.))
        .flex_none()
        .flex()
        .justify_center()
        .ml(px(3.))
        .mr(px(3.))
        .child(icon(if open { "twistie" } else { "twistie-closed" }, 16.))
}
