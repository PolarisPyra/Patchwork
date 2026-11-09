//! The rows for the agent CLIs and the API providers.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Div, FontWeight, Role, SharedString, Window, div, prelude::*, px};
use patchwork_server::provider::api::config::{ApiProvider, ProviderView};
use patchwork_server::provider::AgentStatus;
use crate::settings::SettingsDialog;
use crate::settings::controls::switch;
use crate::ui::theme::{FS_SM, Theme, theme};
use crate::ui::{BtnKind, agent_label, agent_logo, badge, btn, provider_logo};
use super::state::{CLI_KINDS, PromptOf};
use super::cli;
use super::form;

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

/// `~/…` for a path under the home folder (`tildify`).
pub(super) fn tildify(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => {
            format!("~{}", &path[home.len()..])
        }
        _ => path.to_string(),
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
                        .min_w_0()
                        .flex_1()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(label))
                        .child(
                            div()
                                .text_size(px(FS_SM))
                                .text_color(t.text_muted)
                                .child(describe_agent(status)),
                        ),
                )
                .child(status_badge(
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
                .when(!installed && !checking && path.is_none(), |r| {
                    r.child(
                        btn(
                            SharedString::from(format!("install-{key}")),
                            "Download",
                            BtnKind::Plain,
                            installing,
                            &t,
                        )
                        .child(if installing {
                            "Installing…"
                        } else {
                            "Download"
                        })
                        .when(!installing, |b| {
                            b.on_click(cx.listener(move |d, _, _, cx| d.install(kind, cx)))
                        }),
                    )
                    .children(self.providers.install_errors.get(&kind).map(|e| {
                        div()
                            .id(SharedString::from(format!("install-error-{key}")))
                            .role(Role::Alert)
                            .text_size(px(FS_SM))
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
                        .tooltip(crate::ui::tooltip(path.clone()))
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
                        .child("Details")
                        .on_click(cx.listener(move |d, _, _, cx| d.open_details(kind, cx))),
                    )
                })
                .child(
                    btn(
                        SharedString::from(format!("prompt-{key}")),
                        format!("{label} system prompt"),
                        BtnKind::Plain,
                        false,
                        &t,
                    )
                    .aria_expanded(prompting)
                    .when(prompted, |b| b.text_color(t.accent))
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
        let id = p.id.clone();
        let prompting = self
            .providers
            .prompt
            .as_ref()
            .is_some_and(|x| x.of == PromptOf::Api(id.clone()));
        let (edit_a, edit_b, prompt_id, toggle) =
            (view.clone(), view.clone(), id.clone(), p.clone());
        let saved_prompt = p.system_prompt.clone();
        let row = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .border_b_1()
            .border_color(t.border)
            .py(px(10.))
            .child(provider_logo(&p.preset, p.logo.as_deref(), 20.))
            .child(
                crate::ui::button(SharedString::from(format!("open-{id}")), p.name.clone())
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
                            .text_size(px(FS_SM))
                            .text_color(t.text_muted)
                            .child(form::describe(view)),
                    )
                    .on_click(cx.listener(move |d, _, window, cx| {
                        d.open_form(edit_a.provider.clone(), edit_a.has_key, window, cx);
                    })),
            )
            .child(
                btn(
                    SharedString::from(format!("edit-{id}")),
                    format!("Edit {}", p.name),
                    BtnKind::Plain,
                    false,
                    &t,
                )
                .child("Edit")
                .on_click(cx.listener(move |d, _, window, cx| {
                    d.open_form(edit_b.provider.clone(), edit_b.has_key, window, cx);
                })),
            )
            .child(
                btn(
                    SharedString::from(format!("prompt-{id}")),
                    format!("{} system prompt", p.name),
                    BtnKind::Plain,
                    false,
                    &t,
                )
                .aria_expanded(prompting)
                .when(!p.system_prompt.is_empty(), |b| b.text_color(t.accent))
                .child("System Prompt")
                .on_click(cx.listener(move |d, _, window, cx| {
                    d.toggle_prompt(
                        PromptOf::Api(prompt_id.clone()),
                        saved_prompt.clone(),
                        window,
                        cx,
                    );
                })),
            )
            .child(
                switch(
                    format!("use-{id}"),
                    format!("Use {}", p.name),
                    p.enabled,
                    &t,
                )
                .on_click(cx.listener(move |d, _, _, cx| {
                    d.save_provider(
                        ApiProvider {
                            enabled: !toggle.enabled,
                            ..toggle.clone()
                        },
                        cx,
                    );
                })),
            );
        let mut rows = vec![row.into_any_element()];
        if prompting {
            rows.push(self.prompt_editor(window, cx).into_any_element());
        }
        rows
    }
}
