//! The API provider form's state, and what it saves, fetches and removes.

// Rust guideline compliant 2026-10-09

use std::io::Cursor;

use base64::Engine;
use gpui::{Focusable, PathPromptOptions, Window, prelude::*, px};
use patchwork_server::provider::api::config::{
    self, ApiModel, ApiProvider, KeyChange, KeyStore, ModelsQuery, ProviderView,
};

use crate::settings::SettingsDialog;
use super::render::{render};

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
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate piece of view state"
)]
pub struct Form {
    /// Names this form's fields, so the next form starts from its own values.
    prefix: String,
    pub(super) draft: ApiProvider,
    pub(super) has_key: bool,
    pub(super) key: String,
    pub(super) show_key: bool,
    /// The stored key, fetched when the eye first shows it. It is shown, not edited.
    pub(super) saved_key: Option<String>,
    pub(super) clear_key: bool,
    pub(super) use_env: bool,
    pub(super) advanced: bool,
    pub(super) busy: bool,
    pub(super) error: Option<String>,
    pub(super) confirm_remove: bool,
    /// The endpoint's own model list, once fetched.
    pub(super) remote: Option<Vec<ApiModel>>,
    pub(super) loading: bool,
    pub(super) models_error: Option<String>,
    pub(super) query: String,
}

impl Form {
    pub(super) fn field(&self, name: &str) -> String {
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

    pub(super) fn add_model(&mut self, id: &str) {
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
    pub(in crate::settings::providers) fn open_form(
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

    /// Escape closes the open form, unless it is saving. False when no form is open.
    pub(in crate::settings) fn cancel_form(&mut self, cx: &mut Context<Self>) -> bool {
        match &self.providers.form {
            None => return false,
            Some(form) if form.busy => {}
            Some(_) => self.close_form(false, cx),
        }
        true
    }

    pub(super) fn close_form(&mut self, reload: bool, cx: &mut Context<Self>) {
        if let Some(form) = self.providers.form.take() {
            self.drop_fields(&form.prefix);
        }
        if reload {
            self.reload_providers(cx);
        }
        cx.notify();
    }

    pub(super) fn form_mut(&mut self) -> Option<&mut Form> {
        self.providers.form.as_mut()
    }

    pub(super) fn save_form(&mut self, cx: &mut Context<Self>) {
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
                        f.error = Some(e.to_string());
                    }
                }
            },
            cx,
        );
    }

    pub(super) fn remove_provider(&mut self, cx: &mut Context<Self>) {
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
                        f.error = Some(e.to_string());
                    }
                }
            },
            cx,
        );
    }

    pub(super) fn toggle_show_key(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn fetch_models(&mut self, cx: &mut Context<Self>) {
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
                            f.models_error = Some(e.to_string());
                            f.remote = None;
                        }
                    }
                }
            },
            cx,
        );
    }

    pub(super) fn choose_logo(&mut self, cx: &mut Context<Self>) {
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
                );
            });
        })
        .detach();
    }
}
