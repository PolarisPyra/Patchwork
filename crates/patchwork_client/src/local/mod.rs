//! The desktop's extension of the client store, over a `Core` in the same process.
//!
//! It holds the settings, the chats as the task list reads them, the projects, the agents, the
//! chat apps, the quilts and the open project's git. Built only with the `local` feature, so the
//! phone never links the core.
//!
//! [`core`] is the one way the desktop's screens reach the core's operations and types; no
//! screen crate depends on the core itself.

// Rust guideline compliant 2026-10-09

mod agents;
mod chat_apps;
mod chats;
mod errors;
mod git;
mod projects;
mod quilts;
mod settings;

use std::sync::Arc;

use gpui::{App, BorrowAppContext as _, Global};
use patchwork_core::Core;
use patchwork_model::chats::Chat;

pub use agents::AgentsStore;
pub use chat_apps::ChatAppsStore;
pub use git::{GitModel, ReplyFiles};
pub use projects::ProjectsStore;
pub use quilts::QuiltsStore;
pub use settings::SettingsStore;

/// The core's operations and types, as the desktop's screens call them.
pub mod core {
    pub use patchwork_core::*;
}

/// The server phones and browsers connect to, whose methods [`server::CoreServer`] adds to the
/// core.
pub mod server {
    pub use patchwork_server::*;
}

/// The chat apps the core talks through, whose methods [`gateway::CoreControl`] and
/// [`gateway::CoreHandoff`] add to the core.
pub mod gateway {
    pub use patchwork_gateway::*;
}

/// The state every desktop screen reads, kept as a GPUI global.
///
/// The screens draw from this and never keep their own copies. Each store is read again from the
/// core when it changes, and the screens that observe [`AppState`] draw again. Read it with
/// [`AppState::get`], and change it with [`AppState::update`].
#[derive(Clone)]
pub struct AppState {
    pub(crate) core: Arc<Core>,
    /// The app's settings, as last saved.
    pub settings: SettingsStore,
    /// Every chat, each with only the messages the task list reads.
    pub chats: Arc<Vec<Arc<Chat>>>,
    /// The projects' folders.
    pub projects: ProjectsStore,
    /// Every agent, oldest first.
    pub agents: AgentsStore,
    /// The chat apps on each chat, and where each connection stands.
    pub chat_apps: ChatAppsStore,
    /// Every quilt, so a chat's quilt cards can name theirs and open it.
    pub quilts: QuiltsStore,
    /// The last error an action hit, shown above the status bar until dismissed.
    pub error: Option<String>,
}

impl Global for AppState {}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("chats", &self.chats.len())
            .field("folders", &self.projects.len())
            .field("agents", &self.agents.len())
            .field("quilts", &self.quilts.len())
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl AppState {
    /// Reads everything from `core`. The quilts are read later, off the UI thread.
    #[must_use]
    pub fn new(core: Arc<Core>) -> Self {
        Self {
            settings: SettingsStore::read(&core),
            chats: Arc::new(core.list_chats()),
            projects: ProjectsStore::read(&core),
            agents: AgentsStore::read(&core),
            chat_apps: ChatAppsStore::read(&core),
            quilts: QuiltsStore::default(),
            error: None,
            core,
        }
    }

    /// The app's state.
    ///
    /// # Panics
    ///
    /// When the app has not set it yet, which it does before it opens a window.
    #[must_use]
    pub fn get(cx: &App) -> &Self {
        cx.global::<Self>()
    }

    /// Changes the state with `change`, then draws again every view that observes it.
    pub fn update(cx: &mut App, change: impl FnOnce(&mut Self)) {
        cx.update_global::<Self, _>(|state, _| change(state));
    }

    /// The core the state is read from.
    #[must_use]
    pub fn core(&self) -> &Arc<Core> {
        &self.core
    }
}

/// Sets up the desktop's store over `core`; call once at startup, before any window opens.
pub fn init(core: Arc<Core>, cx: &mut App) {
    cx.set_global(AppState::new(core));
    GitModel::init(cx);
}
