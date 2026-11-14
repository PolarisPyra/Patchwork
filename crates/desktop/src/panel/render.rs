//! The panel drawn.

// Rust guideline compliant 2026-10-09

use gpui::{Role, Window, div, prelude::*, px};
use crate::ui::menu::Menu;
use crate::ui::theme::{fs_sm, theme};
use crate::ui::{icon_button, tooltip};
use super::state::{Panel, PanelEvent, PanelTab};

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        if self.focus_terminal && self.shown().next().is_some() {
            self.focus_terminal = false;
            window.focus(&self.term_focus, cx);
        }
        let tools = self.header_tools(cx);
        let body = match self.tab {
            PanelTab::Terminal => {
                let tabs = (self.shown().count() > 1).then(|| self.shell_tabs(cx));
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .pl(px(12.))
                            .bg(t.bg_editor)
                            .child(self.terminal_view(window, cx)),
                    )
                    .children(tabs)
                    .into_any_element()
            }
            PanelTab::Output => div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(self.output_view(window, cx))
                .into_any_element(),
            PanelTab::Screen => div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(self.screen_view(window, cx))
                .into_any_element(),
        };
        div()
            .id("panel")
            .role(Role::Region)
            .aria_label("Panel")
            .track_focus(&self.focus)
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(t.border)
            .bg(t.bg_editor)
            .child(
                div()
                    .h(px(32.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(12.))
                    .child(
                        div()
                            .id("panel-tabs")
                            .role(Role::TabList)
                            .aria_label("Panel views")
                            .h_full()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(self.tab_button(PanelTab::Terminal, "Terminal", cx))
                            .child(self.tab_button(PanelTab::Output, "Output", cx))
                            .when(self.computer_use, |tabs| {
                                tabs.child(self.tab_button(PanelTab::Screen, "Screen", cx))
                            }),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .children(tools)
                            .child(
                                icon_button("panel-close", "Close panel", "x", 14., cx)
                                    .size(px(24.))
                                    .tooltip(tooltip("Close panel"))
                                    .on_click(
                                        cx.listener(|_, _, _, cx| cx.emit(PanelEvent::Close)),
                                    ),
                            ),
                    ),
            )
            .children(self.error.clone().map(|e| {
                div()
                    .id("panel-error")
                    .role(Role::Alert)
                    .flex_none()
                    .px(px(12.))
                    .pb(px(4.))
                    .text_size(px(fs_sm()))
                    .text_color(t.danger)
                    .child(e)
            }))
            .child(body)
            .children(
                self.menu
                    .as_ref()
                    .map(|(_, menu, at)| Menu::view(menu, *at)),
            )
    }
}
