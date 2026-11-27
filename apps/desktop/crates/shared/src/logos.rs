//! The logos and names of agents, API providers and chat apps, from the core's data.
//!
//! The core says which logo each has; `patchwork_ui_theme::brand::logos` draws the file or mark.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gpui::{AnyElement, IntoElement};
use patchwork_backend::gateway::Platform;
use patchwork_backend::import::ImportSource;
use patchwork_backend::provider::api::ApiProvider;
use patchwork_backend::provider::{AgentKind, Logo};
use patchwork_ui_theme::brand::logo_file;
use patchwork_ui_theme::{icon, provider_logo};

/// An agent's mark. A one-colour mark takes the text colour.
/// An API chat shows its provider's preset mark, found from the `provider/model` id.
pub fn agent_logo(kind: AgentKind, model: Option<&str>, size: f32) -> AnyElement {
    match kind.identity().logo {
        Logo::File { file, mono } => logo_file(file, mono, size),
        Logo::Icon(name) => icon(name, size).into_any_element(),
        Logo::ApiProvider => {
            let provider = model.and_then(api_provider_of);
            provider_logo(
                provider.as_ref().map_or("", |p| p.preset.as_str()),
                provider.as_ref().and_then(|p| p.logo.as_deref()),
                size,
            )
        }
    }
}

/// A chat app's logo, or an icon for one without a logo of its own.
#[must_use]
pub fn platform_logo(platform: Platform, size: f32) -> AnyElement {
    let (file, mono) = match platform {
        Platform::Telegram => ("telegram-color.svg", false),
        Platform::Discord => ("discord-color.svg", false),
        Platform::Slack => ("slack-color.svg", false),
        Platform::Matrix => ("matrix.svg", true),
        Platform::Mattermost => ("mattermost-color.svg", false),
        Platform::Signal => ("signal-color.svg", false),
        Platform::Qq => ("qq-color.svg", false),
    };
    logo_file(file, mono, size)
}

/// The API providers by id, read at the first API model drawn and again after Settings changes
/// them.
fn api_providers() -> Arc<HashMap<String, ApiProvider>> {
    let mut cached = PROVIDERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Arc::clone(cached.get_or_insert_with(|| {
        Arc::new(
            patchwork_backend::provider::api::providers()
                .into_iter()
                .map(|p| (p.id.clone(), p))
                .collect(),
        )
    }))
}

static PROVIDERS: Mutex<Option<Arc<HashMap<String, ApiProvider>>>> = Mutex::new(None);

/// Reads the API providers again, after one was added, changed or removed.
pub fn reload_api_providers() {
    *PROVIDERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// The provider of a `provider/model` id (`apiProviderOf`).
fn api_provider_of(model: &str) -> Option<ApiProvider> {
    api_providers().get(model.split('/').next()?).cloned()
}

/// The name of the API provider a `provider/model` id belongs to.
#[must_use]
pub fn api_provider_name(model: &str) -> Option<String> {
    api_provider_of(model).map(|p| p.name)
}

/// Where a local provider's model runs: "this computer", or the machine its server is on.
#[must_use]
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

/// The agents in the order the pickers and the task list show them (`AGENT_KINDS`).
pub const AGENT_KINDS: [AgentKind; 8] = patchwork_backend::provider::DISPLAY_ORDER;

/// The agent's name, as `AGENT_LABELS` gives it.
#[must_use]
pub fn agent_label(kind: AgentKind) -> &'static str {
    kind.identity().title
}

/// The name a thread-import source shows: its agent's, except OpenCode's files are plain "OpenCode".
#[must_use]
pub fn source_label(source: ImportSource) -> &'static str {
    match source {
        ImportSource::OpenCode => "OpenCode",
        _ => source.agent().map_or("", agent_label),
    }
}
