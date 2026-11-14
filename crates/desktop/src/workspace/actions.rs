//! What the window's shortcuts and menus do: search, settings, rules, and the theme.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use futures::StreamExt;
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
                this.backgrounds = crate::ui::backdrop::Backgrounds::default();
                this.sync_panel(cx);
                this.apply_theme(window, cx);
            }
            crate::settings::SettingsEvent::ThreadsChanged => this.reload(cx),
            crate::settings::SettingsEvent::GenerateTheme(prompt) => {
                this.generate_theme(prompt.clone(), cx);
            }
            crate::settings::SettingsEvent::RestartToUpdate => {
                super::updates::restart_to_update(&this.server, window, cx);
            }
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

    /// Light when the window is dark, else dark. A picked theme gives way to it.
    pub(super) fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use patchwork_server::settings::Theme as ThemeSetting;
        let setting = if theme(cx).is_dark() {
            ThemeSetting::Light
        } else {
            ThemeSetting::Dark
        };
        self.settings.theme = setting;
        self.settings.theme_id = None;
        let saved = self.server.store.update_settings(|s| {
            s.theme = setting;
            s.theme_id = None;
        });
        if let Err(e) = saved {
            log::warn!(error:% = e; "could not save the theme: {{error}}");
        }
        self.apply_theme(window, cx);
    }

    /// Starts a chat with `prompt`, which has its agent write a theme. Settings stays open and
    /// is told when the chat could not be started.
    fn generate_theme(&mut self, prompt: String, cx: &mut Context<Self>) {
        use patchwork_server::orchestration::Target;
        let new = self.composer.read(cx).new_thread(None, &self.settings);
        let server = Arc::clone(&self.server);
        let sent = cx
            .background_spawn(async move { server.dispatch_to(&prompt, &Target::New(None), &new) });
        cx.spawn(async move |this, cx| {
            let sent = sent.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = sent {
                    log::warn!(error:% = e; "could not start the chat for a theme: {{error}}");
                    if let Some(dialog) = &this.settings_dialog {
                        dialog.update(cx, |d, cx| d.theme_failed(e.to_string(), cx));
                    }
                }
                this.reload(cx);
            });
        })
        .detach();
    }

    /// Draws the app again whenever a file in the theme folders changes, and has the open
    /// settings list what can be picked again.
    pub(super) fn watch_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (sender, mut received) = futures::channel::mpsc::unbounded::<()>();
        let server = Arc::clone(&self.server);
        cx.background_spawn(async move {
            let watched = server.watch_themes(move || {
                let _ = sender.unbounded_send(());
            });
            if let Err(e) = watched {
                log::warn!(error:% = e; "couldn't watch the theme folders: {{error}}");
            }
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            while received.next().await.is_some() {
                let applied = this.update_in(cx, |this, window, cx| {
                    this.apply_theme(window, cx);
                    if let Some(dialog) = &this.settings_dialog {
                        dialog.update(cx, crate::settings::SettingsDialog::themes_changed);
                    }
                });
                if applied.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Draws the app in what the theme settings come to. The theme's file is read off the UI
    /// thread.
    pub(super) fn apply_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        cx.spawn_in(window, async move |this, cx| {
            let applied = cx
                .background_spawn(async move { server.applied_theme() })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let was = crate::ui::theme::applied(cx);
                let reset =
                    was.text_size != applied.text_size || was.chat_font != applied.chat_font;
                crate::ui::theme::apply(applied, window.appearance(), cx);
                if reset {
                    // Every row of the transcript is another height in another font or size.
                    this.transcript_list.remeasure();
                }
                cx.refresh_windows();
            });
        })
        .detach();
    }
}
