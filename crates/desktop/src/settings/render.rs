//! The Settings dialog drawn.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, FontWeight, MouseButton, Role, SharedString, Window, div, prelude::*, px};
use crate::ui::menu::Menu;
use crate::ui::theme::{FS_LG, FS_XS, radius_sm, theme};
use crate::ui::{icon, icon_button, mix, overflows, scrollbar};
use super::dialog::{GROUPS, SettingsDialog, SettingsEvent, sections};
use super::chat;
use super::controls;
use super::extensions;
use super::general;
use super::mcp;
use super::permissions;
use super::projects;
use super::providers;
use super::scheduled;
use super::shortcuts;
use super::ssh;
use super::terminal;
use super::themes;
use super::tokens;
use super::updates;

impl SettingsDialog {
    pub(super) fn pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            "general" => general::render(self, cx),
            "chat" => chat::render(self, cx),
            "themes" => themes::render(self, window, cx),
            "shortcuts" => shortcuts::render(self, window, cx),
            "providers" => providers::render(self, window, cx),
            "tokens" => tokens::render(self, cx),
            "extensions" => extensions::render(self, window, cx),
            "mcp" => mcp::render(self, window, cx),
            "permissions" => permissions::render(self, window, cx),
            "projects" => projects::render(self, cx),
            "ssh" => ssh::render(self, window, cx),
            "scheduled" => scheduled::render(self, window, cx),
            "terminal" => terminal::render(self, window, cx),
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
        let open = sections()
            .find(|s| s.id == self.section)
            .map_or("General", |s| s.label);
        let overflows = overflows(&self.scroll, window, cx);
        let section = self.section;
        let nav = GROUPS.iter().enumerate().map(|(g, (label, entries))| {
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .when(g > 0, |d| d.mt(px(12.)))
                .child(
                    div()
                        .px(px(8.))
                        .pb(px(2.))
                        .text_size(px(FS_XS))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.text_subtle)
                        .child(*label),
                )
                .children(entries.iter().map(|s| {
                    let current = s.id == section;
                    let id = s.id;
                    let selected_hover = mix(t.bg_selected, t.text, 92.);
                    crate::ui::button(SharedString::from(format!("section-{id}")), s.label)
                        .aria_selected(current)
                        .h(px(32.))
                        .gap(px(10.))
                        .rounded(px(radius_sm()))
                        .px(px(8.))
                        .text_color(t.text)
                        .map(|b| {
                            if current {
                                b.bg(t.bg_selected).hover(move |s| s.bg(selected_hover))
                            } else {
                                b.hover(move |s| s.bg(t.bg_hover))
                            }
                        })
                        .child(icon(s.icon, 18.))
                        .child(s.label)
                        .on_click(cx.listener(move |d, _, _, cx| d.open_section(id, cx)))
                }))
        });
        let nav: Vec<_> = nav.collect();
        let pane = self.pane(window, cx);
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
                            .w(px(192.))
                            .overflow_y_scroll()
                            .border_r_1()
                            .border_color(t.border)
                            .bg(t.bg_panel)
                            // The web list always keeps room for its 10px scrollbar.
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
                                            .text_size(px(FS_LG))
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
