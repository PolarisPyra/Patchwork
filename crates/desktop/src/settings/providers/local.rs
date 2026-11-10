//! The Local providers section: a tile for each model server on
//! this computer, with a dot for whether it is answering. A saved server's tile opens its row; a
//! server running here that is not added yet adds it.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use gpui::{AnyElement, Div, Focusable, FontWeight, Global, SharedString, Window, div, prelude::*, px};
use patchwork_server::provider::api::config::{
    self, ApiModel, ApiProvider, DiscoveredServer, KeyChange, ProviderHealth, ProviderView,
};
use patchwork_server::provider::api::servers;

use super::{PromptOf, form};
use super::rows::status_badge;
use super::pickers::{tile, tile_grid};
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert, switch};
use crate::ui::theme::{FONT_MONO, FS_SM, RADIUS_SM, theme};
use crate::ui::{BtnKind, btn, icon, provider_logo, spinner};

/// The field a moved server's new address is typed in.
const ADDRESS_FIELD: &str = "local-address";

/// Each saved server's last probe, kept while Settings is closed so reopening it shows the last
/// status at once and refreshes it in place, rather than every row going back to "Checking…".
#[derive(Default)]
struct HealthCache(HashMap<String, ProviderHealth>);

impl Global for HealthCache {}

#[derive(Default)]
pub struct LocalState {
    /// The probe of each saved server, by id: missing while it is checked.
    health: HashMap<String, ProviderHealth>,
    /// Servers answering on this computer, once looked for.
    found: Option<Vec<DiscoveredServer>>,
    /// The saved server whose row is open.
    selected: Option<String>,
    /// The new address being typed for the open server, when Change address is open.
    address: Option<String>,
    repairing: bool,
    repair_error: Option<String>,
}

/// The preset a found server is saved as, which picks its logo: only LM Studio has one.
fn preset_of(d: &DiscoveredServer) -> &'static str {
    if d.name == "LM Studio" {
        "lmstudio"
    } else {
        "custom"
    }
}

/// A custom provider prefilled from a server found on this computer, so Add opens the editor ready
/// to save.
fn from_discovered(d: &DiscoveredServer) -> ApiProvider {
    ApiProvider {
        local: true,
        preset: preset_of(d).to_string(),
        name: d.name.clone(),
        base_url: d.base_url.clone(),
        models: d
            .models
            .iter()
            .map(|id| ApiModel {
                id: id.clone(),
                name: None,
            })
            .collect(),
        ..form::blank("custom", "", "", &[])
    }
}

/// One line under a server's name: what it serves right now, or why it is not answering.
fn health_line(p: &ApiProvider, health: Option<&ProviderHealth>) -> String {
    let Some(h) = health else {
        return "Checking…".into();
    };
    if !h.reachable {
        return h.error.clone().unwrap_or_else(|| "Not answering".into());
    }
    let served = h.served_models.len();
    let missing = p
        .models
        .iter()
        .filter(|m| !h.served_models.contains(&m.id))
        .count();
    let mut parts = vec![format!(
        "{served} model{} served",
        if served == 1 { "" } else { "s" }
    )];
    if missing > 0 {
        parts.push(format!("{missing} configured not served"));
    }
    let reads = h.images.len();
    let vision = h.images.values().filter(|v| **v).count();
    if reads > 0 {
        parts.push(if vision == 0 {
            "text only".into()
        } else if vision == reads {
            "reads images".into()
        } else {
            format!("{vision} of {reads} read images")
        });
    }
    if let Some(ms) = h.latency_ms {
        parts.push(format!("{ms} ms"));
    }
    parts.join(" · ")
}

fn trim_slash(url: &str) -> &str {
    url.trim_end_matches('/')
}

impl SettingsDialog {
    /// Probes each saved server, keeping its last result until the fresh one lands, and forgets
    /// servers that are gone.
    pub(super) fn probe_local(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        let cache = &mut cx.default_global::<HealthCache>().0;
        cache.retain(|id, _| ids.contains(id));
        let health = &mut self.providers.local.health;
        health.retain(|id, _| ids.contains(id));
        for (id, last) in cache.iter() {
            health.entry(id.clone()).or_insert_with(|| last.clone());
        }
        for id in ids {
            self.probe_one(id, cx);
        }
    }

    fn probe_one(&mut self, id: String, cx: &mut Context<Self>) {
        let key = id.clone();
        self.run(
            move || config::probe(&id),
            move |d, result, cx| {
                let health = result.unwrap_or_else(|e| ProviderHealth {
                    reachable: false,
                    latency_ms: None,
                    served_models: Vec::new(),
                    error: Some(e.to_string()),
                    images: HashMap::new(),
                });
                cx.default_global::<HealthCache>()
                    .0
                    .insert(key.clone(), health.clone());
                d.providers.local.health.insert(key, health);
            },
            cx,
        );
    }

    /// Looks for model servers answering on this computer's usual ports.
    pub(super) fn discover_servers(&mut self, cx: &mut Context<Self>) {
        self.run(
            config::discover,
            |d, found, _| d.providers.local.found = Some(found),
            cx,
        );
    }

    fn close_address(&mut self) {
        self.providers.local.address = None;
        self.drop_fields(ADDRESS_FIELD);
    }

    /// Runs a repair of the open server, then reloads and probes it again.
    fn repair(
        &mut self,
        id: String,
        task: impl FnOnce() -> Result<(), patchwork_server::Error> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let local = &mut self.providers.local;
        local.repairing = true;
        local.repair_error = None;
        self.run(
            task,
            move |d, done, cx| {
                d.providers.local.repairing = false;
                match done {
                    Ok(()) => {
                        d.close_address();
                        d.providers.local.health.remove(&id);
                        cx.default_global::<HealthCache>().0.remove(&id);
                        d.reload_providers(cx);
                    }
                    Err(e) => d.providers.local.repair_error = Some(e.to_string()),
                }
            },
            cx,
        );
        cx.notify();
    }

    /// Under a server that stopped answering: look for where it moved, or type its new address.
    fn repair_row(
        &mut self,
        view: &ProviderView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = *theme(cx);
        let p = &view.provider;
        let busy = self.providers.local.repairing;
        let error = self.providers.local.repair_error.clone();
        let body = if self.providers.local.address.is_none() {
            let (find_id, base) = (p.id.clone(), p.base_url.clone());
            div()
                .flex()
                .gap(px(8.))
                .child(
                    btn(
                        "find-server",
                        format!("Find where {} moved", p.name),
                        BtnKind::Plain,
                        busy,
                        &t,
                    )
                    .child(if busy {
                        spinner(13., t.text_muted).into_any_element()
                    } else {
                        icon("magnifying-glass", 13.).into_any_element()
                    })
                    .child("Find server")
                    .when(!busy, |b| {
                        b.on_click(cx.listener(move |d, _, _, cx| {
                            let id = find_id.clone();
                            d.repair(id.clone(), move || servers::relocate(&id).map(|_| ()), cx);
                        }))
                    }),
                )
                .child(
                    btn("change-address", "Change address", BtnKind::Plain, busy, &t)
                        .child("Change address")
                        .when(!busy, |b| {
                            b.on_click(cx.listener(move |d, _, window, cx| {
                                d.providers.local.address = Some(base.clone());
                                let _ = d.address_field(window, cx);
                                if let Some(area) = d.field_area(ADDRESS_FIELD) {
                                    area.read(cx).focus_handle(cx).focus(window, cx);
                                }
                                cx.notify();
                            }))
                        }),
                )
        } else {
            let field = self.address_field(window, cx);
            let provider = p.clone();
            let save = move |d: &mut Self, cx: &mut Context<Self>| {
                let Some(address) = d.providers.local.address.clone() else {
                    return;
                };
                let provider = ApiProvider {
                    base_url: address,
                    ..provider.clone()
                };
                let id = provider.id.clone();
                d.repair(
                    id,
                    move || config::save(provider, KeyChange::Keep).map(|_| ()),
                    cx,
                );
            };
            let submit = save.clone();
            self.on_submit(ADDRESS_FIELD, move |d, _, cx| submit(d, cx));
            div()
                .flex()
                .gap(px(8.))
                .child(field.flex_1())
                .child(
                    btn("address-save", "Save", BtnKind::Plain, busy, &t)
                        .when(busy, |b| b.child(spinner(13., t.text_muted)))
                        .child("Save")
                        .when(!busy, |b| {
                            b.on_click(cx.listener(move |d, _, _, cx| save(d, cx)))
                        }),
                )
                .child(
                    btn("address-cancel", "Cancel", BtnKind::Plain, false, &t)
                        .child("Cancel")
                        .on_click(cx.listener(|d, _, _, cx| {
                            d.close_address();
                            cx.notify();
                        })),
                )
        };
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .border_b_1()
            .border_color(t.border)
            .py(px(8.))
            .child(body)
            .children(error.map(|e| alert("repair-error", e, t.danger).pt(px(0.))))
    }

    fn address_field(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let name = self
            .selected_local()
            .map(|v| v.provider.name.clone())
            .unwrap_or_default();
        let value = self.providers.local.address.clone().unwrap_or_default();
        self.field(
            ADDRESS_FIELD,
            format!("{name} address"),
            &value,
            "",
            FieldKind {
                live: true,
                mono: true,
                ..Default::default()
            },
            |d, text, _| d.providers.local.address = Some(text),
            window,
            cx,
        )
        .font_family(FONT_MONO)
    }

    fn selected_local(&self) -> Option<&ProviderView> {
        let id = self.providers.local.selected.as_ref()?;
        self.providers
            .providers
            .iter()
            .find(|p| p.provider.local && &p.provider.id == id)
    }

    /// The open server's row: its status, Recheck, Edit, its system prompt, and the use switch
    /// (`ServerRow`).
    fn server_row(
        &mut self,
        view: &ProviderView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let t = *theme(cx);
        let p = &view.provider;
        let id = p.id.clone();
        let health = self.providers.local.health.get(&id);
        let checking = health.is_none();
        let reachable = health.is_some_and(|h| h.reachable);
        let line = health_line(p, health);
        let prompting = self
            .providers
            .prompt
            .as_ref()
            .is_some_and(|x| x.of == PromptOf::Api(id.clone()));
        let (hover, fg) = (t.bg_hover, t.text);
        let (edit_a, edit_b, recheck, prompt_id, toggle) = (
            view.clone(),
            view.clone(),
            id.clone(),
            id.clone(),
            p.clone(),
        );
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
                crate::ui::button(
                    SharedString::from(format!("open-local-{id}")),
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
                        .text_size(px(FS_SM))
                        .text_color(t.text_muted)
                        .child(line),
                )
                .on_click(cx.listener(move |d, _, window, cx| {
                    d.open_form(edit_a.provider.clone(), edit_a.has_key, window, cx);
                })),
            )
            .child(status_badge(
                if checking {
                    "Checking…"
                } else if reachable {
                    "Ready"
                } else {
                    "Unreachable"
                },
                reachable,
                !checking && !reachable,
                &t,
            ))
            .child(
                crate::ui::button(
                    SharedString::from(format!("recheck-{id}")),
                    format!("Recheck {}", p.name),
                )
                .size(px(28.))
                .justify_center()
                .rounded(px(RADIUS_SM))
                .text_color(t.text_muted)
                .hover(move |s| s.bg(hover).text_color(fg))
                .tooltip(crate::ui::tooltip("Recheck"))
                .child(if checking {
                    spinner(13., t.text_muted).into_any_element()
                } else {
                    icon("arrow-clockwise", 15.).into_any_element()
                })
                .on_click(cx.listener(move |d, _, _, cx| {
                    d.providers.local.health.remove(&recheck);
                    d.probe_one(recheck.clone(), cx);
                    cx.notify();
                })),
            )
            .child(
                btn(
                    SharedString::from(format!("edit-local-{id}")),
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
                    SharedString::from(format!("prompt-local-{id}")),
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
                    format!("use-local-{id}"),
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
        if !checking && !reachable {
            rows.push(self.repair_row(view, window, cx).into_any_element());
        }
        if prompting {
            rows.push(self.prompt_editor(window, cx).into_any_element());
        }
        rows
    }
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let saved: Vec<ProviderView> = d
        .providers
        .providers
        .iter()
        .filter(|p| p.provider.local)
        .cloned()
        .collect();
    let saved_urls: Vec<&str> = saved
        .iter()
        .map(|p| trim_slash(&p.provider.base_url))
        .collect();
    let unsaved: Vec<&DiscoveredServer> = d
        .providers
        .local
        .found
        .iter()
        .flatten()
        .filter(|f| !saved_urls.contains(&trim_slash(&f.base_url)))
        .collect();
    let empty = d.providers.local.found.is_some() && saved.is_empty() && unsaved.is_empty();
    let selected = d.providers.local.selected.clone();

    let mut tiles: Vec<AnyElement> = Vec::new();
    for view in &saved {
        let p = &view.provider;
        let health = d.providers.local.health.get(&p.id);
        let (status, dot) = if !p.enabled {
            ("off", t.text_subtle)
        } else if let Some(h) = health {
            if h.reachable {
                ("answering", t.success)
            } else {
                ("not answering", t.danger)
            }
        } else {
            ("checking", t.border_strong)
        };
        let open = selected.as_deref() == Some(p.id.as_str());
        let id = p.id.clone();
        tiles.push(
            tile(format!("local-{id}"), format!("{}, {status}", p.name), &t)
                .aria_expanded(open)
                .border_color(if open { t.accent } else { t.border })
                .bg(t.bg_panel)
                .text_color(t.text)
                .tooltip(crate::ui::tooltip(health_line(p, health)))
                .child(
                    div()
                        .absolute()
                        .top(px(8.))
                        .right(px(8.))
                        .size(px(8.))
                        .rounded_full()
                        .bg(dot),
                )
                .child(provider_logo(&p.preset, p.logo.as_deref(), 22.))
                .child(div().max_w_full().truncate().child(p.name.clone()))
                .on_click(cx.listener(move |d, _, _, cx| {
                    let local = &mut d.providers.local;
                    local.selected = if local.selected.as_deref() == Some(id.as_str()) {
                        None
                    } else {
                        Some(id.clone())
                    };
                    local.repair_error = None;
                    d.close_address();
                    cx.notify();
                }))
                .into_any_element(),
        );
    }
    for found in &unsaved {
        let provider = from_discovered(found);
        tiles.push(
            tile(
                SharedString::from(format!("found-{}", found.base_url)),
                format!("Add {}", found.name),
                &t,
            )
            .border_color(t.border)
            .bg(t.bg_panel)
            .text_color(t.text)
            .tooltip(crate::ui::tooltip(found.base_url.clone()))
            .child(provider_logo(preset_of(found), None, 22.))
            .child(div().max_w_full().truncate().child(found.name.clone()))
            .on_click(cx.listener(move |d, _, window, cx| {
                d.open_form(provider.clone(), false, window, cx);
            }))
            .into_any_element(),
        );
    }
    let open = saved
        .iter()
        .find(|p| selected.as_deref() == Some(p.provider.id.as_str()))
        .cloned();
    let rows = open
        .map(|view| d.server_row(&view, window, cx))
        .unwrap_or_default();

    div()
        .flex()
        .flex_col()
        .when(!tiles.is_empty(), |c| {
            c.child(tile_grid("Local providers").children(tiles))
        })
        .children(rows)
        .when(empty, |c| {
            c.child(
                div()
                    .py(px(24.))
                    .flex()
                    .justify_center()
                    .text_color(t.text_muted)
                    .child("No local providers available"),
            )
        })
        .into_any_element()
}
