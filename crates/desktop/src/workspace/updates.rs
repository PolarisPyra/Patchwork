//! The update card in the window's top right, the restart into an installed update, and the
//! desktop notice of a newer agent CLI.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::Duration;

use gpui::{App, FontWeight, PromptLevel, Role, Window, deferred, div, prelude::*, px};
use patchwork_core::Core;
use patchwork_core::updates::{RELEASES_URL, RestartResult, UpdateState, restart_path};

use patchwork_ui::theme::{radius, radius_sm, Theme};
use patchwork_ui::{BtnKind, agent_label, btn, button, icon, popup_shadow};
use super::window::Workspace;

/// The tag before a CLI update notice's agent, which a click on it recognizes.
pub(crate) const CLI_UPDATE_TAG: &str = "cli-update";
/// How long after launch agent CLIs are first checked, so the check never slows startup.
const CLI_FIRST_CHECK: Duration = Duration::from_secs(60);
/// How often agent CLIs are checked for newer releases.
const CLI_CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

/// Restarts into the installed update. Replies that are running resume after it; when Settings
/// says a launch never resumes them, the user confirms the restart first.
pub(crate) fn restart_to_update(core: &Arc<Core>, window: &mut Window, cx: &mut App) {
    match core.restart_to_update(false) {
        RestartResult::NothingReady => {}
        RestartResult::Restart => restart(cx),
        RestartResult::Busy { running } => {
            let tasks = if running == 1 {
                "1 task is".to_string()
            } else {
                format!("{running} tasks are")
            };
            let answer = window.prompt(
                PromptLevel::Warning,
                "Restart to update",
                Some(&format!(
                    "{tasks} still running. Restarting interrupts them, and Settings is set not to resume them."
                )),
                &["Restart", "Cancel"],
                cx,
            );
            let core = Arc::clone(core);
            cx.spawn(async move |cx| {
                if answer.await == Ok(0) && core.restart_to_update(true) == RestartResult::Restart {
                    cx.update(restart);
                }
            })
            .detach();
        }
    }
}

/// Quits and starts the copy the update installed.
pub(crate) fn restart(cx: &mut App) {
    if let Some(path) = restart_path() {
        cx.set_restart_path(path);
    }
    cx.restart();
}

impl Workspace {
    /// Checks the connected agent CLIs a minute after launch and then hourly, and shows a desktop
    /// notice for each newer release not announced before. Stops with the window.
    pub(crate) fn watch_cli_updates(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        cx.spawn(async move |this, cx| {
            let mut wait = CLI_FIRST_CHECK;
            loop {
                cx.background_executor().timer(wait).await;
                wait = CLI_CHECK_EVERY;
                let checking = Arc::clone(&core);
                let found = cx
                    .background_executor()
                    .spawn(async move { checking.unannounced_cli_updates() })
                    .await;
                let shown = this.update(cx, |_, cx| {
                    for (kind, update) in found {
                        let label = agent_label(kind);
                        cx.show_system_notification(gpui::SystemNotification {
                            tag: format!("{CLI_UPDATE_TAG}|{kind:?}").into(),
                            title: format!("{label} {} is available", update.latest).into(),
                            body: format!(
                                "You have {}. Update it in Settings → Providers.",
                                update.installed
                            )
                            .into(),
                            actions: Vec::new(),
                        });
                    }
                });
                if shown.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    /// Redraws what shows where updating stands: the card here, and Settings while it is open.
    pub fn on_update_change(&mut self, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.settings_dialog {
            dialog.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    /// A card once a new version is installed (restart to use it) or, for a copy that cannot
    /// install updates itself, out (download it). Each version's card shows until it is closed.
    pub(crate) fn update_card(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let (version, ready) = match self.core.update_status().state {
            UpdateState::Ready { version } => (version, true),
            UpdateState::Available { version } => (version, false),
            UpdateState::Idle
            | UpdateState::Checking
            | UpdateState::UpToDate
            | UpdateState::Downloading { .. }
            | UpdateState::Failed { .. } => return None,
        };
        if self.update_dismissed.as_deref() == Some(version.as_str()) {
            return None;
        }
        let label = format!(
            "Patchwork {version} {}",
            if ready { "is ready" } else { "is out" }
        );
        let action = if ready { "Restart" } else { "Download" };
        let (hover, fg) = (t.bg_hover, t.text);
        Some(
            deferred(
                div()
                    .id("update-card")
                    .role(Role::Status)
                    .aria_label(label.clone())
                    // It floats over the chat's own buttons, which a click on it must not reach.
                    .occlude()
                    .absolute()
                    .top(px(48.))
                    .right(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(radius()))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.bg_elevated)
                    .pl(px(12.))
                    .pr(px(8.))
                    .py(px(8.))
                    .shadow(popup_shadow(t))
                    .child(
                        div()
                            .flex_none()
                            .text_color(t.accent)
                            .child(icon("arrow-circle-up", 18.)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label),
                    )
                    .child(
                        btn("update-card-action", action, BtnKind::Primary, false, t)
                            .child(action)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if ready {
                                    restart_to_update(&this.core, window, cx);
                                } else {
                                    cx.open_url(RELEASES_URL);
                                }
                            })),
                    )
                    .child(
                        button("dismiss-update", "Dismiss")
                            .size(px(28.))
                            .flex_none()
                            .justify_center()
                            .rounded(px(radius_sm()))
                            .text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .child(icon("x", 14.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Err(e) = this.core.dismiss_update(&version) {
                                    log::warn!(error:% = e; "could not save the closed update card: {{error}}");
                                }
                                this.update_dismissed = Some(version.clone());
                                cx.notify();
                            })),
                    ),
            )
            .with_priority(1),
        )
    }
}
