//! The dock drawn, as VS Code's panel: a title bar with its tabs at the left and the tools at the
//! right, over the tab that shows.

// Rust guideline compliant 2026-10-09

use gpui::{Role, Window, div, prelude::*, px};
use patchwork_design_components::menu::Menu;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{fs_sm, radius_sm, theme};
use patchwork_design_components::icon_button;
use super::open_tabs::{Dock, DockEvent, DockTab};
use super::tabs::TOOL;

/// The height of the panel's title bar, as the web panel's.
const TITLE_H: f32 = 35.;

/// The room around the terminal's screen: a little at the top, and a gutter at the left.
const SCREEN_TOP: f32 = 4.;
const SCREEN_LEFT: f32 = 20.;

impl Render for Dock {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_dock(window, cx)
    }
}

impl Dock {
    fn render_dock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t = *theme(cx);
        if self.focus_terminal && self.shown().next().is_some() {
            self.focus_terminal = false;
            window.focus(&self.term_focus, cx);
        }
        let tools = self.header_tools(window, cx);
        let body = match self.tab {
            DockTab::Terminal => {
                let tabs = (self.shown().count() > 1).then(|| self.shell_tabs(cx));
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .bg(t.bg_editor)
                    .children(tabs)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .pt(px(SCREEN_TOP))
                            .pl(px(SCREEN_LEFT))
                            .child(self.terminal_view(window, cx)),
                    )
                    .into_any_element()
            }
            DockTab::Output => div()
                .flex()
                .flex_1()
                .min_h_0()
                .bg(t.bg_editor)
                .child(self.output_view(window, cx))
                .into_any_element(),
            DockTab::Screen => div()
                .flex()
                .flex_1()
                .min_h_0()
                .bg(t.bg_editor)
                .child(self.screen_view(window, cx))
                .into_any_element(),
        };
        // Maximized, the close button gives the panel its own height back instead.
        let close = if self.maximized {
            icon_button(
                "panel-restore",
                "Restore panel size",
                IconName::CaretUp,
                14.,
                cx,
            )
            .size(px(TOOL))
            .tooltip("Restore panel size")
            .on_click(cx.listener(|this, _, _, cx| this.set_maximized(false, cx)))
        } else {
            icon_button("panel-close", "Close panel", IconName::X, 14., cx)
                .size(px(TOOL))
                .tooltip("Close panel (Ctrl+`)")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(DockEvent::Close)))
        };
        div()
            .id("panel")
            .role(Role::Region)
            .aria_label("Panel")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(t.bg_panel)
            .child(
                div()
                    .h(px(TITLE_H))
                    .flex_none()
                    .flex()
                    .items_stretch()
                    .border_b_1()
                    .border_color(t.border)
                    .child(
                        div()
                            .id("panel-tabs")
                            .role(Role::TabList)
                            .aria_label("Panel views")
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_stretch()
                            .child(self.tab_button(DockTab::Terminal, "Terminal", cx))
                            .child(self.tab_button(DockTab::Output, "Output", cx))
                            .when(self.computer_use, |tabs| {
                                tabs.child(self.tab_button(DockTab::Screen, "Screen", cx))
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(2.))
                            .px(px(6.))
                            .children(tools)
                            .child(close),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .child(body)
                    // A problem shows over the top of the view, as the web panel's does.
                    .children(self.error.clone().map(|e| {
                        div()
                            .id("panel-error")
                            .role(Role::Alert)
                            .absolute()
                            .top(px(4.))
                            .left(px(8.))
                            .right(px(8.))
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(radius_sm()))
                            .bg(t.danger_soft)
                            .text_size(px(fs_sm()))
                            .text_color(t.danger)
                            .child(e)
                    })),
            )
            .children(
                self.menu
                    .as_ref()
                    .map(|(_, menu, at)| Menu::view(menu, *at)),
            )
    }
}
