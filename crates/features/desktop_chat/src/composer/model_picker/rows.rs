//! The models the menu offers: each agent's, current ones before older ones, and the starred
//! ones kept across restarts.

// Rust guideline compliant 2026-10-09

use gpui::prelude::*;
use patchwork_client::local::core::Core;
use patchwork_model::provider::{AgentKind, ModelOption};
use patchwork_views_logos::{AGENT_KINDS, agent_label, api_model_place, api_provider_name};

use crate::composer::Composer;

/// Where the starred models are kept, as `agent:model` keys.
const FAVORITES_KEY: &str = "patchwork-model-favorites";

/// A model in the menu: its agent, the option, and its `agent:model` key.
#[derive(Clone)]
pub struct ModelRow {
    pub kind: AgentKind,
    pub item: ModelOption,
    pub key: String,
}

#[must_use]
pub fn row_key(kind: AgentKind, item: &ModelOption) -> String {
    format!(
        "{}:{}",
        agent_label(kind).to_lowercase(),
        item.id.as_deref().unwrap_or("default")
    )
}

pub fn read_favorites(core: &Core) -> Vec<String> {
    core.state(FAVORITES_KEY)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn save_favorites(core: &Core, favorites: &[String]) {
    let saved = serde_json::to_string(favorites)
        .map_err(|e| e.to_string().into())
        .and_then(|json| core.set_state(FAVORITES_KEY, &json));
    if let Err(e) = saved {
        log::warn!(error:% = e; "could not save the favorite models: {{error}}");
    }
}

/// The provider an API model is served by, shown muted beside its name.
pub(super) fn provider_of(item: &ModelOption) -> Option<String> {
    let id = item.id.as_deref()?;
    Some(
        [
            Some(api_provider_name(id).unwrap_or_else(|| "API".to_string())),
            api_model_place(id),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · "),
    )
}

impl Composer {
    /// `kind`'s models, its CLI default first and its legacy models last.
    pub(super) fn rows_of(&self, kind: AgentKind) -> Vec<ModelRow> {
        let mut rows: Vec<ModelRow> = self
            .models_of(kind)
            .unwrap_or_default()
            .iter()
            .map(|item| ModelRow {
                kind,
                key: row_key(kind, item),
                item: item.clone(),
            })
            .collect();
        // A stable sort, so each agent's own order holds within current and legacy models.
        rows.sort_by_key(|row| row.item.is_legacy);
        rows
    }

    /// The starred models that `agents` still offer, in the agents' order.
    pub(super) fn starred(&self, agents: &[AgentKind]) -> Vec<ModelRow> {
        agents
            .iter()
            .flat_map(|&kind| self.rows_of(kind))
            .filter(|row| self.favorites.contains(&row.key))
            .collect()
    }

    /// Stars the model with `key`, or unstars it, and saves the list off the UI thread.
    pub(super) fn toggle_favorite(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(i) = self.favorites.iter().position(|f| f == key) {
            self.favorites.remove(i);
        } else {
            self.favorites.push(key.to_string());
        }
        let core = std::sync::Arc::clone(patchwork_client::AppState::get(cx).core());
        let favorites = self.favorites.clone();
        cx.background_spawn(async move { save_favorites(&core, &favorites) })
            .detach();
        cx.notify();
    }
}

/// The agents a model can come from, in the menus' order.
#[must_use]
pub fn ordered(agents: &[AgentKind]) -> Vec<AgentKind> {
    AGENT_KINDS
        .into_iter()
        .filter(|k| agents.contains(k))
        .collect()
}

/// The agents the menu lists: `agents`, or the chat's own agent when none is set up.
pub(in crate::composer) fn picker_agents(agents: &[AgentKind], own: AgentKind) -> Vec<AgentKind> {
    if agents.is_empty() {
        vec![own]
    } else {
        agents.to_vec()
    }
}
