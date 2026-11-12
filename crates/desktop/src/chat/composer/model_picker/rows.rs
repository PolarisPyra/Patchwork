//! The models the picker lists: starred ones kept across restarts, a search's matches, and the legacy ones.

// Rust guideline compliant 2026-10-09

use gpui::{Entity, FocusHandle, prelude::*};
use patchwork_server::Server;
use patchwork_server::provider::{AgentKind, ModelOption};
use crate::chat::composer::Composer;
use crate::ui::AGENT_KINDS;
use crate::ui::text_area::TextArea;
use crate::ui::{agent_label, api_model_place, api_provider_name};

/// Where the starred models are kept, as `agent:model` keys.
pub(super) const FAVORITES_KEY: &str = "patchwork-model-favorites";

/// The picker's height, and its panels' widths.
pub(super) const HEIGHT: f32 = 346.;
pub(super) const WIDTH: f32 = 360.;
pub(super) const LEGACY_WIDTH: f32 = 300.;

pub struct ModelPicker {
    pub search: Entity<TextArea>,
    /// The provider whose models show, or None for the favorites.
    pub provider: Option<AgentKind>,
    /// The provider whose Legacy models panel is open.
    pub legacy_open: Option<AgentKind>,
    pub favorites: Vec<String>,
    /// The dialog, which Tab and Shift+Tab go round inside rather than leave.
    pub dialog: FocusHandle,
}

/// A model in the list: its agent, the option, and its `agent:model` key.
#[derive(Clone)]
pub struct ModelRow {
    pub kind: AgentKind,
    pub item: ModelOption,
    pub key: String,
}

pub fn row_key(kind: AgentKind, item: &ModelOption) -> String {
    format!(
        "{}:{}",
        agent_label(kind).to_lowercase(),
        item.id.as_deref().unwrap_or("default")
    )
}

pub fn read_favorites(server: &Server) -> Vec<String> {
    server
        .store
        .state(FAVORITES_KEY)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

pub(super) fn save_favorites(server: &Server, favorites: &[String]) {
    let saved = serde_json::to_string(favorites)
        .map_err(|e| e.to_string().into())
        .and_then(|json| server.store.set_state(FAVORITES_KEY, &json));
    if let Err(e) = saved {
        log::warn!(error:% = e; "could not save the favorite models: {{error}}");
    }
}

/// The name a model's second line gives where it comes from.
pub(super) fn source_of(kind: AgentKind, item: &ModelOption) -> String {
    match (&item.id, kind) {
        (None, _) => item
            .default_model
            .clone()
            .unwrap_or_else(|| "CLI default".into()),
        (Some(id), AgentKind::Api) => [
            Some(api_provider_name(id).unwrap_or_else(|| "API".to_string())),
            api_model_place(id),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · "),
        _ => agent_label(kind).into(),
    }
}

impl Composer {
    /// The rows the list shows: a search's matches, the favorites, or one provider's current
    /// models; favorites first, then current models, then legacy ones.
    pub(in crate::chat::composer) fn model_rows(
        &self,
        agents: &[AgentKind],
        cx: &gpui::App,
    ) -> Vec<ModelRow> {
        let Some(picker) = &self.picker else {
            return Vec::new();
        };
        let query = picker.search.read(cx).content.trim().to_lowercase();
        let favorites = &picker.favorites;
        let legacy =
            |item: &ModelOption, key: &str| item.is_legacy && !favorites.iter().any(|f| f == key);
        let rank = |row: &ModelRow| {
            if favorites.contains(&row.key) {
                2
            } else {
                i32::from(!row.item.is_legacy)
            }
        };
        let mut rows: Vec<ModelRow> = agents
            .iter()
            .flat_map(|&kind| {
                self.models_of(kind)
                    .unwrap_or_default()
                    .iter()
                    .map(move |item| ModelRow {
                        kind,
                        key: row_key(kind, item),
                        item: item.clone(),
                    })
            })
            .filter(|row| {
                if query.is_empty() {
                    match picker.provider {
                        None => favorites.contains(&row.key),
                        Some(kind) => row.kind == kind && !legacy(&row.item, &row.key),
                    }
                } else {
                    let id = row.item.id.as_deref();
                    format!(
                        "{} {} {} {} {}",
                        row.item.label,
                        row.item.default_model.as_deref().unwrap_or(""),
                        agent_label(row.kind),
                        id.and_then(api_provider_name).unwrap_or_default(),
                        id.and_then(api_model_place).unwrap_or_default(),
                    )
                    .to_lowercase()
                    .contains(&query)
                }
            })
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(rank(row)));
        rows
    }

    /// The provider's legacy models, shown in their own panel.
    pub(super) fn legacy_rows(&self, kind: AgentKind) -> Vec<ModelRow> {
        let favorites = self
            .picker
            .as_ref()
            .map(|p| p.favorites.clone())
            .unwrap_or_default();
        self.models_of(kind)
            .unwrap_or_default()
            .iter()
            .map(|item| ModelRow {
                kind,
                key: row_key(kind, item),
                item: item.clone(),
            })
            .filter(|row| row.item.is_legacy && !favorites.contains(&row.key))
            .collect()
    }

    pub(super) fn toggle_favorite(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        if let Some(i) = picker.favorites.iter().position(|f| f == key) {
            picker.favorites.remove(i);
        } else {
            picker.favorites.push(key.to_string());
        }
        self.favorites = picker.favorites.clone();
        if let Some(workspace) = self.workspace.upgrade() {
            let server = std::sync::Arc::clone(&workspace.read(cx).server);
            let favorites = self.favorites.clone();
            cx.background_spawn(async move { save_favorites(&server, &favorites) })
                .detach();
        }
        cx.notify();
    }
}

/// The agents a model can come from, in the pickers' order.
pub fn ordered(agents: &[AgentKind]) -> Vec<AgentKind> {
    AGENT_KINDS
        .into_iter()
        .filter(|k| agents.contains(k))
        .collect()
}
