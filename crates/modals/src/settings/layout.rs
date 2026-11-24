//! The Settings dialog drawn.

// Rust guideline compliant 2026-10-09

use std::time::Duration;
use gpui::{
    Animation, AnimationExt, ease_out_quint, AnyElement, FontWeight, MouseButton, Role, Window,
    div, prelude::*, px,
};
use patchwork_ui::controls::menu::Menu;
use patchwork_ui::theme::{fs_lg, fs_sm, fs_xs, theme};
use patchwork_ui::{icon_button, overflows, scrollbar};
use super::dialog::{SettingsDialog, SettingsEvent, entry_of};
use super::panes::chat;
use super::panes::logs;
use super::controls;
use super::panes::costs;
use super::panes::extensions;
use super::panes::gateway;
use super::panes::notifications;
use super::panes::mcp;
use super::panes::permissions;
use super::panes::projects;
use super::panes::providers;
use super::panes::scheduled;
use super::panes::security;
use super::panes::shortcuts;
use super::panes::ssh;
use super::panes::terminal;
use super::panes::themes;
use super::panes::tokens;
use super::panes::updates;
use super::panes::worktrees;

/// The list of entries' width.
pub(super) const NAV_WIDTH: f32 = 236.;

/// The dialog's width and height in `window`: as large as it is drawn, leaving a margin round it.
pub(super) fn dialog_size(window: &Window) -> (f32, f32) {
    let viewport = window.viewport_size();
    (
        (f32::from(viewport.width) - 48.).min(1040.),
        (f32::from(viewport.height) - 48.).min(720.),
    )
}

impl SettingsDialog {
    pub(super) fn pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            "chat" => chat::render(self, cx),
            "logs" => logs::render(self, cx),
            "themes" => themes::render(self, window, cx),
            "code-theme" => themes::code_theme_pane(self, window, cx),
            "orchestrator-background" => themes::orchestrator_background_pane(self, window, cx),
            "chat-background" => themes::chat_background_pane(self, window, cx),
            "quilts-background" => themes::quilts_background_pane(self, cx),
            "interface" => themes::interface_pane(self, window, cx),
            "shortcuts" => shortcuts::render(self, window, cx),
            "providers" => providers::render(self, window, cx),
            "tokens" => tokens::render(self, cx),
            "costs" => costs::render(self, cx),
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
        let (trail, open, about) =
            entry_of(self.section).map_or((String::new(), "General", ""), |(_, entry, pane)| {
                // A pane of its own entry is named once; one of several is named under its entry.
                let trail = if entry.panes.len() > 1 {
                    format!("{} › {}", entry.group, entry.label)
                } else {
                    entry.group.to_owned()
                };
                (trail, pane.label, pane.desc)
            });
        let overflows = overflows(&self.scroll, window, cx);
        let nav = self.nav(window, cx);
        // The shortcuts pane fills the height and scrolls its own list, so its search stays in view.
        let fills = self.section == "shortcuts";
        // A pane fades in as it opens.
        let pane = div()
            .when(fills, |d| d.flex().flex_col().flex_1().min_h_0())
            .child(self.pane(window, cx))
            .with_animation(
                self.section,
                Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
                gpui::Styled::opacity,
            );
        let menu = self
            .menu
            .as_ref()
            .map(|(_, menu, at)| Menu::view(menu, *at));
        let (width, height) = dialog_size(window);
        patchwork_ui::controls::dialog::backdrop("settings-backdrop", true)
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Closed)),
            )
            .child(
                patchwork_ui::controls::dialog::card("settings", "Settings", &self.focus, &t)
                    .key_context("Settings")
                    .on_key_down(cx.listener(Self::on_key))
                    .on_action(cx.listener(|d, _: &patchwork_ui::FocusNext, window, cx| {
                        d.cycle_focus(true, window, cx);
                    }))
                    .on_action(cx.listener(|d, _: &patchwork_ui::FocusPrev, window, cx| {
                        d.cycle_focus(false, window, cx);
                    }))
                    .flex()
                    .h(px(height))
                    .w(px(width))
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
                            .w(px(NAV_WIDTH))
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
                                    .items_start()
                                    .justify_between()
                                    .gap(px(16.))
                                    .pt(px(12.))
                                    // A pane's first row brings its own 10px above it.
                                    .pb(px(2.))
                                    .pr(px(16.))
                                    .pl(px(20.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .min_w_0()
                                            .gap(px(2.))
                                            .child(
                                                div()
                                                    .text_size(px(fs_xs()))
                                                    .text_color(t.text_muted)
                                                    .child(trail),
                                            )
                                            .child(
                                                div()
                                                    .id("settings-title")
                                                    .role(Role::Heading)
                                                    .text_size(px(fs_lg()))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(open),
                                            )
                                            .when(!about.is_empty(), |d| {
                                                d.child(
                                                    div()
                                                        .text_size(px(fs_sm()))
                                                        .text_color(t.text_muted)
                                                        .child(about),
                                                )
                                            }),
                                    )
                                    .child(
                                        icon_button("settings-close", "Close", "x", 16., cx)
                                            .tooltip(patchwork_ui::tooltip("Close"))
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
                                            .map(|d| {
                                                if fills {
                                                    d.overflow_hidden()
                                                } else {
                                                    d.overflow_y_scroll()
                                                }
                                            })
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
