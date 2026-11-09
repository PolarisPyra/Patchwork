//! Settings (`settings/SettingsDialog.tsx`): the dialog around the panes, its list of sections in
//! groups, its title, and which pane is open. Each pane draws itself in its own file, and the
//! controls they share are in [`controls`]. A change is saved at once and handed to the workspace
//! with [`SettingsEvent::Changed`].

pub(crate) mod controls;
mod extensions;
mod general;
mod mcp;
pub(crate) mod permissions;
pub(crate) mod projects;
mod scheduled;
mod ssh;
mod terminal;
pub mod shortcuts;
mod sigils;
mod providers;
mod tokens;
mod updates;

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    KeyDownEvent, MouseButton, Pixels, Point, Role, ScrollHandle, SharedString, Window, div,
    prelude::*, px,
};
use patchwork_server::Server;
use patchwork_server::settings::Settings;

use crate::menu::Menu;
use crate::theme::{FS_LG, FS_XS, RADIUS_LG, RADIUS_SM, theme};
use crate::ui::{icon, icon_button, mix, overflows, popup_shadow, scrollbar};

use controls::Field;
pub(crate) use controls::dropdown_chip;
pub(crate) use general::LOG_LEVELS;

/// What the dialog tells the workspace.
pub enum SettingsEvent {
    /// A setting was saved; the workspace reads them again.
    Changed,
    /// Threads were added or removed; the workspace lists them again.
    ThreadsChanged,
    Closed,
}

/// One entry of the list: its id, its name, and its icon.
struct Section {
    id: &'static str,
    label: &'static str,
    icon: &'static str,
}

/// The panes, in groups under a heading, so the list reads in a few short runs.
const GROUPS: &[(&str, &[Section])] = &[
    (
        "General",
        &[
            Section {
                id: "general",
                label: "General",
                icon: "gear-six",
            },
            Section {
                id: "shortcuts",
                label: "Shortcuts",
                icon: "keyboard",
            },
            Section {
                id: "updates",
                label: "Updates",
                icon: "arrow-circle-up",
            },
        ],
    ),
    (
        "Models",
        &[
            Section {
                id: "providers",
                label: "Providers",
                icon: "sparkle",
            },
            Section {
                id: "tokens",
                label: "AI Tokenomics",
                icon: "gauge",
            },
        ],
    ),
    (
        "Agents",
        &[
            Section {
                id: "extensions",
                label: "Extensions",
                icon: "puzzle-piece",
            },
            Section {
                id: "mcp",
                label: "MCP Servers",
                icon: "plug",
            },
            Section {
                id: "permissions",
                label: "Permissions",
                icon: "shield-check",
            },
        ],
    ),
    (
        "Workspace",
        &[
            Section {
                id: "projects",
                label: "Projects",
                icon: "folder-simple",
            },
            Section {
                id: "scheduled",
                label: "Scheduled",
                icon: "clock",
            },
            Section {
                id: "terminal",
                label: "Terminal",
                icon: "terminal-window",
            },
            Section {
                id: "ssh",
                label: "SSH",
                icon: "hard-drives",
            },
        ],
    ),
];

fn sections() -> impl Iterator<Item = &'static Section> {
    GROUPS.iter().flat_map(|(_, sections)| sections.iter())
}

pub struct SettingsDialog {
    pub server: Arc<Server>,
    pub settings: Settings,
    section: &'static str,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// The text fields drawn, by a name each pane gives them.
    fields: HashMap<SharedString, Field>,
    /// The dropdown whose menu is open: its name, the menu, and where it opened.
    menu: Option<(SharedString, Entity<Menu>, Point<Pixels>)>,
    /// Why the last save failed.
    error: Option<String>,
    general: general::GeneralState,
    shortcuts: shortcuts::ShortcutsState,
    providers: providers::ProvidersState,
    extensions: extensions::ExtensionsState,
    mcp: mcp::McpState,
    permissions: permissions::PermissionsState,
    projects: projects::ProjectsState,
    ssh: ssh::SshState,
    scheduled: scheduled::ScheduledState,
    tokens: tokens::TokensState,
    updates: updates::UpdatesState,
}

impl EventEmitter<SettingsEvent> for SettingsDialog {}

impl Focusable for SettingsDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SettingsDialog {
    /// Opens on the section `section`, by id; None opens on General.
    pub fn new(
        server: Arc<Server>,
        section: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let section = section
            .and_then(|id| sections().find(|s| s.id == id))
            .map_or("general", |s| s.id);
        Self {
            settings: server.store.settings(),
            server,
            section,
            focus,
            scroll: ScrollHandle::new(),
            fields: HashMap::new(),
            menu: None,
            error: None,
            general: general::GeneralState::default(),
            shortcuts: shortcuts::ShortcutsState::default(),
            providers: providers::ProvidersState::default(),
            extensions: extensions::ExtensionsState::default(),
            mcp: mcp::McpState::default(),
            permissions: permissions::PermissionsState::default(),
            projects: projects::ProjectsState::default(),
            ssh: ssh::SshState::default(),
            scheduled: scheduled::ScheduledState::default(),
            tokens: tokens::TokensState::default(),
            updates: updates::UpdatesState::default(),
        }
    }

    /// Saves the settings as `change` leaves them.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let mut next = self.server.store.settings();
        change(&mut next);
        match self.server.save_settings(next) {
            Ok(()) => {
                self.error = None;
                self.settings = self.server.store.settings();
                cx.emit(SettingsEvent::Changed);
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }

    /// Runs `work` off the main thread, then hands its result to `done`.
    pub fn run<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&mut Self, T, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let task = cx.background_spawn(async move { work() });
        cx.spawn(async move |this, cx| {
            let out = task.await;
            let _ = this.update(cx, |d, cx| {
                done(d, out, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn open_section(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.section = id;
        self.menu = None;
        self.shortcuts.recording = None;
        self.shortcuts.recording_sigil = None;
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.record_key(e, cx) || self.record_sigil(e, cx) {
            return cx.stop_propagation();
        }
        let step = match e.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                if self.cancel_draft(cx) {
                    return;
                }
                return cx.emit(SettingsEvent::Closed);
            }
            "up" => -1,
            "down" => 1,
            _ => return,
        };
        // Up and Down move along the list while a section of it has focus.
        let all: Vec<&'static Section> = sections().collect();
        let Some(at) = all.iter().position(|s| s.id == self.section) else {
            return;
        };
        if !self.nav_focused(window, cx) {
            return;
        }
        let next = (at as isize + step).clamp(0, all.len() as isize - 1) as usize;
        self.open_section(all[next].id, cx);
        // The focus goes along when a section's button has it, as the buttons are next to each other.
        let on_list = self
            .general
            .nav_focus
            .as_ref()
            .is_some_and(|f| f.is_focused(window));
        if next != at && !on_list {
            if step > 0 {
                window.focus_next(cx);
            } else {
                window.focus_prev(cx);
            }
        }
        cx.stop_propagation();
    }

    /// Tab and Shift+Tab go round the dialog's own controls, never on to the window behind it.
    fn cycle_focus(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        // Past its last control the window's order goes on behind it, so step until back inside.
        for _ in 0..1000 {
            if forward {
                window.focus_next(cx);
            } else {
                window.focus_prev(cx);
            }
            if self.focus.contains_focused(window, cx) {
                break;
            }
        }
    }

    fn nav_focused(&self, window: &Window, cx: &App) -> bool {
        self.general
            .nav_focus
            .as_ref()
            .is_some_and(|f| f.contains_focused(window, cx))
    }

    fn pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            "general" => general::render(self, cx),
            "shortcuts" => shortcuts::render(self, window, cx),
            "providers" => providers::render(self, window, cx),
            "tokens" => tokens::render(self, cx),
            "extensions" => extensions::render(self, window, cx),
            "mcp" => mcp::render(self, window, cx),
            "permissions" => permissions::render(self, window, cx),
            "projects" => projects::render(self, cx),
            "ssh" => ssh::render(self, cx),
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
                        .rounded(px(RADIUS_SM))
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
        div()
            .id("settings-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::hsla(0., 0., 0., 0.4))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Closed)),
            )
            .child(
                div()
                    .id("settings")
                    .role(Role::Dialog)
                    .aria_label("Settings")
                    .track_focus(&self.focus)
                    .key_context("Settings")
                    .on_key_down(cx.listener(Self::on_key))
                    .on_action(cx.listener(|d, _: &crate::FocusNext, window, cx| {
                        d.cycle_focus(true, window, cx)
                    }))
                    .on_action(cx.listener(|d, _: &crate::FocusPrev, window, cx| {
                        d.cycle_focus(false, window, cx)
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .flex()
                    .h(px(680.))
                    .max_h(viewport.height - px(48.))
                    .w(px(920.))
                    .max_w(viewport.width - px(48.))
                    .overflow_hidden()
                    .rounded(px(RADIUS_LG))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.bg_elevated)
                    .shadow(popup_shadow(&t))
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
                                                cx.emit(SettingsEvent::Closed)
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
