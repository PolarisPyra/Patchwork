//! The agents' and providers' logos, and the API providers' names.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use base64::Engine;
use gpui::{AnyElement, Image, ImageFormat, IntoElement, SharedString, Styled, img, px};
use patchwork_server::provider::AgentKind;
use patchwork_server::provider::api::config::ApiProvider;
use super::controls::{icon, mono_svg};

/// An agent's mark. A one-colour mark takes the text colour.
/// An API chat shows its provider's preset mark, found from the `provider/model` id.
pub fn agent_logo(kind: AgentKind, model: Option<&str>, size: f32) -> AnyElement {
    let (file, mono) = match kind {
        AgentKind::Claude => ("claude-color.svg", false),
        AgentKind::Codex => ("openai.svg", true),
        AgentKind::Antigravity => ("gemini-color.svg", false),
        AgentKind::OpenCode => ("opencode.svg", true),
        AgentKind::Api => {
            let provider = model.and_then(api_provider_of);
            return provider_logo(
                provider.as_ref().map_or("", |p| p.preset.as_str()),
                provider.as_ref().and_then(|p| p.logo.as_deref()),
                size,
            );
        }
    };
    logo_file(file, mono, size)
}

pub(super) fn logo_file(file: &str, mono: bool, size: f32) -> AnyElement {
    let path = SharedString::from(format!("logos/{file}"));
    if mono {
        mono_svg(path, size).into_any_element()
    } else {
        img(path).size(px(size)).flex_none().into_any_element()
    }
}

/// An API provider's mark: the picture it was given, else its preset's, else a plug.
pub fn provider_logo(preset: &str, logo: Option<&str>, size: f32) -> AnyElement {
    if let Some(image) = logo.and_then(logo_image) {
        return img(image).size(px(size)).flex_none().into_any_element();
    }
    match preset_logo(preset) {
        Some((file, mono)) => logo_file(file, mono, size),
        None => icon("plugs-connected", size).into_any_element(),
    }
}

/// A `data:image/…;base64,` logo as an image, decoded once.
pub(super) fn logo_image(url: &str) -> Option<Arc<Image>> {
    static DECODED: OnceLock<Mutex<HashMap<String, Arc<Image>>>> = OnceLock::new();
    let decoded = DECODED.get_or_init(Default::default);
    let mut decoded = decoded.lock().ok()?;
    if let Some(image) = decoded.get(url) {
        return Some(Arc::clone(image));
    }
    let (head, data) = url.strip_prefix("data:")?.split_once(";base64,")?;
    let format = ImageFormat::from_mime_type(head)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    let image = Arc::new(Image::from_bytes(format, bytes));
    decoded.insert(url.to_string(), Arc::clone(&image));
    Some(image)
}

/// The API providers by id, read at the first API model drawn and again after Settings changes
/// them.
pub(super) fn api_providers() -> Arc<HashMap<String, ApiProvider>> {
    let mut cached = PROVIDERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Arc::clone(cached.get_or_insert_with(|| {
        Arc::new(
            patchwork_server::provider::api::config::providers()
                .into_iter()
                .map(|p| (p.id.clone(), p))
                .collect(),
        )
    }))
}

pub(super) static PROVIDERS: Mutex<Option<Arc<HashMap<String, ApiProvider>>>> = Mutex::new(None);

/// Reads the API providers again, after one was added, changed or removed.
pub fn reload_api_providers() {
    *PROVIDERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// The provider of a `provider/model` id (`apiProviderOf`).
pub(super) fn api_provider_of(model: &str) -> Option<ApiProvider> {
    api_providers().get(model.split('/').next()?).cloned()
}

/// The name of the API provider a `provider/model` id belongs to.
pub fn api_provider_name(model: &str) -> Option<String> {
    api_provider_of(model).map(|p| p.name)
}

/// Where a local provider's model runs: "this computer", or the machine its server is on.
pub fn api_model_place(model: &str) -> Option<String> {
    let provider = api_provider_of(model).filter(|p| p.local)?;
    let rest = provider.base_url.split("://").nth(1)?;
    let host = rest.split(['/', '?']).next()?;
    let host = match host.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host,
    };
    Some(match host {
        "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0" => "this computer".into(),
        other => other.into(),
    })
}

/// The mark of an API provider preset (`PRESET_LOGOS`).
pub(super) fn preset_logo(preset: &str) -> Option<(&'static str, bool)> {
    Some(match preset {
        "deepseek" => ("deepseek-color.svg", false),
        "openai" => ("openai.svg", true),
        "gemini" => ("gemini-color.svg", false),
        "xai" => ("grok.svg", true),
        "mistral" => ("mistral-color.svg", false),
        "zai" => ("zai.svg", true),
        "llama" => ("meta-color.svg", false),
        "moonshot" => ("kimi.svg", true),
        "dashscope" => ("qwen-color.svg", false),
        "minimax" => ("minimax-color.svg", false),
        "groq" => ("groq.svg", true),
        "together" => ("together-color.svg", false),
        "fireworks" => ("fireworks-color.svg", false),
        "cerebras" => ("cerebras.svg", true),
        "openrouter" => ("openrouter.svg", true),
        "perplexity" => ("perplexity-color.svg", false),
        "lmstudio" => ("lmstudio.svg", true),
        _ => return None,
    })
}

/// The agents in the order the pickers and the task list show them (`AGENT_KINDS`).
pub const AGENT_KINDS: [AgentKind; 5] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Antigravity,
    AgentKind::OpenCode,
    AgentKind::Api,
];

/// The agent's name, as `AGENT_LABELS` gives it.
pub fn agent_label(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "Claude Code",
        AgentKind::Codex => "Codex",
        AgentKind::Antigravity => "Antigravity CLI",
        AgentKind::OpenCode => "OpenCode CLI",
        AgentKind::Api => "API",
    }
}
