//! A provider's editor (`ProviderForm` in `providers/ApiProvidersPane.tsx`): its name, address,
//! key, models and headers, with Save, Cancel and Remove. It takes the pane while it is open.

use std::io::Cursor;

use base64::Engine;
use gpui::{
    AnyElement, Context, Div, Focusable, FontWeight, PathPromptOptions, Role, SharedString,
    Toggled, Window, div, prelude::*, px,
};
use patchwork_server::provider::api::config::{
    self, ApiModel, ApiProvider, Header, KeyChange, KeyStore, ModelsQuery, ProviderView,
};

use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert};
use crate::ui::theme::{FONT_MONO, FS_SM, RADIUS_SM, Theme, theme};
use crate::ui::{BtnKind, btn, button, icon, icon_button, provider_logo, spinner};

/// Cloud endpoints to start a provider from: id, name, address, and the models of one that has no
/// list to load. The id names the key's environment variable and picks the logo.
pub const PRESETS: &[(&str, &str, &str, &[&str])] = &[
    ("deepseek", "DeepSeek", "https://api.deepseek.com", &[]),
    ("openai", "OpenAI", "https://api.openai.com/v1", &[]),
    (
        "gemini",
        "Gemini",
        "https://generativelanguage.googleapis.com/v1beta/openai",
        &[],
    ),
    ("xai", "Grok", "https://api.x.ai/v1", &[]),
    ("mistral", "Mistral", "https://api.mistral.ai/v1", &[]),
    ("zai", "Z.ai", "https://api.z.ai/api/paas/v4", &[]),
    ("llama", "Meta", "https://api.llama.com/compat/v1", &[]),
    ("moonshot", "Kimi", "https://api.moonshot.ai/v1", &[]),
    (
        "dashscope",
        "Qwen",
        "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        &[],
    ),
    ("minimax", "MiniMax", "https://api.minimax.io/v1", &[]),
    ("groq", "Groq", "https://api.groq.com/openai/v1", &[]),
    (
        "together",
        "Together AI",
        "https://api.together.xyz/v1",
        &[],
    ),
    (
        "fireworks",
        "Fireworks AI",
        "https://api.fireworks.ai/inference/v1",
        &[],
    ),
    ("cerebras", "Cerebras", "https://api.cerebras.ai/v1", &[]),
    (
        "openrouter",
        "OpenRouter",
        "https://openrouter.ai/api/v1",
        &[],
    ),
    (
        "perplexity",
        "Perplexity",
        "https://api.perplexity.ai",
        &["sonar", "sonar-pro", "sonar-reasoning-pro"],
    ),
];

/// A new provider from a preset; "custom" starts with no name.
pub fn blank(preset: &str, name: &str, base_url: &str, models: &[&str]) -> ApiProvider {
    ApiProvider {
        id: String::new(),
        name: if preset == "custom" {
            String::new()
        } else {
            name.to_string()
        },
        preset: preset.to_string(),
        base_url: base_url.to_string(),
        enabled: true,
        local: false,
        models: models
            .iter()
            .map(|id| ApiModel {
                id: id.to_string(),
                name: None,
            })
            .collect(),
        headers: Vec::new(),
        key_env: None,
        key_store: None,
        logo: None,
        system_prompt: String::new(),
    }
}

/// "3 models · key in system keychain".
pub fn describe(view: &ProviderView) -> String {
    let p = &view.provider;
    let models = match p.models.len() {
        0 => "No models".to_string(),
        1 | 2 => p
            .models
            .iter()
            .map(|m| m.name.clone().unwrap_or_else(|| m.id.clone()))
            .collect::<Vec<_>>()
            .join(", "),
        n => format!("{n} models"),
    };
    let key = match (&p.key_env, p.key_store) {
        (Some(env), _) => format!("key from ${env}"),
        (None, Some(KeyStore::Keychain)) => "key in system keychain".into(),
        (None, Some(KeyStore::File)) => "key in Patchwork's config folder".into(),
        (None, None) => "no key".into(),
    };
    format!("{models} · {key}")
}

/// The size a chosen logo is scaled to fit, so the saved image stays small.
const LOGO_PX: u32 = 128;

/// An image file scaled to fit `LOGO_PX` square, as a PNG `data:` URL.
fn logo_data_url(path: &std::path::Path) -> Result<String, String> {
    let unreadable = |_| "That file is not an image Patchwork can read".to_string();
    let image = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .decode()
        .map_err(unreadable)?;
    let image = if image.width() > LOGO_PX || image.height() > LOGO_PX {
        image.thumbnail(LOGO_PX, LOGO_PX)
    } else {
        image
    };
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

/// The editor's state: the provider as edited and everything typed around it.
pub struct Form {
    /// Names this form's fields, so the next form starts from its own values.
    prefix: String,
    draft: ApiProvider,
    has_key: bool,
    key: String,
    show_key: bool,
    /// The stored key, fetched when the eye first shows it. It is shown, not edited.
    saved_key: Option<String>,
    clear_key: bool,
    use_env: bool,
    advanced: bool,
    busy: bool,
    error: Option<String>,
    confirm_remove: bool,
    /// The endpoint's own model list, once fetched.
    remote: Option<Vec<ApiModel>>,
    loading: bool,
    models_error: Option<String>,
    query: String,
}

impl Form {
    fn field(&self, name: &str) -> String {
        format!("{}-{name}", self.prefix)
    }

    /// The key change Save sends. A variable replaces a saved key, so the saved one goes.
    fn key_change(&self) -> KeyChange {
        if self.use_env {
            if self.draft.key_store.is_some() {
                KeyChange::Clear
            } else {
                KeyChange::Keep
            }
        } else if self.clear_key {
            KeyChange::Clear
        } else if !self.key.trim().is_empty() {
            KeyChange::Set {
                key: self.key.clone(),
            }
        } else {
            KeyChange::Keep
        }
    }

    fn add_model(&mut self, id: &str) {
        let id = id.trim();
        if !id.is_empty() && !self.draft.models.iter().any(|m| m.id == id) {
            self.draft.models.push(ApiModel {
                id: id.to_string(),
                name: None,
            });
        }
    }
}

impl SettingsDialog {
    pub(super) fn open_form(
        &mut self,
        provider: ApiProvider,
        has_key: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        static FORMS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = FORMS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let use_env = provider.key_env.is_some();
        let advanced = !provider.headers.is_empty();
        self.providers.form = Some(Form {
            prefix: format!("provider-form-{n}"),
            draft: provider,
            has_key,
            key: String::new(),
            show_key: false,
            saved_key: None,
            clear_key: false,
            use_env,
            advanced,
            busy: false,
            error: None,
            confirm_remove: false,
            remote: None,
            loading: false,
            models_error: None,
            query: String::new(),
        });
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        // The name takes focus when it is empty, and the key when there is none yet, as the web
        // form's `autoFocus` does.
        let _ = render(self, window, cx);
        let first = self.providers.form.as_ref().and_then(|f| {
            if f.draft.name.is_empty() {
                Some(f.field("name"))
            } else {
                (!f.has_key).then(|| f.field("key"))
            }
        });
        if let Some(area) = first.and_then(|k| self.field_area(&k)) {
            let focus = area.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    fn close_form(&mut self, reload: bool, cx: &mut Context<Self>) {
        if let Some(form) = self.providers.form.take() {
            self.drop_fields(&form.prefix);
        }
        if reload {
            self.reload_providers(cx);
        }
        cx.notify();
    }

    fn form_mut(&mut self) -> Option<&mut Form> {
        self.providers.form.as_mut()
    }

    fn save_form(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form_mut() else {
            return;
        };
        form.busy = true;
        form.error = None;
        let mut provider = form.draft.clone();
        if form.use_env {
            provider.key_env = Some(provider.key_env.unwrap_or_default());
        } else {
            provider.key_env = None;
        }
        let change = form.key_change();
        self.run(
            move || config::save(provider, change),
            |d, saved, cx| match saved {
                Ok(_) => d.close_form(true, cx),
                Err(e) => {
                    if let Some(f) = d.form_mut() {
                        f.busy = false;
                        f.error = Some(e);
                    }
                }
            },
            cx,
        );
    }

    fn remove_provider(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form_mut() else {
            return;
        };
        form.busy = true;
        let id = form.draft.id.clone();
        self.run(
            move || config::remove(&id),
            |d, removed, cx| match removed {
                Ok(()) => d.close_form(true, cx),
                Err(e) => {
                    if let Some(f) = d.form_mut() {
                        f.busy = false;
                        f.error = Some(e);
                    }
                }
            },
            cx,
        );
    }

    fn toggle_show_key(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form_mut() else {
            return;
        };
        form.show_key = !form.show_key;
        let (show, field) = (form.show_key, form.field("key"));
        let fetch = show && form.has_key && form.key.is_empty() && form.saved_key.is_none();
        let id = form.draft.id.clone();
        if let Some(area) = self.field_area(&field) {
            area.update(cx, |a, cx| {
                a.masked = !show;
                cx.notify();
            });
        }
        if fetch {
            self.run(
                move || config::stored_key(&id),
                move |d, key, cx| {
                    let key = key.unwrap_or_default();
                    if let Some(area) = d.field_area(&field) {
                        area.update(cx, |a, cx| a.set_text(&key, cx));
                    }
                    if let Some(f) = d.form_mut() {
                        f.saved_key = Some(key);
                    }
                },
                cx,
            );
        }
        cx.notify();
    }

    fn fetch_models(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form_mut() else {
            return;
        };
        form.loading = true;
        form.models_error = None;
        let query = ModelsQuery {
            base_url: form.draft.base_url.clone(),
            headers: form.draft.headers.clone(),
            key: (!form.use_env && !form.key.trim().is_empty())
                .then(|| form.key.trim().to_string()),
            key_env: form
                .use_env
                .then(|| {
                    form.draft
                        .key_env
                        .clone()
                        .unwrap_or_default()
                        .trim()
                        .to_string()
                })
                .filter(|e| !e.is_empty()),
            provider_id: (!form.draft.id.is_empty()).then(|| form.draft.id.clone()),
        };
        self.run(
            move || config::fetch_models(query),
            |d, list, _| {
                if let Some(f) = d.form_mut() {
                    f.loading = false;
                    match list {
                        Ok(list) => f.remote = Some(list),
                        Err(e) => {
                            f.models_error = Some(e);
                            f.remote = None;
                        }
                    }
                }
            },
            cx,
        );
    }

    fn choose_logo(&mut self, cx: &mut Context<Self>) {
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
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |d, cx| {
                d.run(
                    move || logo_data_url(&path),
                    |d, logo, _| {
                        if let Some(f) = d.form_mut() {
                            match logo {
                                Ok(logo) => f.draft.logo = Some(logo),
                                Err(e) => f.error = Some(e),
                            }
                        }
                    },
                    cx,
                )
            });
        })
        .detach();
    }

    /// A live field of the form, saved into it by `set`.
    #[allow(clippy::too_many_arguments)]
    fn form_field(
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

    fn models_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
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
                let remove = id.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .h(px(24.))
                    .rounded(px(RADIUS_SM))
                    .bg(t.bg_selected)
                    .pl(px(8.))
                    .pr(px(2.))
                    .font_family(FONT_MONO)
                    .text_size(px(FS_SM))
                    .child(id.clone())
                    .child(
                        button(
                            SharedString::from(format!("remove-model-{id}")),
                            format!("Remove {id}"),
                        )
                        .size(px(20.))
                        .justify_center()
                        .rounded(px(RADIUS_SM))
                        .text_color(muted)
                        .hover(move |s| s.bg(hover).text_color(fg))
                        .child(icon("x", 11.))
                        .on_click(cx.listener(move |d, _, _, cx| {
                            if let Some(f) = d.form_mut() {
                                f.draft.models.retain(|m| m.id != remove);
                            }
                            cx.notify();
                        })),
                    )
            })
            .collect();
        let list = remote.map(|remote| {
            let shown: Vec<ApiModel> = remote
                .into_iter()
                .filter(|m| query.is_empty() || m.id.to_lowercase().contains(&query))
                .collect();
            let rows: Vec<_> = shown
                .into_iter()
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
                        .font_family(FONT_MONO)
                        .text_size(px(FS_SM))
                        .hover(move |s| s.bg(hover))
                        .child(
                            div()
                                .flex_none()
                                .size(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(RADIUS_SM))
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
                .rounded(px(RADIUS_SM))
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
                                    .text_size(px(FS_SM))
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
                    .rounded(px(RADIUS_SM))
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

    fn headers_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
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

/// A field's name over it.
fn label_text(text: &str, t: &Theme) -> Div {
    div()
        .text_size(px(FS_SM))
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
                .rounded(px(RADIUS_SM))
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
                            .text_size(px(FS_SM))
                            .text_color(t.accent)
                            .hover(|s| s.underline())
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
                .text_size(px(FS_SM))
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
