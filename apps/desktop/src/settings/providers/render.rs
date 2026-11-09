//! Settings → Providers drawn.

use gpui::{AnyElement, Context, Window, div, prelude::*};
use patchwork_server::provider::api::config::ProviderView;
use patchwork_server::provider::AgentKind;
use crate::settings::SettingsDialog;
use crate::settings::controls::{alert, section_heading, token_row};
use crate::ui::theme::theme;
use super::{cli};
use super::{form};
use super::{local};

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
    let connected = d.settings.agents.clone();
    let vision = connected
        .contains(&AgentKind::Api)
        .then(|| token_row("Vision model", None, &t).child(d.vision_picker(cx)));
    let default = (connected.len() > 1)
        .then(|| token_row("Default for new tasks", None, &t).child(d.default_picker(cx)));
    let presets = d.preset_grid(cx);
    let local = local::render(d, window, cx);
    div()
        .flex()
        .flex_col()
        .children(cli_rows)
        .children(cloud_rows)
        .children(vision)
        .children(default)
        .child(section_heading("Add a provider", false, &t))
        .child(presets)
        .child(section_heading("Local providers", false, &t))
        .child(local)
        .children(
            d.providers
                .error
                .clone()
                .map(|e| alert("providers-error", e, t.danger)),
        )
        .into_any_element()
}
