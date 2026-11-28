//! The dialogs and menus drawn over the window: what is open, and drawing them.

// Rust guideline compliant 2026-10-09

use gpui::{
    App, Entity, FocusHandle, Pixels, Point, StyleRefinement, Window, deferred, div, prelude::*,
};
use patchwork_desktop_dialogs::add_project::AddProject;
use patchwork_desktop_settings::background::BackgroundDialog;
use patchwork_desktop_dialogs::credential::CredentialDialog;
use patchwork_desktop_dialogs::granular::GranularDialog;
use patchwork_desktop_dialogs::interrupted::InterruptedCard;
use patchwork_desktop_search::Palette;
use patchwork_desktop_settings::settings::SettingsDialog;
use patchwork_design_components::menu::Menu;

use crate::bars::machine_menu_at;
use crate::wiring::background::Owner;

/// The dialogs and menus over the window, each while it is open.
///
/// The workspace opens and closes them, since what each does when it closes reaches into the
/// rest of the window; this view keeps them and draws them over everything.
pub(crate) struct Dialogs {
    /// Search, and what had focus before it opened.
    pub(crate) palette: Option<Entity<Palette>>,
    pub(crate) palette_return: Option<FocusHandle>,
    pub(crate) settings: Option<Entity<SettingsDialog>>,
    pub(crate) add_project: Option<Entity<AddProject>>,
    /// A chat's or an agent's background picker, with whose background it picks.
    pub(crate) background: Option<(Owner, Entity<BackgroundDialog>)>,
    pub(crate) granular: Option<Entity<GranularDialog>>,
    /// Git's sign-in questions, shown while a command waits on one.
    pub(crate) credentials: Entity<CredentialDialog>,
    /// How many replies the last session cut off wait to be resumed, with Settings set to ask.
    pub(crate) interrupted: Entity<InterruptedCard>,
    /// The toolbar's machine menu, and where the press that last closed it was.
    pub(crate) machine_menu: Option<Entity<Menu>>,
    pub(crate) machine_menu_dismissed: Option<Point<Pixels>>,
}

impl Dialogs {
    /// None open, with the dialogs that live as long as the window.
    pub(crate) fn new(
        credentials: Entity<CredentialDialog>,
        interrupted: Entity<InterruptedCard>,
    ) -> Self {
        Self {
            palette: None,
            palette_return: None,
            settings: None,
            add_project: None,
            background: None,
            granular: None,
            credentials,
            interrupted,
            machine_menu: None,
            machine_menu_dismissed: None,
        }
    }

    /// Whether a dialog or a menu is over the window. It keeps the keys to itself.
    pub(crate) fn covered(&self, cx: &App) -> bool {
        self.palette.is_some()
            || self.settings.is_some()
            || self.background.is_some()
            || self.granular.is_some()
            || self.credentials.read(cx).is_open()
            || self.add_project.is_some()
            || self.machine_menu.is_some()
    }
}

impl Render for Dialogs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let credentials = self.credentials.clone();
        let credential_dialog = credentials
            .read(cx)
            .is_open()
            .then(|| deferred(credentials).with_priority(4));
        // Laid over the whole window, as its own children were, and drawn after the rest of it.
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            // Cached, so a moving chat backdrop under them does not draw them again each frame.
            .children(
                self.palette
                    .clone()
                    .map(|p| deferred(p.cached(overlay())).with_priority(2)),
            )
            .children(
                self.settings
                    .clone()
                    .map(|d| deferred(d.cached(overlay())).with_priority(2)),
            )
            .children(
                self.add_project
                    .clone()
                    .map(|d| deferred(d).with_priority(2)),
            )
            .children(
                self.background
                    .as_ref()
                    .map(|(_, d)| deferred(d.clone()).with_priority(2)),
            )
            .children(self.granular.clone().map(|d| deferred(d).with_priority(2)))
            .children(credential_dialog)
            .children(
                self.machine_menu
                    .as_ref()
                    .map(|m| Menu::view(m, machine_menu_at())),
            )
    }
}

/// The window-filling place of a dialog drawn over everything.
fn overlay() -> StyleRefinement {
    StyleRefinement::default()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

impl crate::window::Workspace {
    /// Settings, while open.
    pub(crate) fn settings_dialog(&self, cx: &App) -> Option<Entity<SettingsDialog>> {
        self.dialogs.read(cx).settings.clone()
    }

    /// Changes what is open over the window, and draws it again.
    pub(crate) fn dialogs_mut<R>(&self, cx: &mut App, change: impl FnOnce(&mut Dialogs) -> R) -> R {
        self.dialogs.update(cx, |d, cx| {
            let changed = change(d);
            cx.notify();
            changed
        })
    }
}
