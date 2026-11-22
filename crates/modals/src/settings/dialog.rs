//! The Settings dialog's state, its list and the panes each entry holds.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use gpui::{
    App, Entity, EventEmitter, FocusHandle, Focusable, Pixels, Point, ScrollHandle, SharedString,
    Window, prelude::*, px,
};
use patchwork_core::Core;
use patchwork_core::settings::Settings;
use patchwork_ui::controls::menu::Menu;
use patchwork_ui::text::text_area::TextArea;
use super::controls::Field;
use super::panes::extensions;
use super::panes::gateway;
use super::panes::general;
use super::panes::mcp;
use super::panes::permissions;
use super::panes::projects;
use super::panes::providers;
use super::panes::scheduled;
use super::panes::security;
use super::panes::shortcuts;
use super::panes::ssh;
use super::panes::themes;
use super::panes::costs;
use super::panes::tokens;
use super::panes::updates;
use super::panes::worktrees;

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
    GenerateTheme(String, Option<patchwork_core::chats::AgentPick>),
    /// Close Settings and open this chat.
    OpenChat(String),
    Closed,
}

/// One pane: its id, and its name and icon, which are its row under an entry that holds more
/// than one.
pub(super) struct Pane {
    pub(super) id: &'static str,
    pub(super) label: &'static str,
    pub(super) icon: &'static str,
}

/// One entry of the list: its name, its icon and the panes it holds.
pub(super) struct Entry {
    pub(super) label: &'static str,
    pub(super) icon: &'static str,
    pub(super) panes: &'static [Pane],
}

/// The list. Panes on one subject share an entry, so the list stays short.
pub(super) const ENTRIES: &[Entry] = &[
    Entry {
        label: "General",
        icon: "gear-six",
        panes: &[
            Pane {
                id: "general",
                label: "General",
                icon: "gear-six",
            },
            Pane {
                id: "notifications",
                label: "Notifications",
                icon: "bell",
            },
            Pane {
                id: "updates",
                label: "Updates",
                icon: "arrow-circle-up",
            },
        ],
    },
    Entry {
        label: "Appearance",
        icon: "palette",
        panes: &[
            Pane {
                id: "themes",
                label: "Window theme",
                icon: "palette",
            },
            Pane {
                id: "code-theme",
                label: "Syntax theme",
                icon: "code",
            },
            Pane {
                id: "orchestrator-background",
                label: "Orchestrator theme",
                icon: "robot",
            },
            Pane {
                id: "chat-background",
                label: "Chat theme",
                icon: "chat-circle",
            },
            Pane {
                id: "quilts-background",
                label: "Quilts theme",
                icon: "code-block",
            },
            Pane {
                id: "interface",
                label: "Interface",
                icon: "desktop",
            },
        ],
    },
    Entry {
        label: "Shortcuts",
        icon: "keyboard",
        panes: &[Pane {
            id: "shortcuts",
            label: "Shortcuts",
            icon: "keyboard",
        }],
    },
    Entry {
        label: "Providers",
        icon: "sparkle",
        panes: &[
            Pane {
                id: "providers",
                label: "Providers",
                icon: "sparkle",
            },
            Pane {
                id: "tokens",
                label: "Usage",
                icon: "chart-bar",
            },
        ],
    },
    Entry {
        label: "Costs",
        icon: "gauge",
        panes: &[Pane {
            id: "costs",
            label: "Costs",
            icon: "gauge",
        }],
    },
    Entry {
        label: "Extensions",
        icon: "puzzle-piece",
        panes: &[
            Pane {
                id: "extensions",
                label: "Extensions",
                icon: "puzzle-piece",
            },
            Pane {
                id: "mcp",
                label: "MCP Servers",
                icon: "plugs-connected",
            },
        ],
    },
    Entry {
        label: "Gateway",
        icon: "chats",
        panes: &[Pane {
            id: "gateway",
            label: "Gateway",
            icon: "chats",
        }],
    },
    Entry {
        label: "Permissions",
        icon: "shield-check",
        panes: &[Pane {
            id: "permissions",
            label: "Permissions",
            icon: "shield-check",
        }],
    },
    Entry {
        label: "Security",
        icon: "fingerprint",
        panes: &[Pane {
            id: "security",
            label: "Security",
            icon: "fingerprint",
        }],
    },
    Entry {
        label: "Projects",
        icon: "folder-simple",
        panes: &[
            Pane {
                id: "projects",
                label: "Projects",
                icon: "folder-simple",
            },
            Pane {
                id: "scheduled",
                label: "Scheduled",
                icon: "clock",
            },
            Pane {
                id: "worktrees",
                label: "Worktrees",
                icon: "folders",
            },
        ],
    },
    Entry {
        label: "Terminal",
        icon: "terminal-window",
        panes: &[Pane {
            id: "terminal",
            label: "Terminal",
            icon: "terminal-window",
        }],
    },
    Entry {
        label: "Devices",
        icon: "devices",
        panes: &[Pane {
            id: "ssh",
            label: "Devices",
            icon: "devices",
        }],
    },
];

/// The entry that holds the pane `id`, with its place in the list and the pane.
pub(super) fn entry_of(id: &str) -> Option<(usize, &'static Entry, &'static Pane)> {
    ENTRIES.iter().enumerate().find_map(|(at, entry)| {
        let pane = entry.panes.iter().find(|p| p.id == id)?;
        Some((at, entry, pane))
    })
}

pub struct SettingsDialog {
    pub core: Arc<Core>,
    pub settings: Settings,
    /// The pane showing, by id.
    pub(super) section: &'static str,
    /// The pane each entry last showed, by the entry's name, so it opens there.
    pub(super) last: HashMap<&'static str, &'static str>,
    pub(super) focus: FocusHandle,
    /// The list's search field, made when the list is first drawn.
    pub(super) search: Option<Entity<TextArea>>,
    /// The search result the arrow keys have highlighted.
    pub(super) search_at: usize,
    /// The entries showing their panes, by their place in the list. One stays open until it is
    /// clicked shut.
    pub(super) expanded: HashSet<usize>,
    /// The entry whose panes are sliding open.
    pub(super) opening: Option<usize>,
    /// The entry whose panes are sliding shut, by its place in the list.
    pub(super) closing: Option<usize>,
    /// The focus of each pane's row in the list, by the pane's id.
    pub(super) rows: HashMap<&'static str, FocusHandle>,
    pub(super) scroll: ScrollHandle,
    /// The text fields drawn, by a name each pane gives them.
    pub(super) fields: HashMap<SharedString, Field>,
    /// The dropdown whose menu is open: its name, the menu, and where it opened.
    pub(super) menu: Option<(SharedString, Entity<Menu>, Point<Pixels>)>,
    /// Why the last save failed.
    pub(super) error: Option<String>,
    pub(super) general: general::GeneralState,
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
    /// Opens on the pane `section`, by id; None opens on General.
    pub fn new(
        core: Arc<Core>,
        section: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let section = section
            .and_then(entry_of)
            .map_or("general", |(_, _, pane)| pane.id);
        Self {
            settings: core.settings(),
            core,
            section,
            last: HashMap::new(),
            focus,
            search: None,
            search_at: 0,
            rows: HashMap::new(),
            expanded: entry_of(section)
                .filter(|(_, entry, _)| entry.panes.len() > 1)
                .map(|(at, _, _)| at)
                .into_iter()
                .collect(),
            opening: None,
            closing: None,
            scroll: ScrollHandle::new(),
            fields: HashMap::new(),
            menu: None,
            error: None,
            general: general::GeneralState::default(),
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
        }
    }

    /// Saves the settings as `change` leaves them.
    pub fn update(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let mut next = self.core.settings();
        change(&mut next);
        match self.core.save_settings(next) {
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
        patchwork_ui::off_thread(cx, work, move |d, out, cx| {
            done(d, out, cx);
            cx.notify();
        });
    }

    /// Opens the entry at `at` of the list, on the pane it last showed.
    pub(super) fn open_entry(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(entry) = ENTRIES.get(at) else {
            return;
        };
        let pane = self.last.get(entry.label).copied();
        if let Some(id) = pane.or_else(|| entry.panes.first().map(|p| p.id)) {
            self.open_section(id, cx);
        }
    }

    pub(super) fn open_section(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if let Some((at, entry, _)) = entry_of(id) {
            self.last.insert(entry.label, id);
            self.expand(at);
        }
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
