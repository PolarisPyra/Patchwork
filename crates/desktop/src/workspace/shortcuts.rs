//! What the window's shortcuts and menus do: search, settings, rules, and the theme.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use gpui::{Window, prelude::*};
use patchwork_core::permissions::PermissionScope;
use patchwork_modals::search::{Action, Closed, Palette};
use patchwork_ui::theme::theme;
use super::window::Workspace;

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
        let items = crate::workspace::hosts::search::items(self);
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
        let core = Arc::clone(&self.core);
        let dialog =
            cx.new(|cx| patchwork_modals::settings::SettingsDialog::new(core, section, window, cx));
        let open = self.chat.as_ref().map(|t| t.id.clone());
        dialog.update(cx, |d, _| d.set_open_chat(open));
        cx.subscribe_in(&dialog, window, |this, _, event, window, cx| match event {
            patchwork_modals::settings::SettingsEvent::Changed => {
                this.settings = this.core.settings();
                // "Run setup again" leaves Settings for the setup.
                if !this.settings.onboarded {
                    this.settings_dialog = None;
                }
                // A picture added or removed in Settings changes the ones a chat can show.
                this.backgrounds.reload();
                this.agent_pictures_changed(cx);
                this.sync_panel(cx);
                this.apply_theme(window, cx);
            }
            patchwork_modals::settings::SettingsEvent::ChatsChanged => this.reload(cx),
            patchwork_modals::settings::SettingsEvent::GenerateTheme(prompt, pick) => {
                this.generate_theme(prompt.clone(), pick.as_ref(), cx);
            }
            patchwork_modals::settings::SettingsEvent::OpenChat(id) => {
                this.settings_dialog = None;
                this.open_chat(id, cx);
                window.focus(&this.focus, cx);
                cx.notify();
            }
            patchwork_modals::settings::SettingsEvent::RestartToUpdate => {
                super::updates::restart_to_update(&this.core, window, cx);
            }
            patchwork_modals::settings::SettingsEvent::Closed => {
                this.settings_dialog = None;
                window.focus(&this.focus, cx);
                cx.notify();
            }
        })
        .detach();
        self.settings_dialog = Some(dialog);
        cx.notify();
    }

    pub(crate) fn close_palette(
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
            Some(Action::NewChat) => self.new_chat_in(None, None, window, cx),
            Some(Action::NewTask(id)) => self.new_chat_in(Some(id), None, window, cx),
            Some(Action::NewWorktreeTask(id)) => self.new_task_in_worktree(id, window, cx),
            Some(Action::RunChecks(id)) => self.rerun_checks(id, cx),
            Some(Action::ChatBackground(id)) => self.open_background_dialog(&id, window, cx),
            Some(Action::ToggleTheme) => self.toggle_theme(window, cx),
            Some(Action::OpenSettings) => self.open_settings(None, window, cx),
            Some(Action::OpenFolder) => self.open_folder(window, cx),
            Some(Action::OpenChat(id)) => self.reveal_chat(&id, cx),
            Some(Action::OpenProject(id)) => self.select_project(&id, cx),
            None => {}
        }
        cx.notify();
    }

    /// Light when the window is dark, else dark. A picked theme gives way to it.
    pub(crate) fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use patchwork_core::settings::Theme as ThemeSetting;
        let setting = if theme(cx).is_dark() {
            ThemeSetting::Light
        } else {
            ThemeSetting::Dark
        };
        self.settings.theme = setting;
        self.settings.theme_id = None;
        let saved = self.core.update_settings(|s| {
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
    fn generate_theme(
        &mut self,
        prompt: String,
        pick: Option<&patchwork_core::chats::AgentPick>,
        cx: &mut Context<Self>,
    ) {
        use patchwork_core::orchestration::Target;
        let new = self.composer.read(cx).new_chat(None, &self.settings);
        let mut new = match pick {
            Some(pick) => new.picked(pick, &self.settings),
            None => new,
        };
        // The effort is the one picked in Settings, never the one left in the composer.
        new.effort = pick.and_then(|pick| pick.effort.clone());
        let core = Arc::clone(&self.core);
        let sent =
            cx.background_spawn(
                async move { core.dispatch_to(&prompt, &Target::New(None), &new, &[]) },
            );
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
    pub(crate) fn watch_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (sender, mut received) = futures::channel::mpsc::unbounded::<()>();
        let core = Arc::clone(&self.core);
        cx.background_spawn(async move {
            let watched = core.watch_themes(move || {
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
                        dialog.update(
                            cx,
                            patchwork_modals::settings::SettingsDialog::themes_changed,
                        );
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
    pub(crate) fn apply_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        cx.spawn_in(window, async move |this, cx| {
            let applied = cx
                .background_spawn(async move { core.applied_theme() })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let was = patchwork_ui::theme::applied(cx);
                let reset = was.text_size != applied.text_size || was.ui_font != applied.ui_font;
                patchwork_ui::theme::apply(applied, window.appearance(), cx);
                if reset {
                    // Every row of the transcript is another height in another font or size.
                    this.transcript_list.remeasure();
                }
                cx.refresh_windows();
                this.await_theme_switch(window, cx);
            });
        })
        .detach();
    }

    /// Draws the theme anew when the clock or the sun next changes it. The wait is cut into
    /// short ones, so a computer that slept still switches soon after it wakes.
    pub(crate) fn await_theme_switch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(wait) = patchwork_ui::theme::applied(cx).switch_in else {
            self.theme_switch = None;
            return;
        };
        let wait = wait.min(Duration::from_secs(300)) + Duration::from_secs(1);
        self.theme_switch = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update_in(cx, Self::apply_theme);
        }));
    }

    /// Draws the app in the open project's look once another project is the open one.
    pub(crate) fn follow_project_look(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.themed_project == self.settings.active_project_id {
            return;
        }
        self.themed_project
            .clone_from(&self.settings.active_project_id);
        if !self.settings.project_looks.is_empty() {
            self.apply_theme(window, cx);
        }
    }
}
