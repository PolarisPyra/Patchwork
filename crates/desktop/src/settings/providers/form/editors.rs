//! The form's fields: a text field, the list of models, and the extra headers.

// Rust guideline compliant 2026-10-09

use gpui::{Div, Role, SharedString, Toggled, Window, div, prelude::*, px};
use patchwork_server::provider::api::config::Header;

use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert};
use crate::ui::theme::{font_mono, fs_sm, radius_sm, theme};
use crate::ui::{BtnKind, btn, button, icon, icon_button, spinner};
use super::render::{label_text};
use super::state::{Form};

impl SettingsDialog {
    /// A live field of the form, saved into it by `set`.
    #[expect(
        clippy::too_many_arguments,
        reason = "each argument is a separate piece of what it draws"
    )]
    pub(super) fn form_field(
        &mut self,
        name: &str,
        label: &str,
        value: &str,
        placeholder: &str,
        kind: FieldKind,
        set: impl Fn(&mut Form, String) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(key) = self.providers.form.as_ref().map(|f| f.field(name)) else {
            return div();
        };
        self.field(
            key,
            label.to_string(),
            value,
            placeholder,
            FieldKind { live: true, ..kind },
            move |d, text, cx| {
                if let Some(f) = d.form_mut() {
                    set(f, text);
                }
                cx.notify();
            },
            window,
            cx,
        )
        .w_full()
    }

    pub(super) fn models_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = *theme(cx);
        let Some(form) = &self.providers.form else {
            return div();
        };
        let (loading, base_empty, fetched) = (
            form.loading,
            form.draft.base_url.trim().is_empty(),
            form.remote.is_some(),
        );
        let chosen: Vec<String> = form.draft.models.iter().map(|m| m.id.clone()).collect();
        let placeholder = if chosen.is_empty() {
            "Add a model id, then Enter"
        } else {
            "Add a model"
        };
        let models_error = form.models_error.clone();
        let remote = form.remote.clone();
        let typed_query = form.query.clone();
        let query = form.query.trim().to_lowercase();
        let add_key = form.field("add-model");
        let add = self.form_field(
            "add-model",
            "Add model id",
            "",
            placeholder,
            FieldKind {
                mono: true,
                ..Default::default()
            },
            |f, text| {
                // A comma ends an id, as Enter does.
                if let Some(id) = text.strip_suffix(',') {
                    f.add_model(id);
                }
            },
            window,
            cx,
        );
        if let Some(area) = self.field_area(&add_key)
            && area.read(cx).content.ends_with(',')
        {
            area.update(cx, |a, cx| a.set_text("", cx));
        }
        let clear_key = add_key.clone();
        self.on_submit(&add_key, move |d, text, cx| {
            if let Some(f) = d.form_mut() {
                f.add_model(&text);
            }
            if let Some(area) = d.field_area(&clear_key) {
                area.update(cx, |a, cx| a.set_text("", cx));
            }
            cx.notify();
        });
        // Backspace in the empty field takes back the last model added.
        self.on_back(&add_key, |d, cx| {
            if let Some(f) = d.form_mut() {
                f.draft.models.pop();
            }
            cx.notify();
        });
        let search = remote.as_ref().filter(|r| r.len() > 8).map(|r| {
            let n = r.len();
            self.form_field(
                "search-models",
                "Search models",
                &typed_query,
                &format!("Search {n} models"),
                FieldKind::default(),
                |f, text| f.query = text,
                window,
                cx,
            )
        });
        let (hover, muted, fg) = (t.bg_hover, t.text_muted, t.text);
        let chips: Vec<_> = chosen
            .iter()
            .map(|id| {
                let model_id = id.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(24.))
                    .rounded(px(radius_sm()))
                    .bg(t.bg_selected)
                    .pl(px(8.))
                    .pr(px(2.))
                    .font_family(font_mono())
                    .text_size(px(fs_sm()))
                    .child(id.clone())
                    .child(
                        button(
                            SharedString::from(format!("remove-model-{id}")),
                            format!("Remove {id}"),
                        )
                        .size(px(20.))
                        .justify_center()
                        .rounded(px(radius_sm()))
                        .text_color(muted)
                        .hover(move |s| s.bg(hover).text_color(fg))
                        .child(icon("x", 11.))
                        .on_click(cx.listener(move |d, _, _, cx| {
                            if let Some(f) = d.form_mut() {
                                f.draft.models.retain(|m| m.id != model_id);
                            }
                            cx.notify();
                        })),
                    )
            })
            .collect();
        let list = remote.map(|remote| {
            let rows: Vec<_> = remote
                .into_iter()
                .filter(|m| query.is_empty() || m.id.to_lowercase().contains(&query))
                .map(|m| {
                    let on = chosen.contains(&m.id);
                    let id = m.id.clone();
                    div()
                        .id(SharedString::from(format!("model-{id}")))
                        .role(Role::CheckBox)
                        .aria_label(id.clone())
                        .aria_toggled(if on { Toggled::True } else { Toggled::False })
                        .tab_index(0)
                        .focus_visible(crate::ui::focus_ring)
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .w_full()
                        .px(px(8.))
                        .py(px(4.))
                        .font_family(font_mono())
                        .text_size(px(fs_sm()))
                        .hover(move |s| s.bg(hover))
                        .child(
                            div()
                                .flex_none()
                                .size(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(radius_sm()))
                                .border_1()
                                .text_color(t.text_on_accent)
                                .map(|b| {
                                    if on {
                                        b.border_color(t.accent)
                                            .bg(t.accent)
                                            .child(icon("check", 11.))
                                    } else {
                                        b.border_color(t.border_strong)
                                    }
                                }),
                        )
                        .child(div().min_w_0().truncate().child(m.id.clone()))
                        .on_click(cx.listener(move |d, _, _, cx| {
                            if let Some(f) = d.form_mut() {
                                if f.draft.models.iter().any(|m| m.id == id) {
                                    f.draft.models.retain(|m| m.id != id);
                                } else {
                                    f.add_model(&id);
                                }
                            }
                            cx.notify();
                        }))
                })
                .collect();
            let empty = rows.is_empty();
            div()
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(t.border)
                .children(search.map(|s| {
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .border_b_1()
                        .border_color(t.border)
                        .px(px(8.))
                        .py(px(4.))
                        .text_color(t.text_subtle)
                        .child(icon("magnifying-glass", 14.))
                        .child(s)
                }))
                .child(
                    div()
                        .id("remote-models")
                        .role(Role::List)
                        .aria_label("Models the endpoint serves")
                        .max_h(px(180.))
                        .overflow_y_scroll()
                        .when(empty, |l| {
                            l.child(
                                div()
                                    .px(px(8.))
                                    .py(px(6.))
                                    .text_size(px(fs_sm()))
                                    .text_color(muted)
                                    .child("No models"),
                            )
                        })
                        .children(rows),
                )
        });
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label_text("Models", &t))
                    .child(
                        btn(
                            "fetch-models",
                            "Fetch models",
                            BtnKind::Plain,
                            loading || base_empty,
                            &t,
                        )
                        .child(if loading {
                            spinner(13., t.text_muted).into_any_element()
                        } else {
                            icon("arrow-clockwise", 13.).into_any_element()
                        })
                        .child(if fetched {
                            "Refresh list"
                        } else {
                            "Fetch models"
                        })
                        .when(!loading && !base_empty, |b| {
                            b.on_click(cx.listener(|d, _, _, cx| d.fetch_models(cx)))
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(radius_sm()))
                    .border_1()
                    .border_color(t.border_strong)
                    .bg(t.bg_editor)
                    .p(px(6.))
                    .children(chips)
                    .child(
                        div().flex_1().min_w(px(140.)).child(
                            add.border_0()
                                .bg(gpui::transparent_black())
                                .px(px(4.))
                                .h(px(24.)),
                        ),
                    ),
            )
            .children(models_error.map(|e| alert("models-error", e, t.danger)))
            .children(list)
    }

    pub(super) fn headers_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = *theme(cx);
        let headers: Vec<Header> = self
            .providers
            .form
            .as_ref()
            .map(|f| f.draft.headers.clone())
            .unwrap_or_default();
        let mut rows = Vec::new();
        for (i, h) in headers.iter().enumerate() {
            let name = self.form_field(
                &format!("header-{i}-name"),
                &format!("Header {} name", i + 1),
                &h.name,
                "Header",
                FieldKind {
                    mono: true,
                    ..Default::default()
                },
                move |f, text| {
                    if let Some(h) = f.draft.headers.get_mut(i) {
                        h.name = text;
                    }
                },
                window,
                cx,
            );
            let value = self.form_field(
                &format!("header-{i}-value"),
                &format!("Header {} value", i + 1),
                &h.value,
                "Value",
                FieldKind {
                    mono: true,
                    ..Default::default()
                },
                move |f, text| {
                    if let Some(h) = f.draft.headers.get_mut(i) {
                        h.value = text;
                    }
                },
                window,
                cx,
            );
            let label = if h.name.is_empty() {
                (i + 1).to_string()
            } else {
                h.name.clone()
            };
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(name)
                    .child(value)
                    .child(
                        icon_button(
                            SharedString::from(format!("remove-header-{i}")),
                            format!("Remove header {label}"),
                            "x",
                            14.,
                            cx,
                        )
                        .on_click(cx.listener(move |d, _, _, cx| {
                            if let Some(f) = d.form_mut()
                                && i < f.draft.headers.len()
                            {
                                f.draft.headers.remove(i);
                            }
                            cx.notify();
                        })),
                    ),
            );
        }
        div().flex().flex_col().gap(px(6.)).children(rows).child(
            div().flex().child(
                btn("add-header", "Add header", BtnKind::Plain, false, &t)
                    .child(icon("plus", 13.))
                    .child("Add header")
                    .on_click(cx.listener(|d, _, _, cx| {
                        if let Some(f) = d.form_mut() {
                            f.draft.headers.push(Header {
                                name: String::new(),
                                value: String::new(),
                            });
                        }
                        cx.notify();
                    })),
            ),
        )
    }
}
