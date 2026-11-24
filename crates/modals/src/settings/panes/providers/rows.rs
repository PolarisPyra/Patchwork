//! The rows for the agent CLIs and the API providers.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, ClipboardItem, Div, FontWeight, Role, SharedString, Stateful, Window, div,
    prelude::*, px,
};
use patchwork_core::provider::api::{ApiProvider, ProviderView};
use patchwork_core::provider::{AgentKind, AgentStatus, UpdateMethod};
use crate::settings::SettingsDialog;
use crate::settings::controls::switch;
use patchwork_ui::theme::{fs, fs_sm, Theme, theme};
use patchwork_ui::{BtnKind, agent_label, agent_logo, badge, btn, provider_logo, spinner};
use super::AntigravityAuth;
use super::catalog::{CLI_KINDS, PromptOf, UpdateStep};
use super::cli;
use super::form;

/// How wide each button in a provider row is at medium text: wide enough for "System Prompt",
/// so the buttons line up from row to row.
const ACTION_W: f32 = 128.;
/// The narrowest a CLI's name and version may get, so "OpenCode CLI" never wraps.
const TITLE_MIN_W: f32 = 120.;

/// "Ready · 2.1.0", or why it is not: `describeAgent`.
pub(super) fn describe_agent(status: Option<&Option<AgentStatus>>) -> String {
    match status {
        None => "Checking…".into(),
        Some(None) => "Unavailable".into(),
        Some(Some(s)) if !s.installed => {
            if s.detail.is_empty() {
                "Not installed".into()
            } else {
                s.detail.clone()
            }
        }
        Some(Some(s)) => {
            let state = if s.signed_in {
                "Ready"
            } else {
                "Sign-in required"
            };
            match &s.version {
                Some(v) => format!("{state} · {v}"),
                None => state.into(),
            }
        }
    }
}

/// `.badge`, green when `ok`, red when `bad`.
pub(super) fn status_badge(text: impl Into<SharedString>, ok: bool, bad: bool, t: &Theme) -> Div {
    let b = badge(text, t);
    if ok {
        b.bg(t.success.opacity(0.15)).text_color(t.success)
    } else if bad {
        b.bg(t.danger_soft).text_color(t.danger)
    } else {
        b
    }
}

impl SettingsDialog {
    /// One row per CLI: whether it is installed, where it is, and whether Patchwork connects to it.
    pub(super) fn cli_rows(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let t = *theme(cx);
        let mut rows = Vec::new();
        for kind in CLI_KINDS {
            let label = agent_label(kind);
            let status = self.providers.status.get(&kind);
            let checking = status.is_none();
            let installed = status.is_some_and(|s| s.as_ref().is_some_and(|s| s.installed));
            let signed_in = status.is_some_and(|s| s.as_ref().is_some_and(|s| s.signed_in));
            let agy_auth_busy = kind == AgentKind::Antigravity
                && matches!(
                    self.providers.antigravity_auth,
                    Some(
                        AntigravityAuth::Starting
                            | AntigravityAuth::Verifying { .. }
                            | AntigravityAuth::SigningOut
                    )
                );
            let on = self.settings.agents.contains(&kind);
            let path = self.settings.agent_paths.get(&kind).cloned();
            let prompted = self
                .settings
                .system_prompts
                .get(&kind)
                .is_some_and(|p| !p.is_empty());
            let prompting = self
                .providers
                .prompt
                .as_ref()
                .is_some_and(|p| p.of == PromptOf::Cli(kind));
            let installing = self.providers.installing.contains(&kind);
            let update = self.providers.updates.get(&kind).filter(|_| installed);
            // Only installed agents can be switched on; any agent can be switched off.
            let locked = !on && !installed;
            let key = format!("{kind:?}").to_lowercase();
            let row = div()
                .flex()
                .items_center()
                .gap(px(12.))
                .border_b_1()
                .border_color(t.border)
                .py(px(10.))
                .child(agent_logo(kind, None, 20.))
                .child(
                    div()
                        .min_w(px(fs(TITLE_MIN_W)))
                        .flex_1()
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(label),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(fs_sm()))
                                .text_color(t.text_muted)
                                .child(match update {
                                    Some(u) => format!(
                                        "{} · {} available",
                                        describe_agent(status),
                                        u.release.latest
                                    ),
                                    None => describe_agent(status),
                                }),
                        )
                        .children(update.and_then(|u| {
                            match &u.step {
                                UpdateStep::Failed(e) => Some(
                                    div()
                                        .id(SharedString::from(format!("update-error-{key}")))
                                        .role(Role::Alert)
                                        .text_size(px(fs_sm()))
                                        .text_color(t.danger)
                                        .child(e.clone()),
                                ),
                                UpdateStep::Offered | UpdateStep::Running => None,
                            }
                        })),
                )
                // The Update button says the CLI is installed, and needs the badge's room.
                .when(update.is_none(), |r| {
                    r.child(status_badge(
                        if checking {
                            "Checking…"
                        } else if installed {
                            "Installed"
                        } else {
                            "Not installed"
                        },
                        installed,
                        false,
                        &t,
                    ))
                })
                .children(update.map(|u| {
                    let release = &u.release;
                    let running = matches!(u.step, UpdateStep::Running);
                    let button = match &release.method {
                        UpdateMethod::Run => btn(
                            SharedString::from(format!("update-{key}")),
                            format!(
                                "Update {label} from {} to {}",
                                release.installed, release.latest
                            ),
                            BtnKind::Plain,
                            running,
                            &t,
                        )
                        .when(running, |b| {
                            b.role(Role::Status)
                                .child(spinner(12., t.text_muted))
                                .child("Updating…")
                        })
                        .when(!running, |b| {
                            b.child("Update")
                                .on_click(cx.listener(move |d, _, _, cx| d.update_cli(kind, cx)))
                        }),
                        UpdateMethod::Manual(command) => {
                            let copied = command.clone();
                            btn(
                                SharedString::from(format!("update-{key}")),
                                format!("Copy the command that updates {label}: {command}"),
                                BtnKind::Plain,
                                false,
                                &t,
                            )
                            .tooltip(patchwork_ui::tooltip(command.clone()))
                            .child("Copy Update")
                            .on_click(cx.listener(
                                move |_, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copied.clone(),
                                    ));
                                },
                            ))
                        }
                    };
                    button.min_w(px(fs(ACTION_W))).into_any_element()
                }))
                .when(!installed && !checking && path.is_none(), |r| {
                    r.child(
                        btn(
                            SharedString::from(format!("install-{key}")),
                            if installing {
                                format!("Installing {label}")
                            } else {
                                format!("Download {label}")
                            },
                            BtnKind::Plain,
                            installing,
                            &t,
                        )
                        .min_w(px(fs(ACTION_W)))
                        .when(installing, |b| {
                            b.role(Role::Status)
                                .child(spinner(12., t.text_muted))
                                .child("Installing…")
                        })
                        .when(!installing, |b| {
                            b.child("Download")
                                .on_click(cx.listener(move |d, _, _, cx| d.install(kind, cx)))
                        }),
                    )
                    .children(self.providers.install_errors.get(&kind).map(|e| {
                        div()
                            .id(SharedString::from(format!("install-error-{key}")))
                            .role(Role::Alert)
                            .text_size(px(fs_sm()))
                            .text_color(t.danger)
                            .child(e.clone())
                    }))
                })
                .map(|r| match &path {
                    Some(path) => r.child(
                        btn(
                            SharedString::from(format!("auto-{key}")),
                            format!("Auto-detect {label} path"),
                            BtnKind::Plain,
                            false,
                            &t,
                        )
                        .tooltip(patchwork_ui::tooltip(path.clone()))
                        .min_w(px(fs(ACTION_W)))
                        .child("Auto-detect")
                        .on_click(cx.listener(move |d, _, _, cx| d.set_agent_path(kind, None, cx))),
                    ),
                    None if !checking && !installed => r.child(
                        btn(
                            SharedString::from(format!("locate-{key}")),
                            format!("Locate {label}"),
                            BtnKind::Plain,
                            false,
                            &t,
                        )
                        .min_w(px(fs(ACTION_W)))
                        .child("Locate…")
                        .on_click(cx.listener(move |d, _, _, cx| d.locate(kind, cx))),
                    ),
                    None => r,
                })
                .when(installed && cli::has_details(kind), |r| {
                    r.child(
                        btn(
                            SharedString::from(format!("details-{key}")),
                            format!("{label} details"),
                            BtnKind::Plain,
                            false,
                            &t,
                        )
                        .min_w(px(fs(ACTION_W)))
                        .child("Details")
                        .on_click(cx.listener(move |d, _, _, cx| d.open_details(kind, cx))),
                    )
                })
                .when(
                    kind == AgentKind::Antigravity && installed && !checking,
                    |r| {
                        if signed_in {
                            r.child(
                                btn(
                                    "agy-sign-out",
                                    "Sign out of Antigravity",
                                    BtnKind::Plain,
                                    agy_auth_busy,
                                    &t,
                                )
                                .min_w(px(fs(ACTION_W)))
                                .child("Sign out")
                                .when(!agy_auth_busy, |b| {
                                    b.on_click(
                                        cx.listener(|d, _, _, cx| d.sign_out_antigravity(cx)),
                                    )
                                }),
                            )
                        } else {
                            r.child(
                                btn(
                                    "agy-sign-in",
                                    "Sign in to Antigravity",
                                    BtnKind::Primary,
                                    agy_auth_busy,
                                    &t,
                                )
                                .min_w(px(fs(ACTION_W)))
                                .child(if agy_auth_busy {
                                    "Signing in…"
                                } else {
                                    "Sign in"
                                })
                                .when(!agy_auth_busy, |b| {
                                    b.on_click(
                                        cx.listener(|d, _, _, cx| d.start_antigravity_login(cx)),
                                    )
                                }),
                            )
                        }
                    },
                )
                // An agent that may not be given a system prompt offers none to write, and one not
                // installed yet has Download and Locate in the prompt's place, so its row fits.
                .when(kind.takes_persona() && (installed || prompting), |row| {
                    row.child(
                        btn(
                            SharedString::from(format!("prompt-{key}")),
                            format!("{label} system prompt"),
                            BtnKind::Plain,
                            false,
                            &t,
                        )
                        .aria_expanded(prompting)
                        .when(prompted, |b| b.text_color(t.accent))
                        .min_w(px(fs(ACTION_W)))
                        .child("System Prompt")
                        .on_click(cx.listener(move |d, _, window, cx| {
                            let saved = d
                                .settings
                                .system_prompts
                                .get(&kind)
                                .cloned()
                                .unwrap_or_default();
                            d.toggle_prompt(PromptOf::Cli(kind), saved, window, cx);
                        })),
                    )
                })
                .child(
                    switch(format!("connect-{key}"), format!("Connect {label}"), on, &t)
                        .when(locked, |s| s.opacity(0.5).cursor_default())
                        .when(!locked, |s| {
                            s.on_click(cx.listener(move |d, _, _, cx| {
                                d.update(
                                    |s| {
                                        if s.agents.contains(&kind) {
                                            s.agents.retain(|k| *k != kind);
                                        } else {
                                            s.agents.push(kind);
                                        }
                                    },
                                    cx,
                                );
                                d.load_vision_models(cx);
                            }))
                        }),
                );
            rows.push(row.into_any_element());
            if kind == AgentKind::Antigravity
                && let Some(panel) = self.antigravity_auth_panel(window, cx)
            {
                rows.push(panel.into_any_element());
            }
            if prompting {
                rows.push(self.prompt_editor(window, cx).into_any_element());
            }
        }
        rows
    }

    /// A saved cloud provider: click or Edit to change it, its system prompt, the switch to use it
    /// (`ApiProviderRow`).
    pub(super) fn api_row(
        &mut self,
        view: &ProviderView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let t = *theme(cx);
        let p = &view.provider;
        let prompting = self
            .providers
            .prompt
            .as_ref()
            .is_some_and(|x| x.of == PromptOf::Api(p.id.clone()));
        let row = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .border_b_1()
            .border_color(t.border)
            .py(px(10.))
            .child(provider_logo(&p.preset, p.logo.as_deref(), 20.))
            .child(provider_title(view, "", form::describe(view), &t, cx))
            .children(provider_actions(view, "", prompting, &t, cx));
        let mut rows = vec![row.into_any_element()];
        if prompting {
            rows.push(self.prompt_editor(window, cx).into_any_element());
        }
        rows
    }
}

/// A saved provider's name over `line`; a click opens its form. `ids` sets a local server's ids
/// apart from a cloud provider's.
pub(super) fn provider_title(
    view: &ProviderView,
    ids: &str,
    line: impl Into<SharedString>,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let p = &view.provider;
    let edit = view.clone();
    patchwork_ui::button(
        SharedString::from(format!("open-{ids}{}", p.id)),
        p.name.clone(),
    )
    .min_w_0()
    .flex_1()
    .flex_col()
    .items_start()
    .child(
        div()
            .font_weight(FontWeight::SEMIBOLD)
            .child(p.name.clone()),
    )
    .child(
        div()
            .w_full()
            .truncate()
            .text_size(px(fs_sm()))
            .text_color(t.text_muted)
            .child(line.into()),
    )
    .on_click(cx.listener(move |d, _, window, cx| {
        d.open_form(edit.provider.clone(), edit.has_key, window, cx);
    }))
}

/// A saved provider's Edit, System Prompt, and the switch to use it.
pub(super) fn provider_actions(
    view: &ProviderView,
    ids: &str,
    prompting: bool,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> [AnyElement; 3] {
    let p = &view.provider;
    let id = &p.id;
    let (edit, prompt_id, toggle) = (view.clone(), id.clone(), p.clone());
    let saved_prompt = p.system_prompt.clone();
    [
        btn(
            SharedString::from(format!("edit-{ids}{id}")),
            format!("Edit {}", p.name),
            BtnKind::Plain,
            false,
            t,
        )
        .min_w(px(fs(ACTION_W)))
        .child("Edit")
        .on_click(cx.listener(move |d, _, window, cx| {
            d.open_form(edit.provider.clone(), edit.has_key, window, cx);
        }))
        .into_any_element(),
        btn(
            SharedString::from(format!("prompt-{ids}{id}")),
            format!("{} system prompt", p.name),
            BtnKind::Plain,
            false,
            t,
        )
        .aria_expanded(prompting)
        .when(!p.system_prompt.is_empty(), |b| b.text_color(t.accent))
        .min_w(px(fs(ACTION_W)))
        .child("System Prompt")
        .on_click(cx.listener(move |d, _, window, cx| {
            d.toggle_prompt(
                PromptOf::Api(prompt_id.clone()),
                saved_prompt.clone(),
                window,
                cx,
            );
        }))
        .into_any_element(),
        switch(
            format!("use-{ids}{id}"),
            format!("Use {}", p.name),
            p.enabled,
            t,
        )
        .on_click(cx.listener(move |d, _, _, cx| {
            d.save_provider(
                ApiProvider {
                    enabled: !toggle.enabled,
                    ..toggle.clone()
                },
                cx,
            );
        }))
        .into_any_element(),
    ]
}
