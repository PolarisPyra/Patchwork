//! Antigravity ACP sign-in (`agy acp --login`) and sign-out (`logout`) controls in Settings.

// Rust guideline compliant 2026-11-22

use std::sync::Arc;

use gpui::{Div, Focusable, Role, Window, div, prelude::*, px};
use patchwork_client::local::core::provider::{AgentKind, antigravity};
use patchwork_design_theme::{fs_sm, theme};
use patchwork_design_components::{BtnKind, btn, spinner};

use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert};

/// The text field where the user pastes the Google sign-in code or callback URL.
const AUTH_CODE_FIELD: &str = "antigravity-auth-code";

/// Lifecycle of an Antigravity ACP sign-in or sign-out flow.
pub(super) enum AntigravityAuth {
    /// Waiting for `agy acp --login` to emit the Google OAuth URL.
    Starting,
    /// Browser sign-in link opened; waiting for the user to paste the code or callback URL.
    WaitingCode {
        session: Arc<antigravity::LoginSession>,
        url: String,
        code: String,
        error: Option<String>,
    },
    /// Code submitted; waiting for `agy acp --login` to store credentials.
    Verifying {
        session: Arc<antigravity::LoginSession>,
    },
    /// Running the official ACP `logout` RPC.
    SigningOut,
    /// The last sign-in or sign-out attempt failed.
    Failed(String),
}

impl SettingsDialog {
    /// Starts `agy acp --login` off-thread, opens the Google sign-in URL, and shows the code box.
    pub(super) fn start_antigravity_login(&mut self, cx: &mut Context<Self>) {
        self.drop_fields(AUTH_CODE_FIELD);
        self.providers.antigravity_auth = Some(AntigravityAuth::Starting);
        self.run(
            antigravity::start_login,
            |d, started, cx| {
                // Ignore if the user already cancelled while `agy acp --login` was starting.
                if !matches!(
                    d.providers.antigravity_auth,
                    Some(AntigravityAuth::Starting)
                ) {
                    if let Ok(session) = started {
                        session.cancel();
                    }
                    return;
                }
                match started {
                    Ok(session) => {
                        let url = session.url().to_string();
                        cx.open_url(&url);
                        d.providers.antigravity_auth = Some(AntigravityAuth::WaitingCode {
                            session: Arc::new(session),
                            url,
                            code: String::new(),
                            error: None,
                        });
                    }
                    Err(e) => {
                        d.providers.antigravity_auth = Some(AntigravityAuth::Failed(e.to_string()));
                    }
                }
            },
            cx,
        );
        cx.notify();
    }

    /// Submits the pasted code or callback URL to `agy acp --login`.
    pub(super) fn submit_antigravity_code(&mut self, cx: &mut Context<Self>) {
        let Some(AntigravityAuth::WaitingCode { session, code, .. }) =
            self.providers.antigravity_auth.take()
        else {
            return;
        };
        let submitted = code;
        let runner = Arc::clone(&session);
        self.providers.antigravity_auth = Some(AntigravityAuth::Verifying { session });
        self.run(
            move || runner.submit(&submitted),
            |d, done, cx| match done {
                Ok(()) => {
                    d.providers.antigravity_auth = None;
                    d.drop_fields(AUTH_CODE_FIELD);
                    d.core.retire_idle_agents();
                    d.core.usage.invalidate(AgentKind::Antigravity);
                    d.providers.status.remove(&AgentKind::Antigravity);
                    d.detect(AgentKind::Antigravity, cx);
                }
                Err(e) => {
                    d.providers.antigravity_auth = Some(AntigravityAuth::Failed(e.to_string()));
                    d.drop_fields(AUTH_CODE_FIELD);
                }
            },
            cx,
        );
        cx.notify();
    }

    /// Signs out of Antigravity via the official ACP `logout` RPC.
    pub(super) fn sign_out_antigravity(&mut self, cx: &mut Context<Self>) {
        if let Some(
            AntigravityAuth::WaitingCode { session, .. }
            | AntigravityAuth::Verifying { session, .. },
        ) = self.providers.antigravity_auth.take()
        {
            session.cancel();
        }
        self.drop_fields(AUTH_CODE_FIELD);
        self.providers.antigravity_auth = Some(AntigravityAuth::SigningOut);
        self.run(
            antigravity::sign_out,
            |d, done, cx| match done {
                Ok(()) => {
                    d.providers.antigravity_auth = None;
                    d.core.retire_idle_agents();
                    d.core.usage.invalidate(AgentKind::Antigravity);
                    d.providers.status.remove(&AgentKind::Antigravity);
                    d.detect(AgentKind::Antigravity, cx);
                }
                Err(e) => {
                    d.providers.antigravity_auth = Some(AntigravityAuth::Failed(e.to_string()));
                }
            },
            cx,
        );
        cx.notify();
    }

    /// Escape cancels an open Antigravity sign-in panel unless verifying or signing out.
    pub(crate) fn cancel_antigravity_auth(&mut self, cx: &mut Context<Self>) -> bool {
        match self.providers.antigravity_auth.take() {
            None => false,
            Some(state @ (AntigravityAuth::Verifying { .. } | AntigravityAuth::SigningOut)) => {
                self.providers.antigravity_auth = Some(state);
                true
            }
            Some(AntigravityAuth::WaitingCode { session, .. }) => {
                session.cancel();
                self.drop_fields(AUTH_CODE_FIELD);
                cx.notify();
                true
            }
            Some(AntigravityAuth::Starting | AntigravityAuth::Failed(_)) => {
                self.drop_fields(AUTH_CODE_FIELD);
                cx.notify();
                true
            }
        }
    }

    /// Inline panel under the Antigravity row while signing in, signing out, or showing an error.
    pub(super) fn antigravity_auth_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let t = *theme(cx);
        let state_code = match self.providers.antigravity_auth.as_ref()? {
            AntigravityAuth::WaitingCode { code, .. } => Some(code.clone()),
            AntigravityAuth::Starting
            | AntigravityAuth::Verifying { .. }
            | AntigravityAuth::SigningOut
            | AntigravityAuth::Failed(_) => None,
        };
        let field = state_code.map(|draft| {
            let f = self.field(
                AUTH_CODE_FIELD,
                "Google authorization code or callback URL",
                &draft,
                "Paste the authorization code or callback URL from Google",
                FieldKind {
                    live: true,
                    ..Default::default()
                },
                |d, text, _| {
                    if let Some(AntigravityAuth::WaitingCode { code, error, .. }) =
                        &mut d.providers.antigravity_auth
                    {
                        *code = text;
                        *error = None;
                    }
                },
                window,
                cx,
            );
            if draft.is_empty()
                && let Some(area) = self.field_area(AUTH_CODE_FIELD)
                && !area.read(cx).focus_handle(cx).is_focused(window)
            {
                area.read(cx).focus_handle(cx).focus(window, cx);
            }
            f
        });

        let panel = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .border_b_1()
            .border_color(t.border)
            .py(px(10.));

        Some(match self.providers.antigravity_auth.as_ref()? {
            AntigravityAuth::Starting => panel.child(
                div()
                    .id("agy-auth-starting")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .role(Role::Status)
                    .text_size(px(fs_sm()))
                    .text_color(t.text_muted)
                    .child(spinner(12., t.text_muted))
                    .child("Starting Antigravity ACP Google sign-in…")
                    .child(
                        btn("agy-auth-cancel", "Cancel", BtnKind::Plain, false, &t)
                            .child("Cancel")
                            .on_click(cx.listener(|d, _, _, cx| {
                                d.cancel_antigravity_auth(cx);
                            })),
                    ),
            ),
            AntigravityAuth::WaitingCode {
                url, code, error, ..
            } => {
                let open_url = url.clone();
                let empty = code.trim().is_empty();
                panel
                    .child(
                        div()
                            .text_size(px(fs_sm()))
                            .text_color(t.text_muted)
                            .child("Complete sign-in in your browser, then paste the authorization code or callback URL from Google below."),
                    )
                    .children(field)
                    .children(error.clone().map(|e| alert("agy-auth-error", e, t.danger)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                btn("agy-auth-open", "Open Google Sign-in", BtnKind::Plain, false, &t)
                                    .child("Open Google Sign-in")
                                    .on_click(move |_, _, cx| cx.open_url(&open_url)),
                            )
                            .child(
                                btn("agy-auth-cancel", "Cancel", BtnKind::Plain, false, &t)
                                    .child("Cancel")
                                    .on_click(cx.listener(|d, _, _, cx| {
                                        d.cancel_antigravity_auth(cx);
                                    })),
                            )
                            .child(
                                btn(
                                    "agy-auth-submit",
                                    "Complete Sign-in",
                                    BtnKind::Primary,
                                    empty,
                                    &t,
                                )
                                .child("Complete Sign-in")
                                .when(!empty, |b| {
                                    b.on_click(cx.listener(|d, _, _, cx| {
                                        d.submit_antigravity_code(cx);
                                    }))
                                }),
                            ),
                    )
            }
            AntigravityAuth::Verifying { .. } => panel.child(
                div()
                    .id("agy-auth-verifying")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .role(Role::Status)
                    .text_size(px(fs_sm()))
                    .text_color(t.text_muted)
                    .child(spinner(12., t.text_muted))
                    .child("Verifying Google sign-in with Antigravity…"),
            ),
            AntigravityAuth::SigningOut => panel.child(
                div()
                    .id("agy-auth-signing-out")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .role(Role::Status)
                    .text_size(px(fs_sm()))
                    .text_color(t.text_muted)
                    .child(spinner(12., t.text_muted))
                    .child("Signing out of Antigravity…"),
            ),
            AntigravityAuth::Failed(message) => panel
                .child(alert("agy-auth-failed", message.clone(), t.danger))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            btn("agy-auth-dismiss", "Dismiss", BtnKind::Plain, false, &t)
                                .child("Dismiss")
                                .on_click(cx.listener(|d, _, _, cx| {
                                    d.cancel_antigravity_auth(cx);
                                })),
                        )
                        .child(
                            btn("agy-auth-retry", "Try again", BtnKind::Primary, false, &t)
                                .child("Try again")
                                .on_click(cx.listener(|d, _, _, cx| {
                                    d.start_antigravity_login(cx);
                                })),
                        ),
                ),
        })
    }
}
