//! The Settings dialog drawn: the list at the left, and the open page with its header.

// Rust guideline compliant 2026-10-09

use gpui::component::button::Button;
use gpui::component::button::ButtonVariants as _;
use gpui::component::Sizable as _;
use gpui::{AnyElement, FontWeight, MouseButton, Role, Window, div, prelude::*, px};
use patchwork_design_components::menu::Menu;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{fs_lg, fs_xs, theme};
use patchwork_design_components::{overflows, scrollbar};
use super::dialog::{SettingsDialog, SettingsEvent};
use super::pages::{PAGES, page};
use super::chat;
use super::logs;
use super::controls;
use super::costs;
use super::devices;
use super::extensions;
use super::gateway;
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

/// The list of pages' width, as on the web.
pub(super) const NAV_WIDTH: f32 = 236.;

/// The page's padding at its sides and foot, as on the web, so sections sit well in from the
/// list and the dialog's edge.
const PAGE_PAD: f32 = 32.;

/// The dialog's width and height in `window`: as large as it is drawn, leaving a margin round it.
pub(super) fn dialog_size(window: &Window) -> (f32, f32) {
    let viewport = window.viewport_size();
    (
        (f32::from(viewport.width) - 48.).min(1040.),
        (f32::from(viewport.height) - 48.).min(720.),
    )
}

impl SettingsDialog {
    /// The open page's content, in the sections it is built from.
    pub(super) fn pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            "general" => {
                let mut parts = chat::sections(self, cx);
                parts.extend(terminal::sections(self, window, cx));
                stack(parts)
            }
            "appearance" => themes::appearance(self, window, cx),
            "shortcuts" => shortcuts::render(self, window, cx),
            "notifications" => notifications::render(self, cx),
            "providers" => providers::render(self, window, cx),
            "permissions" => permissions::render(self, window, cx),
            "agents" => extensions::render(self, extensions::Tab::Agents, window, cx),
            "skills" => extensions::render(self, extensions::Tab::Skills, window, cx),
            "hooks" => extensions::render(self, extensions::Tab::Hooks, window, cx),
            "mcp" => mcp::render(self, window, cx),
            "projects" => stack(vec![
                projects::render(self, cx),
                worktrees::render(self, cx),
            ]),
            "scheduled" => scheduled::render(self, window, cx),
            "devices" => devices::render(self, cx),
            "ssh" => ssh::render(self, window, cx),
            "chat-apps" => gateway::render(self, window, cx),
            "usage" => stack(vec![
                tokens::render(self, cx),
                controls::Section::new("Costs")
                    .bare()
                    .child(costs::render(self, cx))
                    .into_any_element(),
            ]),
            "vault" => security::render(self, window, cx),
            "logs" => logs::render(self, cx),
            "about" => updates::render(self, cx),
            _ => div().into_any_element(),
        }
    }
}

/// Sections from more than one source, one under the other.
fn stack(parts: Vec<AnyElement>) -> AnyElement {
    div().flex().flex_col().children(parts).into_any_element()
}

impl Render for SettingsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let nav_focus = self
            .general
            .nav_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let page = page(self.section).or_else(|| PAGES.first());
        let (group, title) = page.map_or(("", ""), |p| (p.group, p.label));
        // A found row fades over several frames; keep drawing until it is done.
        if controls::flashing() {
            window.request_animation_frame();
        }
        let overflows = overflows(&self.scroll, window, cx);
        let field = self.nav_field(window, cx);
        let list = self.nav_list(cx);
        // The shortcuts page fills the height and scrolls its own list, so its search stays in view.
        let fills = self.section == "shortcuts";
        let body = div()
            .relative()
            .when(fills, |d| d.flex().flex_col().flex_1().min_h_0())
            .child(self.pane(window, cx));
        let menu = self
            .menu
            .as_ref()
            .map(|(_, menu, at)| Menu::view(menu, *at));
        let (width, height) = dialog_size(window);
        patchwork_design_components::dialog::backdrop("settings-backdrop", true)
            .items_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Closed)),
            )
            .child(
                patchwork_design_components::dialog::card("settings", "Settings", &self.focus, &t)
                    .key_context("Settings")
                    .on_key_down(cx.listener(Self::on_key))
                    .on_action(cx.listener(
                        |d, _: &patchwork_views_actions::FocusNext, window, cx| {
                            d.cycle_focus(true, window, cx);
                        },
                    ))
                    .on_action(cx.listener(
                        |d, _: &patchwork_views_actions::FocusPrev, window, cx| {
                            d.cycle_focus(false, window, cx);
                        },
                    ))
                    .flex()
                    .h(px(height))
                    .w(px(width))
                    .overflow_hidden()
                    .child(
                        div()
                            .id("settings-pages")
                            .role(Role::Navigation)
                            .aria_label("Settings pages")
                            .track_focus(&nav_focus)
                            .flex()
                            .flex_col()
                            .flex_none()
                            .w(px(NAV_WIDTH))
                            .border_r_1()
                            .border_color(t.border)
                            .bg(t.bg_app)
                            .child(div().flex_none().p(px(10.)).pb(px(4.)).child(field))
                            .child(
                                div()
                                    .id("settings-page-list")
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .px(px(10.))
                                    .pt(px(4.))
                                    .pb(px(12.))
                                    .children(list),
                            ),
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
                                    .gap(px(12.))
                                    .px(px(PAGE_PAD))
                                    .pt(px(24.))
                                    .pb(px(16.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .flex_1()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .text_size(px(fs_xs()))
                                                    .text_color(t.text_subtle)
                                                    .child(format!("{group} › {title}")),
                                            )
                                            .child(
                                                div()
                                                    .id("settings-title")
                                                    .role(Role::Heading)
                                                    .mt(px(2.))
                                                    .text_size(px(fs_lg() * 1.2))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(t.text)
                                                    .child(title),
                                            ),
                                    )
                                    .child(
                                        // Pulled up and out so the X sits in the corner, as on the web.
                                        div().mt(px(-4.)).mr(px(-12.)).child(
                                            Button::new("settings-close")
                                                .ghost()
                                                .small()
                                                .icon(IconName::X)
                                                .accessibility_label("Close")
                                                .on_click(cx.listener(|_, _, _, cx| {
                                                    cx.emit(SettingsEvent::Closed);
                                                })),
                                        ),
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
                                            .aria_label(title)
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
                                            // The side padding, and room for the scrollbar when there is one.
                                            .pl(px(PAGE_PAD))
                                            .pr(px(if overflows {
                                                PAGE_PAD + 4.
                                            } else {
                                                PAGE_PAD
                                            }))
                                            .pb(px(PAGE_PAD))
                                            .children(self.error.clone().map(|e| {
                                                controls::alert("settings-error", e, t.danger)
                                                    .mb(px(16.))
                                            }))
                                            .child(body),
                                    )
                                    .children(scrollbar(&self.scroll, 10., 2., t.border_strong)),
                            ),
                    ),
            )
            .children(menu)
    }
}
