//! A local server drawn as a row, with its address field and its repair row.

// Rust guideline compliant 2026-10-09

use super::state::health_line;
use super::state::ADDRESS_FIELD;
use gpui::{AnyElement, Div, Focusable, FontWeight, SharedString, Window, div, prelude::*, px};
use patchwork_server::provider::api::config::{self, ApiProvider, KeyChange, ProviderView};
use patchwork_server::provider::api::servers;
use crate::settings::providers::PromptOf;
use crate::settings::providers::rows::status_badge;
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert, switch};
use crate::ui::theme::{font_mono, fs_sm, radius_sm, theme};
use crate::ui::{BtnKind, btn, icon, provider_logo, spinner};

impl SettingsDialog {
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
        .font_family(font_mono())
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
    pub(super) fn server_row(
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
                        .text_size(px(fs_sm()))
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
                .rounded(px(radius_sm()))
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
