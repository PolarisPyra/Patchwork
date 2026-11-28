//! Settings → Providers drawn.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Window, div, prelude::*, px};
use patchwork_model::provider::api::ProviderView;
use patchwork_model::provider::AgentKind;
use crate::settings::SettingsDialog;
use crate::settings::controls::{Section, alert, token_row};
use patchwork_design_theme::theme;
use super::cli;
use super::form;
use super::local;

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
    let vision = d
        .settings
        .agents
        .contains(&AgentKind::Api)
        .then(|| token_row("Vision model", &t).child(d.vision_picker(cx)));
    let presets = d.preset_grid(cx);
    let local = local::render(d, window, cx);
    let cloud = (!cloud_rows.is_empty() || vision.is_some()).then(|| {
        Section::new("API providers")
            .children(cloud_rows)
            .children(vision)
    });
    div()
        .flex()
        .flex_col()
        .children(
            d.providers
                .error
                .clone()
                .map(|e| alert("providers-error", e, t.danger).pb(px(12.))),
        )
        .child(Section::new("Agent CLIs").children(cli_rows))
        .children(cloud)
        .child(Section::new("Add a provider").bare().child(presets))
        .child(Section::new("Local providers").bare().child(local))
        .into_any_element()
}
