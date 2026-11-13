//! Local model servers: what is known of each, finding them, and checking and repairing one.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use gpui::{Global, prelude::*};
use patchwork_server::provider::api::config::{
    self, ApiModel, ApiProvider, DiscoveredServer, ProviderHealth,
};
use crate::settings::providers::form;
use crate::settings::SettingsDialog;

/// The field a moved server's new address is typed in.
pub(super) const ADDRESS_FIELD: &str = "local-address";

/// Each saved server's last probe, kept while Settings is closed so reopening it shows the last
/// status at once and refreshes it in place, rather than every row going back to "Checking…".
#[derive(Default)]
pub(super) struct HealthCache(HashMap<String, ProviderHealth>);

impl Global for HealthCache {}

#[derive(Default)]
pub struct LocalState {
    /// The probe of each saved server, by id: missing while it is checked.
    pub(super) health: HashMap<String, ProviderHealth>,
    /// Servers answering on this computer, once looked for.
    pub(super) found: Option<Vec<DiscoveredServer>>,
    /// The saved server whose row is open.
    pub(super) selected: Option<String>,
    /// The new address being typed for the open server, when Change address is open.
    pub(super) address: Option<String>,
    pub(super) repairing: bool,
    pub(super) repair_error: Option<String>,
}

/// The preset a found server is saved as, which picks its logo: only LM Studio has one.
pub(super) fn preset_of(d: &DiscoveredServer) -> &'static str {
    if d.name == "LM Studio" {
        "lmstudio"
    } else {
        "custom"
    }
}

/// A custom provider prefilled from a server found on this computer, so Add opens the editor ready
/// to save.
pub(super) fn from_discovered(d: &DiscoveredServer) -> ApiProvider {
    ApiProvider {
        local: true,
        preset: preset_of(d).to_string(),
        name: d.name.clone(),
        base_url: d.base_url.clone(),
        models: d
            .models
            .iter()
            .map(|id| ApiModel {
                id: id.clone(),
                name: None,
            })
            .collect(),
        ..form::blank("custom", "", "", &[])
    }
}

/// One line under a server's name: what it serves right now, or why it is not answering.
pub(super) fn health_line(p: &ApiProvider, health: Option<&ProviderHealth>) -> String {
    let Some(h) = health else {
        return "Checking…".into();
    };
    if !h.reachable {
        return h.error.clone().unwrap_or_else(|| "Not answering".into());
    }
    let served = h.served_models.len();
    let missing = p
        .models
        .iter()
        .filter(|m| !h.served_models.contains(&m.id))
        .count();
    let mut parts = vec![format!(
        "{served} model{} served",
        if served == 1 { "" } else { "s" }
    )];
    if missing > 0 {
        parts.push(format!("{missing} configured not served"));
    }
    let reads = h.images.len();
    let vision = h.images.values().filter(|v| **v).count();
    if reads > 0 {
        parts.push(if vision == 0 {
            "text only".into()
        } else if vision == reads {
            "reads images".into()
        } else {
            format!("{vision} of {reads} read images")
        });
    }
    if let Some(ms) = h.latency_ms {
        parts.push(format!("{ms} ms"));
    }
    parts.join(" · ")
}

pub(super) fn trim_slash(url: &str) -> &str {
    url.trim_end_matches('/')
}

impl SettingsDialog {
    /// Probes each saved server, keeping its last result until the fresh one lands, and forgets
    /// servers that are gone.
    pub(in crate::settings::providers) fn probe_local(
        &mut self,
        ids: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let cache = &mut cx.default_global::<HealthCache>().0;
        cache.retain(|id, _| ids.contains(id));
        let health = &mut self.providers.local.health;
        health.retain(|id, _| ids.contains(id));
        for (id, last) in cache.iter() {
            health.entry(id.clone()).or_insert_with(|| last.clone());
        }
        for id in ids {
            self.probe_one(id, cx);
        }
    }

    pub(super) fn probe_one(&mut self, id: String, cx: &mut Context<Self>) {
        let key = id.clone();
        self.run(
            move || config::probe(&id),
            move |d, result, cx| {
                let health = result.unwrap_or_else(|e| ProviderHealth {
                    reachable: false,
                    latency_ms: None,
                    served_models: Vec::new(),
                    error: Some(e.to_string()),
                    images: HashMap::new(),
                });
                cx.default_global::<HealthCache>()
                    .0
                    .insert(key.clone(), health.clone());
                d.providers.local.health.insert(key, health);
            },
            cx,
        );
    }

    /// Looks for model servers answering on this computer's usual ports.
    pub(in crate::settings::providers) fn discover_servers(&mut self, cx: &mut Context<Self>) {
        self.run(
            config::discover,
            |d, found, _| d.providers.local.found = Some(found),
            cx,
        );
    }

    /// Escape closes the address being edited. False when none is.
    pub(in crate::settings) fn cancel_address(&mut self, cx: &mut Context<Self>) -> bool {
        if self.providers.local.address.is_none() {
            return false;
        }
        self.close_address();
        cx.notify();
        true
    }

    pub(super) fn close_address(&mut self) {
        self.providers.local.address = None;
        self.drop_fields(ADDRESS_FIELD);
    }

    /// Runs a repair of the open server, then reloads and probes it again.
    pub(super) fn repair(
        &mut self,
        id: String,
        task: impl FnOnce() -> Result<(), patchwork_server::Error> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let local = &mut self.providers.local;
        local.repairing = true;
        local.repair_error = None;
        self.run(
            task,
            move |d, done, cx| {
                d.providers.local.repairing = false;
                match done {
                    Ok(()) => {
                        d.close_address();
                        d.providers.local.health.remove(&id);
                        cx.default_global::<HealthCache>().0.remove(&id);
                        d.reload_providers(cx);
                    }
                    Err(e) => d.providers.local.repair_error = Some(e.to_string()),
                }
            },
            cx,
        );
        cx.notify();
    }
}
