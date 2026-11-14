//! The Settings dialog's state and its sections.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;
use gpui::{
    App, Entity, EventEmitter, FocusHandle, Focusable, Pixels, Point, ScrollHandle, SharedString,
    Window, prelude::*, px,
};
use patchwork_server::Server;
use patchwork_server::settings::Settings;
use crate::ui::menu::Menu;
use super::controls::Field;
use super::extensions;
use super::general;
use super::mcp;
use super::permissions;
use super::projects;
use super::providers;
use super::scheduled;
use super::shortcuts;
use super::ssh;
use super::themes;
use super::tokens;
use super::updates;

/// What the dialog tells the workspace.
pub enum SettingsEvent {
    /// A setting was saved; the workspace reads them again.
    Changed,
    /// Threads were added or removed; the workspace lists them again.
    ThreadsChanged,
    /// The user asked to restart into the installed update.
    RestartToUpdate,
    /// Start a chat with this message, whose agent writes a theme.
    GenerateTheme(String),
    Closed,
}

/// One entry of the list: its id, its name, and its icon.
pub(super) struct Section {
    pub(super) id: &'static str,
    pub(super) label: &'static str,
    pub(super) icon: &'static str,
}

/// The panes, in groups under a heading, so the list reads in a few short runs.
pub(super) const GROUPS: &[(&str, &[Section])] = &[
    (
        "General",
        &[
            Section {
                id: "general",
                label: "General",
                icon: "gear-six",
            },
            Section {
                id: "themes",
                label: "Themes",
                icon: "palette",
            },
            Section {
                id: "chat",
                label: "Chat",
                icon: "chats",
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
                label: "Devices",
                icon: "devices",
            },
        ],
    ),
];

pub(super) fn sections() -> impl Iterator<Item = &'static Section> {
    GROUPS.iter().flat_map(|(_, sections)| sections.iter())
}

pub struct SettingsDialog {
    pub server: Arc<Server>,
    pub settings: Settings,
    pub(super) section: &'static str,
    pub(super) focus: FocusHandle,
    pub(super) scroll: ScrollHandle,
    /// The text fields drawn, by a name each pane gives them.
    pub(super) fields: HashMap<SharedString, Field>,
    /// The dropdown whose menu is open: its name, the menu, and where it opened.
    pub(super) menu: Option<(SharedString, Entity<Menu>, Point<Pixels>)>,
    /// Why the last save failed.
    pub(super) error: Option<String>,
    pub(super) general: general::GeneralState,
    pub(super) shortcuts: shortcuts::ShortcutsState,
    pub(super) providers: providers::ProvidersState,
    pub(super) extensions: extensions::ExtensionsState,
    pub(super) mcp: mcp::McpState,
    pub(super) permissions: permissions::PermissionsState,
    pub(super) projects: projects::ProjectsState,
    pub(super) ssh: ssh::SshState,
    pub(super) themes: themes::ThemesState,
    pub(super) scheduled: scheduled::ScheduledState,
    pub(super) tokens: tokens::TokensState,
    pub(super) updates: updates::UpdatesState,
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
            themes: themes::ThemesState::default(),
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
            Err(e) => self.error = Some(e.to_string()),
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

    pub(super) fn open_section(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.section = id;
        self.menu = None;
        // A theme written since the last look shows up.
        self.themes.loaded = false;
        self.shortcuts.recording = None;
        self.shortcuts.recording_sigil = None;
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }
}
