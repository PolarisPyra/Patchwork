//! What the window's shortcuts and menus do: search, settings, rules, and the theme.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use gpui::{Window, prelude::*};
use patchwork_server::permissions::PermissionScope;
use crate::workspace::palette::{Action, Closed, Palette};
use crate::ui::theme::theme;
use super::state::Workspace;

impl Workspace {
    /// Starts dragging the sidebar's edge from `x`.
    pub fn start_resize(&mut self, x: f32) {
        self.resizing = Some((x, self.sidebar_width));
    }

    /// Opens search, or closes it when it is open.
    pub fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            return self.close_palette(None, window, cx);
        }
        self.palette_return = window.focused(cx);
        let items = crate::workspace::palette::items(self);
        let palette = cx.new(|cx| Palette::new(items, window, cx));
        cx.subscribe_in(&palette, window, |this, _, closed: &Closed, window, cx| {
            this.close_palette(closed.0.clone(), window, cx);
        })
        .detach();
        self.palette = Some(palette);
        cx.notify();
    }

    /// Opens Settings → Permissions on `scope`, with `rule_id` picked out.
    pub fn open_rule(
        &mut self,
        scope: PermissionScope,
        rule_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings(Some("permissions"), window, cx);
        if let Some(dialog) = &self.settings_dialog {
            dialog.update(cx, |d, _| d.focus_rule(scope, rule_id));
        }
    }

    /// Opens Settings on the section `section`, by id.
    pub fn open_settings(
        &mut self,
        section: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.settings_dialog.is_some() {
            return;
        }
        let server = Arc::clone(&self.server);
        let dialog = cx.new(|cx| crate::settings::SettingsDialog::new(server, section, window, cx));
        cx.subscribe_in(&dialog, window, |this, _, event, window, cx| match event {
            crate::settings::SettingsEvent::Changed => {
                this.settings = this.server.store.settings();
                // "Run setup again" leaves Settings for the setup.
                if !this.settings.onboarded {
                    this.settings_dialog = None;
                }
                // A picture added or removed in Settings changes the ones a chat can show.
                this.backgrounds = crate::chat::backdrop::Backgrounds::default();
                this.sync_panel(cx);
                cx.set_global(crate::ui::theme::Theme::for_setting(
                    this.settings.theme,
                    window.appearance(),
                ));
                cx.refresh_windows();
            }
            crate::settings::SettingsEvent::ThreadsChanged => this.reload(cx),
            crate::settings::SettingsEvent::Closed => {
                this.settings_dialog = None;
                window.focus(&this.focus, cx);
                cx.notify();
            }
        })
        .detach();
        self.settings_dialog = Some(dialog);
        cx.notify();
    }

    pub(super) fn close_palette(
        &mut self,
        action: Option<Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette = None;
        match self.palette_return.take() {
            Some(focus) => window.focus(&focus, cx),
            None => window.focus(&self.focus, cx),
        }
        match action {
            Some(Action::NewChat) => self.new_thread_in(None, None, window, cx),
            Some(Action::NewTask(id)) => self.new_thread_in(Some(id), None, window, cx),
            Some(Action::NewWorktreeTask(id)) => self.new_task_in_worktree(id, window, cx),
            Some(Action::RunChecks(id)) => self.rerun_checks(id, cx),
            Some(Action::ChatBackground(id)) => self.open_background_dialog(&id, window, cx),
            Some(Action::ToggleTheme) => self.toggle_theme(window, cx),
            Some(Action::OpenSettings) => self.open_settings(None, window, cx),
            Some(Action::OpenFolder) => self.open_folder(window, cx),
            Some(Action::OpenThread(id)) => self.reveal_thread(&id, cx),
            Some(Action::OpenProject(id)) => self.select_project(&id, cx),
            None => {}
        }
        cx.notify();
    }

    /// Light when the window shows a dark theme, else dark.
    pub(super) fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use patchwork_server::settings::Theme as ThemeSetting;
        let light = theme(cx).bg_app == crate::ui::theme::Theme::light().bg_app;
        let setting = if light {
            ThemeSetting::Dark
        } else {
            ThemeSetting::Light
        };
        self.settings.theme = setting;
        if let Err(e) = self.server.store.update_settings(|s| s.theme = setting) {
            log::warn!(error:% = e; "could not save the theme: {{error}}");
        }
        cx.set_global(crate::ui::theme::Theme::for_setting(
            setting,
            window.appearance(),
        ));
        cx.refresh_windows();
    }
}
