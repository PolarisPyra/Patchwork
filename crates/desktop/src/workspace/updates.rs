//! The update card in the window's top right, and the restart into an installed update.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{App, FontWeight, PromptLevel, Role, Window, deferred, div, prelude::*, px};
use patchwork_server::Server;
use patchwork_server::updates::{RELEASES_URL, RestartResult, UpdateState, restart_path};

use crate::ui::theme::{RADIUS, RADIUS_SM, Theme};
use crate::ui::{BtnKind, btn, button, icon, popup_shadow};
use super::state::Workspace;

/// Restarts into the installed update. Replies that are running resume after it; when Settings
/// says a launch never resumes them, the user confirms the restart first.
pub(super) fn restart_to_update(server: &Arc<Server>, window: &mut Window, cx: &mut App) {
    match server.restart_to_update(false) {
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
            let server = Arc::clone(server);
            cx.spawn(async move |cx| {
                if answer.await == Ok(0) && server.restart_to_update(true) == RestartResult::Restart
                {
                    cx.update(restart);
                }
            })
            .detach();
        }
    }
}

/// Quits and starts the copy the update installed.
fn restart(cx: &mut App) {
    if let Some(path) = restart_path() {
        cx.set_restart_path(path);
    }
    cx.restart();
}

impl Workspace {
    /// Redraws what shows where updating stands: the card here, and Settings while it is open.
    pub fn on_update_change(&mut self, cx: &mut Context<Self>) {
        if let Some(dialog) = &self.settings_dialog {
            dialog.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    /// A card once a new version is installed (restart to use it) or, for a copy that cannot
    /// install updates itself, out (download it). Each version's card shows until it is closed.
    pub(super) fn update_card(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let (version, ready) = match self.server.update_status().state {
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
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.bg_elevated)
                    .p(px(10.))
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
                                    restart_to_update(&this.server, window, cx);
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
                            .rounded(px(RADIUS_SM))
                            .text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .child(icon("x", 14.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Err(e) = this.server.dismiss_update(&version) {
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
