//! The API provider form drawn.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Div, FontWeight, Role, Toggled, Window, div, prelude::*, px};
use patchwork_server::provider::api::config::KeyStore;

use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert};
use crate::ui::theme::{fs_sm, radius_sm, Theme, theme};
use crate::ui::{BtnKind, btn, button, icon, icon_button, provider_logo};

/// A field's name over it.
pub(super) fn label_text(text: &str, t: &Theme) -> Div {
    div()
        .text_size(px(fs_sm()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.text_muted)
        .child(text.to_string())
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let Some(form) = &d.providers.form else {
        return div().into_any_element();
    };
    let draft = form.draft.clone();
    let (use_env, show_key, has_key, clear_key, advanced, busy, confirm) = (
        form.use_env,
        form.show_key,
        form.has_key,
        form.clear_key,
        form.advanced,
        form.busy,
        form.confirm_remove,
    );
    let key_value = if form.key.is_empty() {
        form.saved_key.clone().unwrap_or_default()
    } else {
        form.key.clone()
    };
    let typed_key = !form.key.is_empty();
    let error = form.error.clone();
    let saved = !draft.id.is_empty();
    let key_placeholder = if clear_key {
        "Key will be removed"
    } else {
        match (has_key, draft.key_store) {
            (true, Some(KeyStore::Keychain)) => "Saved in the system keychain",
            (true, Some(KeyStore::File)) => "Saved in Patchwork's config folder",
            _ => "sk-…",
        }
    };
    let env_placeholder = format!(
        "{}_API_KEY",
        if draft.preset == "custom" {
            "API".to_string()
        } else {
            draft.preset.to_uppercase().replace('-', "_")
        }
    );

    let name = d.form_field(
        "name",
        "Name",
        &draft.name,
        "",
        FieldKind::default(),
        |f, text| f.draft.name = text,
        window,
        cx,
    );
    let base = d.form_field(
        "base-url",
        "Base URL",
        &draft.base_url,
        "https://example.com/v1",
        FieldKind::default(),
        |f, text| f.draft.base_url = text,
        window,
        cx,
    );
    let key_field = if use_env {
        d.form_field(
            "key-env",
            "API key environment variable",
            draft.key_env.as_deref().unwrap_or_default(),
            &env_placeholder,
            FieldKind {
                mono: true,
                ..Default::default()
            },
            |f, text| f.draft.key_env = Some(text),
            window,
            cx,
        )
    } else {
        d.form_field(
            "key",
            "API key",
            &key_value,
            key_placeholder,
            FieldKind {
                mono: true,
                masked: !show_key,
                ..Default::default()
            },
            |f, text| {
                // The stored key put in to be seen is not a new key.
                if f.saved_key.as_deref() == Some(text.as_str()) {
                    return;
                }
                f.key = text;
                f.saved_key = None;
                f.clear_key = false;
            },
            window,
            cx,
        )
    };
    let models = d.models_editor(window, cx);
    let headers = advanced.then(|| d.headers_editor(window, cx));
    let (hover, muted, fg) = (t.bg_hover, t.text_muted, t.text);
    let can_save = !busy && !draft.name.trim().is_empty() && !draft.base_url.trim().is_empty();

    let mark = if draft.preset == "custom" {
        let logo = draft.logo.clone();
        div()
            .group("logo")
            .relative()
            .flex_none()
            .child(
                button(
                    "choose-logo",
                    if logo.is_some() {
                        "Change logo"
                    } else {
                        "Choose logo"
                    },
                )
                .size(px(32.))
                .justify_center()
                .rounded(px(radius_sm()))
                .border_1()
                .border_dashed()
                .border_color(if logo.is_some() {
                    gpui::transparent_black()
                } else {
                    t.border_strong
                })
                .text_color(muted)
                .hover(move |s| s.border_color(t.accent).text_color(fg))
                .tooltip(crate::ui::tooltip(if logo.is_some() {
                    "Change logo"
                } else {
                    "Choose logo"
                }))
                .child(match &logo {
                    Some(logo) => provider_logo("custom", Some(logo), 20.),
                    None => icon("image", 16.).into_any_element(),
                })
                .on_click(cx.listener(|d, _, _, cx| d.choose_logo(cx))),
            )
            .when(logo.is_some(), |m| {
                m.child(
                    button("remove-logo", "Remove logo")
                        .absolute()
                        .top(px(-6.))
                        .right(px(-6.))
                        .size(px(16.))
                        .justify_center()
                        .rounded_full()
                        .border_1()
                        .border_color(t.border_strong)
                        .bg(t.bg_elevated)
                        .text_color(muted)
                        .opacity(0.)
                        .group_hover("logo", |s| s.opacity(1.))
                        .focus(|s| s.opacity(1.))
                        .hover(move |s| s.text_color(fg))
                        .tooltip(crate::ui::tooltip("Remove logo"))
                        .child(icon("x", 9.))
                        .on_click(cx.listener(|d, _, _, cx| {
                            if let Some(f) = d.form_mut() {
                                f.draft.logo = None;
                            }
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    } else {
        provider_logo(&draft.preset, None, 20.)
    };

    div()
        .id("provider-form")
        .role(Role::Form)
        .aria_label(if draft.name.is_empty() {
            "New provider".to_string()
        } else {
            draft.name.clone()
        })
        .flex()
        .flex_col()
        .gap(px(12.))
        .py(px(10.))
        .child(
            div().flex().items_center().gap(px(8.)).child(mark).child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if draft.name.is_empty() {
                        "New provider".to_string()
                    } else {
                        draft.name.clone()
                    }),
            ),
        )
        .child(
            div()
                .grid()
                .grid_cols(2)
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(label_text("Name", &t))
                        .child(name),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(label_text("Base URL", &t))
                        .child(base),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(label_text(
                            if use_env {
                                "Environment variable"
                            } else {
                                "API key"
                            },
                            &t,
                        ))
                        .child(
                            button(
                                "key-source",
                                if use_env {
                                    "Enter a key instead"
                                } else {
                                    "Use an environment variable"
                                },
                            )
                            .text_size(px(fs_sm()))
                            .text_color(t.accent)
                            .hover(gpui::Styled::underline)
                            .child(if use_env {
                                "Enter a key instead"
                            } else {
                                "Use an environment variable"
                            })
                            .on_click(cx.listener(|d, _, _, cx| {
                                if let Some(f) = d.form_mut() {
                                    f.use_env = !f.use_env;
                                }
                                cx.notify();
                            })),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .child(key_field)
                        .when(!use_env, |r| {
                            r.child(
                                icon_button(
                                    "show-key",
                                    "Show key",
                                    if show_key { "eye-slash" } else { "eye" },
                                    15.,
                                    cx,
                                )
                                .aria_toggled(if show_key {
                                    Toggled::True
                                } else {
                                    Toggled::False
                                })
                                .tooltip(crate::ui::tooltip(if show_key {
                                    "Hide key"
                                } else {
                                    "Show key"
                                }))
                                .on_click(cx.listener(|d, _, _, cx| d.toggle_show_key(cx))),
                            )
                            .when(
                                has_key && draft.key_store.is_some() && !typed_key,
                                |r| {
                                    r.child(
                                        icon_button("clear-key", "Remove saved key", "x", 15., cx)
                                            .aria_toggled(if clear_key {
                                                Toggled::True
                                            } else {
                                                Toggled::False
                                            })
                                            .when(clear_key, |b| b.bg(hover).text_color(t.danger))
                                            .tooltip(crate::ui::tooltip("Remove saved key"))
                                            .on_click(cx.listener(|d, _, _, cx| {
                                                if let Some(f) = d.form_mut() {
                                                    f.clear_key = !f.clear_key;
                                                }
                                                cx.notify();
                                            })),
                                    )
                                },
                            )
                        }),
                ),
        )
        .child(models)
        .child(
            button("headers-toggle", "Headers")
                .aria_expanded(advanced)
                .gap(px(4.))
                .text_size(px(fs_sm()))
                .text_color(muted)
                .hover(move |s| s.text_color(fg))
                .child(icon(
                    if advanced {
                        "caret-down"
                    } else {
                        "caret-right"
                    },
                    12.,
                ))
                .child("Headers")
                .on_click(cx.listener(|d, _, _, cx| {
                    if let Some(f) = d.form_mut() {
                        f.advanced = !f.advanced;
                    }
                    cx.notify();
                })),
        )
        .children(headers)
        .children(error.map(|e| alert("form-error", e, t.danger)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .border_t_1()
                .border_color(t.border)
                .pt(px(12.))
                .child(
                    btn("form-save", "Save", BtnKind::Primary, !can_save, &t)
                        .child(if busy { "Saving…" } else { "Save" })
                        .when(can_save, |b| {
                            b.on_click(cx.listener(|d, _, _, cx| d.save_form(cx)))
                        }),
                )
                .child(
                    btn("form-cancel", "Cancel", BtnKind::Plain, busy, &t)
                        .child("Cancel")
                        .when(!busy, |b| {
                            b.on_click(cx.listener(|d, _, _, cx| d.close_form(false, cx)))
                        }),
                )
                .when(saved, |r| {
                    r.child(div().flex_1()).child(
                        btn(
                            "form-remove",
                            if confirm {
                                "Remove provider and key"
                            } else {
                                "Remove"
                            },
                            BtnKind::Plain,
                            busy,
                            &t,
                        )
                        .when(confirm, |b| b.text_color(t.danger))
                        .child(if confirm {
                            "Remove provider and key"
                        } else {
                            "Remove"
                        })
                        .when(!busy, |b| {
                            b.on_click(cx.listener(move |d, _, _, cx| {
                                if confirm {
                                    d.remove_provider(cx);
                                } else if let Some(f) = d.form_mut() {
                                    f.confirm_remove = true;
                                }
                                cx.notify();
                            }))
                        }),
                    )
                }),
        )
        .into_any_element()
}
