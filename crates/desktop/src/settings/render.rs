//! The Settings dialog drawn.

// Rust guideline compliant 2026-10-09

use std::time::Duration;
use gpui::{
    Animation, AnimationExt, ease_out_quint, AnyElement, FontWeight, MouseButton, Role, Window,
    div, prelude::*, px,
};
use crate::ui::menu::Menu;
use crate::ui::theme::{fs_lg, theme};
use crate::ui::{icon_button, overflows, scrollbar};
use super::dialog::{SettingsDialog, SettingsEvent, entry_of};
use super::chat;
use super::controls;
use super::extensions;
use super::gateway;
use super::general;
use super::notifications;
use super::mcp;
use super::permissions;
use super::projects;
use super::providers;
use super::scheduled;
use super::security;
use super::shortcuts;
use super::ssh;
use super::terminal;
use super::themes;
use super::tokens;
use super::updates;
use super::worktrees;

impl SettingsDialog {
    pub(super) fn pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            "general" => {
                let t = *theme(cx);
                div()
                    .flex()
                    .flex_col()
                    .child(general::render(self, cx))
                    .child(controls::section_heading("Chat", false, &t))
                    .child(chat::render(self, cx))
                    .into_any_element()
            }
            "themes" => themes::render(self, window, cx),
            "code-theme" => themes::code_theme_pane(self, window, cx),
            "orchestrator-background" => themes::orchestrator_background_pane(self, window, cx),
            "chat-background" => themes::chat_background_pane(self, window, cx),
            "interface" => themes::interface_pane(self, window, cx),
            "shortcuts" => shortcuts::render(self, window, cx),
            "providers" => providers::render(self, window, cx),
            "tokens" => tokens::render(self, cx),
            "extensions" => extensions::render(self, window, cx),
            "mcp" => mcp::render(self, window, cx),
            "gateway" => gateway::render(self, window, cx),
            "permissions" => permissions::render(self, window, cx),
            "security" => security::render(self, window, cx),
            "projects" => projects::render(self, cx),
            "ssh" => ssh::render(self, window, cx),
            "scheduled" => scheduled::render(self, window, cx),
            "worktrees" => worktrees::render(self, cx),
            "terminal" => terminal::render(self, window, cx),
            "notifications" => notifications::render(self, cx),
            "updates" => updates::render(self, cx),
            _ => div().into_any_element(),
        }
    }
}

impl Render for SettingsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let nav_focus = self
            .general
            .nav_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let open = entry_of(self.section).map_or("General", |(_, _, pane)| pane.label);
        let overflows = overflows(&self.scroll, window, cx);
        let nav = self.nav(window, cx);
        // A pane fades in as it opens.
        let pane = div().child(self.pane(window, cx)).with_animation(
            self.section,
            Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
            gpui::Styled::opacity,
        );
        let menu = self
            .menu
            .as_ref()
            .map(|(_, menu, at)| Menu::view(menu, *at));
        let viewport = window.viewport_size();
        crate::ui::dialog::backdrop("settings-backdrop", true)
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Closed)),
            )
            .child(
                crate::ui::dialog::card("settings", "Settings", &self.focus, &t)
                    .key_context("Settings")
                    .on_key_down(cx.listener(Self::on_key))
                    .on_action(cx.listener(|d, _: &crate::FocusNext, window, cx| {
                        d.cycle_focus(true, window, cx);
                    }))
                    .on_action(cx.listener(|d, _: &crate::FocusPrev, window, cx| {
                        d.cycle_focus(false, window, cx);
                    }))
                    .flex()
                    .h(px(680.))
                    .max_h(viewport.height - px(48.))
                    .w(px(920.))
                    .max_w(viewport.width - px(48.))
                    .overflow_hidden()
                    .child(
                        div()
                            .id("settings-sections")
                            .role(Role::Navigation)
                            .aria_label("Settings sections")
                            .track_focus(&nav_focus)
                            .flex()
                            .flex_col()
                            .flex_none()
                            .w(px(236.))
                            .overflow_y_scroll()
                            .border_r_1()
                            .border_color(t.border)
                            .bg(t.bg_panel)
                            // The web list always keeps room for its 10px scrollbar.
                            .gap(px(2.))
                            .pl(px(8.))
                            .pr(px(18.))
                            .py(px(10.))
                            .children(nav),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .flex_none()
                                    .h(px(48.))
                                    .items_center()
                                    .justify_between()
                                    .pr(px(16.))
                                    .pl(px(20.))
                                    .child(
                                        div()
                                            .id("settings-title")
                                            .role(Role::Heading)
                                            .text_size(px(fs_lg()))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(open),
                                    )
                                    .child(
                                        icon_button("settings-close", "Close", "x", 16., cx)
                                            .tooltip(crate::ui::tooltip("Close"))
                                            .on_click(cx.listener(|_, _, _, cx| {
                                                cx.emit(SettingsEvent::Closed);
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .relative()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h_0()
                                    .child(
                                        div()
                                            .id("settings-pane")
                                            .role(Role::Region)
                                            .aria_label(open)
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .min_h_0()
                                            .overflow_y_scroll()
                                            .track_scroll(&self.scroll)
                                            // 20px, and room for the scrollbar when there is one.
                                            .pl(px(20.))
                                            .pr(px(if overflows { 30. } else { 20. }))
                                            .pb(px(20.))
                                            .child(pane)
                                            .children(self.error.clone().map(|e| {
                                                controls::alert("settings-error", e, t.danger)
                                            })),
                                    )
                                    .children(scrollbar(&self.scroll, 10., 2., t.border_strong)),
                            ),
                    ),
            )
            .children(menu)
    }
}
