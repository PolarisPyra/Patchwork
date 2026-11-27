//! The Settings dialog's state: the page showing, the search, and each page's own state.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::Arc;
use gpui::{
    App, Entity, EventEmitter, FocusHandle, Focusable, Pixels, Point, ScrollHandle, SharedString,
    Window, prelude::*, px,
};
use patchwork_backend::Core;
use patchwork_backend::settings::Settings;
use patchwork_ui_components::menu::Menu;
use patchwork_ui_editor::TextArea;
use patchwork_ui_theme::theme;
use super::controls::{Field, center_when_drawn, seek};
use super::pages::resolve;
use super::devices;
use super::extensions;
use super::gateway;
use super::general;
use super::logs;
use super::mcp;
use super::permissions;
use super::projects;
use super::providers;
use super::scheduled;
use super::security;
use super::shortcuts;
use super::ssh;
use super::themes;
use super::costs;
use super::tokens;
use super::updates;
use super::worktrees;

/// What the dialog tells the workspace.
#[derive(Debug)]
pub enum SettingsEvent {
    /// A setting was saved; the workspace reads them again.
    Changed,
    /// Chats were added or removed; the workspace lists them again.
    ChatsChanged,
    /// The user asked to restart into the installed update.
    RestartToUpdate,
    /// Start a chat with this message, whose agent writes a theme, on this agent, model and
    /// effort when they were picked.
    GenerateTheme(String, Option<patchwork_backend::chats::AgentPick>),
    /// Close Settings and open this chat.
    OpenChat(String),
    Closed,
}

pub struct SettingsDialog {
    pub core: Arc<Core>,
    pub settings: Settings,
    /// The page showing, by id.
    pub(super) section: &'static str,
    pub(super) focus: FocusHandle,
    /// The list's search field, made when the list is first drawn.
    pub(super) search: Option<Entity<TextArea>>,
    /// The search result the arrow keys have highlighted.
    pub(super) search_at: usize,
    /// The focus of each page's row in the list, by the page's id.
    pub(super) rows: HashMap<&'static str, FocusHandle>,
    pub(super) scroll: ScrollHandle,
    /// The text fields drawn, by a name each pane gives them.
    pub(super) fields: HashMap<SharedString, Field>,
    /// The dropdown whose menu is open: its name, the menu, and where it opened.
    pub(super) menu: Option<(SharedString, Entity<Menu>, Point<Pixels>)>,
    /// Why the last save failed.
    pub(super) error: Option<String>,
    pub(super) general: general::GeneralState,
    pub(super) logs: logs::LogsState,
    pub(super) worktrees: worktrees::WorktreesState,
    pub(super) shortcuts: shortcuts::ShortcutsState,
    pub(super) providers: providers::ProvidersState,
    pub(super) extensions: extensions::ExtensionsState,
    pub(super) gateway: gateway::GatewayState,
    pub(super) mcp: mcp::McpState,
    pub(super) permissions: permissions::PermissionsState,
    pub(super) projects: projects::ProjectsState,
    pub(super) ssh: ssh::SshState,
    pub(super) themes: themes::ThemesState,
    pub(super) scheduled: scheduled::ScheduledState,
    pub(super) security: security::SecurityState,
    pub(super) tokens: tokens::TokensState,
    pub(super) costs: costs::CostsState,
    pub(super) updates: updates::UpdatesState,
    pub(super) devices: devices::DevicesState,
}

impl std::fmt::Debug for SettingsDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsDialog")
            .field("section", &self.section)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl EventEmitter<SettingsEvent> for SettingsDialog {}

impl Focusable for SettingsDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SettingsDialog {
    /// Opens on the page `section`, by its id or the id of a pane from before the pages were
    /// merged, scrolled to the section that pane became; None opens on General.
    pub fn new(
        core: Arc<Core>,
        section: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let (page, jump) = section
            .and_then(resolve)
            .map_or(("general", None), |(page, jump)| (page.id, jump));
        if let Some(name) = jump {
            seek(Some((name, theme(cx).bg_accent_soft)));
        }
        let mut dialog = Self {
            settings: core.settings(),
            core,
            section: page,
            focus,
            search: None,
            search_at: 0,
            rows: HashMap::new(),
            scroll: ScrollHandle::new(),
            fields: HashMap::new(),
            menu: None,
            error: None,
            general: general::GeneralState::default(),
            logs: logs::LogsState::default(),
            worktrees: worktrees::WorktreesState::default(),
            shortcuts: shortcuts::ShortcutsState::default(),
            providers: providers::ProvidersState::default(),
            extensions: extensions::ExtensionsState::default(),
            gateway: gateway::GatewayState::default(),
            mcp: mcp::McpState::default(),
            permissions: permissions::PermissionsState::default(),
            projects: projects::ProjectsState::default(),
            ssh: ssh::SshState::default(),
            themes: themes::ThemesState::default(),
            scheduled: scheduled::ScheduledState::default(),
            security: security::SecurityState::default(),
            tokens: tokens::TokensState::default(),
            costs: costs::CostsState::default(),
            updates: updates::UpdatesState::default(),
            devices: devices::DevicesState::default(),
        };
        if jump.is_some() {
            center_when_drawn(dialog.scroll.clone(), window);
        }
        dialog.watch_keys(window, cx);
        dialog
    }

    /// Saves the settings as `change` leaves them.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let mut next = self.core.settings();
        change(&mut next);
        match self.core.update_settings(|s| *s = next) {
            Ok(()) => {
                self.error = None;
                self.settings = self.core.settings();
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
        patchwork_ui_theme::off_thread(cx, work, move |d, out, cx| {
            done(d, out, cx);
            cx.notify();
        });
    }

    /// The id of the page showing.
    #[must_use]
    pub fn page(&self) -> &'static str {
        self.section
    }

    /// How far the page showing is scrolled.
    #[must_use]
    pub fn scroll_offset(&self) -> Point<Pixels> {
        self.scroll.offset()
    }

    /// Scrolls the page showing to `offset`.
    pub fn set_scroll_offset(&mut self, offset: Point<Pixels>, cx: &mut Context<Self>) {
        self.scroll.set_offset(offset);
        cx.notify();
    }

    /// Opens the page `id`, at its top.
    pub(super) fn open_section(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.section = id;
        self.menu = None;
        // A theme written since the last look shows up.
        self.themes.loaded = false;
        // Costs add up again, counting replies since the last look.
        self.costs.asked = false;
        self.shortcuts.recording = None;
        self.shortcuts.recording_sigil = None;
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }
}
