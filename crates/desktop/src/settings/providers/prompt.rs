//! A provider's system prompt, and its editor.

// Rust guideline compliant 2026-10-09

use gpui::{Div, Focusable, Window, div, prelude::*, px};
use patchwork_server::provider::api::config::{self, ApiProvider, KeyChange};
use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert};
use crate::ui::theme::theme;
use crate::ui::{BtnKind, agent_label, btn};
use super::state::{PROMPT_FIELD, Prompt, PromptOf};

impl SettingsDialog {
    /// Opens a system prompt, or closes it when it is the one open.
    pub(super) fn toggle_prompt(
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

    pub(super) fn prompt_field(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
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

    pub(super) fn provider_name(&self, id: &str) -> String {
        self.providers
            .providers
            .iter()
            .find(|p| p.provider.id == id)
            .map_or_else(|| id.to_string(), |p| p.provider.name.clone())
    }

    pub(super) fn save_prompt(&mut self, cx: &mut Context<Self>) {
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
                                p.error = Some(e.to_string());
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
    pub(super) fn prompt_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
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
}
