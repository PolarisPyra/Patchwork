//! Settings → Providers (`providers/ProvidersPane.tsx`): the agent CLIs, whether each is installed
//! and connected, the API providers saved, the presets to add one from, and the model servers on
//! this computer. A CLI's Details and a provider's editor take the whole pane while open.

mod cli;
mod form;
mod local;

use std::collections::{HashMap, HashSet};

use gpui::{
    AnyElement, Context, Div, Focusable, FontWeight, PathPromptOptions, Role, SharedString,
    Stateful, Window, div, prelude::*, px,
};
use patchwork_server::provider::api::config::{self, ApiProvider, KeyChange, ProviderView};
use patchwork_server::provider::{AgentKind, AgentStatus, ModelOption};
use patchwork_server::settings::VisionModel;

use crate::settings::SettingsDialog;
use crate::settings::controls::{
    FieldKind, alert, choice, section_heading, select_box, switch, token_row,
};
use crate::theme::{FS_SM, RADIUS, Theme, theme};
use crate::ui::{AGENT_KINDS, BtnKind, agent_label, agent_logo, badge, btn, icon, provider_logo};

/// The agent CLIs, in the order the pane lists them (`CLI_KINDS`).
const CLI_KINDS: [AgentKind; 4] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Antigravity,
    AgentKind::OpenCode,
];

/// Agents whose models can read a message's images for an API model.
const VISION_AGENTS: [AgentKind; 3] = [AgentKind::Api, AgentKind::Claude, AgentKind::Codex];

/// The field the open system prompt is typed in.
const PROMPT_FIELD: &str = "provider-prompt";

/// Whose system prompt is open: a CLI's, kept in the settings, or a provider's, by id.
#[derive(Clone, PartialEq)]
enum PromptOf {
    Cli(AgentKind),
    Api(String),
}

/// The system prompt being edited: what it was, what is typed, and whether it is saving.
struct Prompt {
    of: PromptOf,
    saved: String,
    draft: String,
    busy: bool,
    error: Option<String>,
}

#[derive(Default)]
pub struct ProvidersState {
    /// Whether the agents were read and the providers loaded since the dialog opened.
    started: bool,
    /// Each CLI's reading, by kind: missing while it is checked, None when it could not be.
    status: HashMap<AgentKind, Option<AgentStatus>>,
    installing: HashSet<AgentKind>,
    install_errors: HashMap<AgentKind, String>,
    providers: Vec<ProviderView>,
    /// The models of each agent the vision picker offers.
    models: HashMap<AgentKind, Vec<ModelOption>>,
    prompt: Option<Prompt>,
    form: Option<form::Form>,
    details: Option<cli::Details>,
    local: local::LocalState,
    error: Option<String>,
}

/// "Ready · 2.1.0", or why it is not: `describeAgent`.
fn describe_agent(status: Option<&Option<AgentStatus>>) -> String {
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
fn tildify(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && path.starts_with(&home) => {
            format!("~{}", &path[home.len()..])
        }
        _ => path.to_string(),
    }
}

/// `.badge`, green when `ok`, red when `bad`.
fn status_badge(text: impl Into<SharedString>, ok: bool, bad: bool, t: &Theme) -> Div {
    let b = badge(text, t);
    if ok {
        b.bg(t.success.opacity(0.15)).text_color(t.success)
    } else if bad {
        b.bg(t.danger_soft).text_color(t.danger)
    } else {
        b
    }
}

/// A grid of provider tiles: the presets to add, and the servers on this computer.
fn tile_grid(label: &'static str) -> Stateful<Div> {
    div()
        .id(label)
        .role(Role::Group)
        .aria_label(label)
        .grid()
        .grid_cols(6)
        .gap(px(8.))
        .w_full()
        .py(px(10.))
}

/// One tile of a grid: a mark over a name.
fn tile(id: impl Into<SharedString>, label: impl Into<SharedString>, t: &Theme) -> Stateful<Div> {
    let (hover, strong, fg) = (t.bg_hover, t.border_strong, t.text);
    crate::ui::button(id.into(), label)
        .relative()
        .h(px(72.))
        .min_w_0()
        .flex_col()
        .justify_center()
        .gap(px(8.))
        .rounded(px(RADIUS))
        .border_1()
        .px(px(8.))
        .text_size(px(FS_SM))
        .hover(move |s| s.border_color(strong).bg(hover).text_color(fg))
}

impl SettingsDialog {
    /// Reads each CLI again, and loads the providers, the first time the pane draws.
    fn start_providers(&mut self, cx: &mut Context<Self>) {
        self.providers.started = true;
        for kind in CLI_KINDS {
            self.detect(kind, cx);
        }
        self.reload_providers(cx);
        self.discover_servers(cx);
    }

    fn detect(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        self.run(
            move || patchwork_server::provider::detect_agent(kind),
            move |d, status, _| {
                d.providers.status.insert(kind, Some(status));
            },
            cx,
        );
    }

    /// Loads the saved providers, then puts the API agent among the connected ones exactly when a
    /// provider can run a task (`syncAgents`).
    fn reload_providers(&mut self, cx: &mut Context<Self>) {
        self.run(
            config::list,
            |d, list, cx| {
                crate::ui::reload_api_providers();
                let usable = list
                    .iter()
                    .any(|p| p.provider.enabled && !p.provider.models.is_empty());
                if usable != d.settings.agents.contains(&AgentKind::Api) {
                    d.update(
                        |s| {
                            if usable {
                                s.agents.push(AgentKind::Api);
                            } else {
                                s.agents.retain(|k| *k != AgentKind::Api);
                            }
                        },
                        cx,
                    );
                }
                let ids: Vec<String> = list
                    .iter()
                    .filter(|p| p.provider.local)
                    .map(|p| p.provider.id.clone())
                    .collect();
                d.providers.providers = list;
                d.probe_local(ids, cx);
                d.load_vision_models(cx);
            },
            cx,
        );
    }

    /// Saves a provider as it is with its key kept, then loads the list again.
    fn save_provider(&mut self, provider: ApiProvider, cx: &mut Context<Self>) {
        self.providers.error = None;
        self.run(
            move || config::save(provider, KeyChange::Keep),
            |d, saved, cx| match saved {
                Ok(_) => d.reload_providers(cx),
                Err(e) => d.providers.error = Some(e),
            },
            cx,
        );
    }

    fn load_vision_models(&mut self, cx: &mut Context<Self>) {
        for kind in VISION_AGENTS {
            if !self.settings.agents.contains(&kind) || self.providers.models.contains_key(&kind) {
                continue;
            }
            self.run(
                move || patchwork_server::provider::list_models(kind),
                move |d, list, _| {
                    d.providers.models.insert(kind, list);
                },
                cx,
            );
        }
    }

    fn install(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        self.providers.installing.insert(kind);
        self.providers.install_errors.remove(&kind);
        let server = self.server.clone();
        self.run(
            move || server.install_cli(kind),
            move |d, done, cx| {
                d.providers.installing.remove(&kind);
                match done {
                    Ok(()) => d.detect(kind, cx),
                    Err(e) => {
                        d.providers.install_errors.insert(kind, e);
                    }
                }
            },
            cx,
        );
    }

    /// Uses `path` for the CLI, or finds it again when None, then reads it again.
    fn set_agent_path(&mut self, kind: AgentKind, path: Option<String>, cx: &mut Context<Self>) {
        self.providers.status.remove(&kind);
        self.update(
            |s| match path {
                Some(path) => {
                    s.agent_paths.insert(kind, path);
                }
                None => {
                    s.agent_paths.remove(&kind);
                }
            },
            cx,
        );
        self.detect(kind, cx);
    }

    fn locate(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.first().map(|p| p.to_string_lossy().into_owned()) else {
                return;
            };
            let _ = this.update(cx, |d, cx| d.set_agent_path(kind, Some(path), cx));
        })
        .detach();
    }

    /// Opens a system prompt, or closes it when it is the one open.
    fn toggle_prompt(
        &mut self,
        of: PromptOf,
        saved: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drop_fields(PROMPT_FIELD);
        if self.providers.prompt.as_ref().is_some_and(|p| p.of == of) {
            self.providers.prompt = None;
            cx.notify();
            return;
        }
        self.providers.prompt = Some(Prompt {
            of,
            draft: saved.clone(),
            saved,
            busy: false,
            error: None,
        });
        // Draw the field now, so it can take focus as the web editor's `autoFocus` does.
        let _ = self.prompt_field(window, cx);
        if let Some(area) = self.field_area(PROMPT_FIELD) {
            area.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn prompt_field(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let (label, draft) = match &self.providers.prompt {
            Some(p) => (
                match &p.of {
                    PromptOf::Cli(kind) => format!("{} system prompt", agent_label(*kind)),
                    PromptOf::Api(id) => format!("{} system prompt", self.provider_name(id)),
                },
                p.draft.clone(),
            ),
            None => (String::new(), String::new()),
        };
        self.field(
            PROMPT_FIELD,
            label,
            &draft,
            "",
            FieldKind {
                live: true,
                lines: true,
                ..Default::default()
            },
            |d, text, _| {
                if let Some(p) = &mut d.providers.prompt {
                    p.draft = text;
                }
            },
            window,
            cx,
        )
    }

    fn provider_name(&self, id: &str) -> String {
        self.providers
            .providers
            .iter()
            .find(|p| p.provider.id == id)
            .map_or_else(|| id.to_string(), |p| p.provider.name.clone())
    }

    fn save_prompt(&mut self, cx: &mut Context<Self>) {
        let Some(prompt) = &mut self.providers.prompt else {
            return;
        };
        prompt.busy = true;
        prompt.error = None;
        let text = prompt.draft.clone();
        match prompt.of.clone() {
            PromptOf::Cli(kind) => {
                self.update(
                    |s| {
                        if text.trim().is_empty() {
                            s.system_prompts.remove(&kind);
                        } else {
                            s.system_prompts.insert(kind, text);
                        }
                    },
                    cx,
                );
                self.providers.prompt = None;
                self.drop_fields(PROMPT_FIELD);
            }
            PromptOf::Api(id) => {
                let Some(view) = self
                    .providers
                    .providers
                    .iter()
                    .find(|p| p.provider.id == id)
                else {
                    return;
                };
                let provider = ApiProvider {
                    system_prompt: text,
                    ..view.provider.clone()
                };
                self.run(
                    move || config::save(provider, KeyChange::Keep),
                    |d, saved, cx| match saved {
                        Ok(_) => {
                            d.providers.prompt = None;
                            d.drop_fields(PROMPT_FIELD);
                            d.reload_providers(cx);
                        }
                        Err(e) => {
                            if let Some(p) = &mut d.providers.prompt {
                                p.busy = false;
                                p.error = Some(e);
                            }
                        }
                    },
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// The open system prompt, with Clear, Cancel and Save under it (`PromptEditor`).
    fn prompt_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = *theme(cx);
        let field = self.prompt_field(window, cx);
        let Some(p) = &self.providers.prompt else {
            return div();
        };
        let (busy, empty, unchanged) = (p.busy, p.draft.is_empty(), p.draft == p.saved);
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .border_b_1()
            .border_color(t.border)
            .py(px(10.))
            .child(field)
            .children(p.error.clone().map(|e| alert("prompt-error", e, t.danger)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        btn("prompt-clear", "Clear", BtnKind::Plain, busy || empty, &t)
                            .child("Clear")
                            .when(!busy && !empty, |b| {
                                b.on_click(cx.listener(|d, _, _, cx| {
                                    if let Some(p) = &mut d.providers.prompt {
                                        p.draft.clear();
                                    }
                                    if let Some(area) = d.field_area(PROMPT_FIELD) {
                                        area.update(cx, |a, cx| a.set_text("", cx));
                                    }
                                    cx.notify();
                                }))
                            }),
                    )
                    .child(
                        btn("prompt-cancel", "Cancel", BtnKind::Plain, busy, &t)
                            .child("Cancel")
                            .when(!busy, |b| {
                                b.on_click(cx.listener(|d, _, _, cx| {
                                    d.providers.prompt = None;
                                    d.drop_fields(PROMPT_FIELD);
                                    cx.notify();
                                }))
                            }),
                    )
                    .child(
                        btn(
                            "prompt-save",
                            "Save",
                            BtnKind::Primary,
                            busy || unchanged,
                            &t,
                        )
                        .child(if busy { "Saving…" } else { "Save" })
                        .when(!busy && !unchanged, |b| {
                            b.on_click(cx.listener(|d, _, _, cx| d.save_prompt(cx)))
                        }),
                    ),
            )
    }

    /// One row per CLI: whether it is installed, where it is, and whether Patchwork connects to it.
    fn cli_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
                        d.toggle_prompt(PromptOf::Cli(kind), saved, window, cx)
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
    fn api_row(
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
                        d.open_form(edit_a.provider.clone(), edit_a.has_key, window, cx)
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
                    d.open_form(edit_b.provider.clone(), edit_b.has_key, window, cx)
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
                    )
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
                    )
                })),
            );
        let mut rows = vec![row.into_any_element()];
        if prompting {
            rows.push(self.prompt_editor(window, cx).into_any_element());
        }
        rows
    }

    /// The model that reads a message's images for an API chat, from every connected agent that
    /// can read them; the first choice sends the images to the chat's own model.
    fn vision_picker(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *theme(cx);
        let vision = self.settings.vision.clone();
        let kinds: Vec<AgentKind> = AGENT_KINDS
            .into_iter()
            .filter(|k| VISION_AGENTS.contains(k) && self.settings.agents.contains(k))
            .collect();
        let mut options: Vec<(Option<VisionModel>, String)> =
            vec![(None, "The chat's own model".into())];
        for kind in kinds {
            let mut listed: Vec<(String, String)> = self
                .providers
                .models
                .get(&kind)
                .into_iter()
                .flatten()
                .filter_map(|m| m.id.clone().map(|id| (id, m.label.clone())))
                .collect();
            // A saved model that is no longer listed still shows as picked.
            if let Some(v) = vision.as_ref().filter(|v| v.agent == kind)
                && !listed.iter().any(|(id, _)| *id == v.model)
            {
                listed.push((v.model.clone(), v.model.clone()));
            }
            for (model, name) in listed {
                options.push((
                    Some(VisionModel { agent: kind, model }),
                    format!("{} · {name}", agent_label(kind)),
                ));
            }
        }
        let current = options
            .iter()
            .find(|(v, _)| {
                v.as_ref().map(|v| (v.agent, &v.model))
                    == vision.as_ref().map(|v| (v.agent, &v.model))
            })
            .map_or_else(
                || "The chat's own model".to_string(),
                |(_, name)| name.clone(),
            );
        let open = self.menu_open("vision");
        select_box("vision-model", "Vision model", current, open, &t)
            .w(px(200.))
            .on_click(cx.listener(move |d, e: &gpui::ClickEvent, window, cx| {
                let weak = cx.entity().downgrade();
                let items = options
                    .iter()
                    .map(|(v, name)| {
                        let pick = v.clone();
                        let checked = v.as_ref().map(|v| (v.agent, &v.model))
                            == d.settings.vision.as_ref().map(|v| (v.agent, &v.model));
                        choice(weak.clone(), name.clone(), checked, move |d, cx| {
                            let pick = pick.clone();
                            d.menu = None;
                            d.update(|s| s.vision = pick, cx)
                        })
                    })
                    .collect();
                d.toggle_menu("vision", "Vision model", items, e.position(), window, cx)
            }))
            .into_any_element()
    }

    /// Which connected agent a new task starts with: the first of `agents`.
    fn default_picker(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *theme(cx);
        let connected = self.settings.agents.clone();
        let current = connected.first().map_or("", |k| agent_label(*k));
        let open = self.menu_open("default-agent");
        select_box("default-agent", "Default for new tasks", current, open, &t)
            .w(px(200.))
            .on_click(cx.listener(move |d, e: &gpui::ClickEvent, window, cx| {
                let weak = cx.entity().downgrade();
                let first = d.settings.agents.first().copied();
                let items = AGENT_KINDS
                    .into_iter()
                    .filter(|k| connected.contains(k))
                    .map(|kind| {
                        choice(
                            weak.clone(),
                            agent_label(kind),
                            first == Some(kind),
                            move |d, cx| {
                                d.menu = None;
                                d.update(
                                    |s| {
                                        s.agents.retain(|k| *k != kind);
                                        s.agents.insert(0, kind);
                                    },
                                    cx,
                                )
                            },
                        )
                    })
                    .collect();
                d.toggle_menu(
                    "default-agent",
                    "Default for new tasks",
                    items,
                    e.position(),
                    window,
                    cx,
                )
            }))
            .into_any_element()
    }

    /// The presets to start a provider from, then Add a server for any other address.
    fn preset_grid(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = *theme(cx);
        let tiles: Vec<_> = form::PRESETS
            .iter()
            .map(|&(id, name, base_url, models)| {
                tile(format!("preset-{id}"), format!("Add {name}"), &t)
                    .border_color(t.border)
                    .bg(t.bg_panel)
                    .text_color(t.text)
                    .child(provider_logo(id, None, 22.))
                    .child(div().max_w_full().truncate().child(name))
                    .on_click(cx.listener(move |d, _, window, cx| {
                        d.open_form(form::blank(id, name, base_url, models), false, window, cx)
                    }))
            })
            .collect();
        tile_grid("Add a provider").children(tiles).child(
            tile("preset-custom", "Add a server", &t)
                .border_dashed()
                .border_color(t.border_strong)
                .text_color(t.text_muted)
                .child(icon("plus", 22.))
                .child(div().max_w_full().truncate().child("Add a server"))
                .on_click(cx.listener(|d, _, window, cx| {
                    d.open_form(form::blank("custom", "", "", &[]), false, window, cx)
                })),
        )
    }
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    if !d.providers.started {
        d.start_providers(cx);
    }
    if d.providers.details.is_some() {
        return cli::render(d, window, cx);
    }
    if d.providers.form.is_some() {
        return form::render(d, window, cx);
    }
    let cli_rows = d.cli_rows(window, cx);
    let cloud: Vec<ProviderView> = d
        .providers
        .providers
        .iter()
        .filter(|p| !p.provider.local)
        .cloned()
        .collect();
    let mut cloud_rows = Vec::new();
    for view in &cloud {
        cloud_rows.extend(d.api_row(view, window, cx));
    }
    let connected = d.settings.agents.clone();
    let vision = connected
        .contains(&AgentKind::Api)
        .then(|| token_row("Vision model", None, &t).child(d.vision_picker(cx)));
    let default = (connected.len() > 1)
        .then(|| token_row("Default for new tasks", None, &t).child(d.default_picker(cx)));
    let presets = d.preset_grid(cx);
    let local = local::render(d, window, cx);
    div()
        .flex()
        .flex_col()
        .children(cli_rows)
        .children(cloud_rows)
        .children(vision)
        .children(default)
        .child(section_heading("Add a provider", false, &t))
        .child(presets)
        .child(section_heading("Local providers", false, &t))
        .child(local)
        .children(
            d.providers
                .error
                .clone()
                .map(|e| alert("providers-error", e, t.danger)),
        )
        .into_any_element()
}
