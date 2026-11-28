//! The models the picker offers: one list per rail tab, an agent's or the starred ones kept across
//! restarts, or every agent's models that match what is typed.

// Rust guideline compliant 2026-10-09

use patchwork_views_logos::{AGENT_KINDS, agent_label, api_model_place, api_provider_name};
use gpui::{Entity, FocusHandle, ScrollHandle, Subscription, prelude::*};
use patchwork_client::local::core::Core;
use patchwork_model::provider::{AgentKind, ModelOption};
use crate::composer::Composer;
use patchwork_design_editor::TextArea;

use super::nav::Tab;

/// Where the starred models are kept, as `agent:model` keys.
pub(super) const FAVORITES_KEY: &str = "patchwork-model-favorites";

pub struct ModelPicker {
    /// The keys' home: typing here searches every agent's models, and the arrows move the keys.
    pub search: Entity<TextArea>,
    pub favorites: Vec<String>,
    /// The dialog, which Tab and Shift+Tab go round inside rather than leave.
    pub dialog: FocusHandle,
    /// The effort slider, which takes the keys after the search field.
    pub effort: FocusHandle,
    /// The rail tab whose models are listed while the search is empty.
    pub tab: Tab,
    /// The row under the arrows or the pointer.
    pub index: usize,
    /// The agents whose models are offered, for the keys to see the list as it is drawn.
    pub agents: Vec<AgentKind>,
    /// The list's scroll, which follows the row under the keys.
    pub scroll: ScrollHandle,
    /// Takes the arrows from the search field before it moves its caret; dropped with the picker.
    pub(super) _arrows: Subscription,
}

/// A model in the list: its agent, the option, and its `agent:model` key.
#[derive(Clone)]
pub struct ModelRow {
    pub kind: AgentKind,
    pub item: ModelOption,
    pub key: String,
}

/// A small heading drawn above a row of the list.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Heading {
    /// The agent whose matches follow, while searching.
    Agent(AgentKind),
    /// The agent's older models, which follow its current ones.
    Older,
}

/// A row of the list, and the heading drawn above it, if any.
pub struct ListRow {
    pub row: ModelRow,
    pub heading: Option<Heading>,
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

pub(super) fn save_favorites(core: &Core, favorites: &[String]) {
    let saved = serde_json::to_string(favorites)
        .map_err(|e| e.to_string().into())
        .and_then(|json| core.set_state(FAVORITES_KEY, &json));
    if let Err(e) = saved {
        log::warn!(error:% = e; "could not save the favorite models: {{error}}");
    }
}

/// Where a model comes from, shown muted beside its name.
pub(super) fn source_of(kind: AgentKind, item: &ModelOption) -> String {
    match (&item.id, kind) {
        (None, _) => item
            .default_model
            .clone()
            .unwrap_or_else(|| "The CLI's own default".into()),
        (Some(id), AgentKind::Api) => [
            Some(api_provider_name(id).unwrap_or_else(|| "API".to_string())),
            api_model_place(id),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · "),
        (Some(id), _) => id.clone(),
    }
}

/// The words a search is matched against: the model's name and id, its agent, and for an API
/// model its provider.
fn haystack(row: &ModelRow) -> String {
    let id = row.item.id.as_deref();
    format!(
        "{} {} {} {} {} {}",
        row.item.label,
        id.unwrap_or(""),
        row.item.default_model.as_deref().unwrap_or(""),
        agent_label(row.kind),
        id.and_then(api_provider_name).unwrap_or_default(),
        id.and_then(api_model_place).unwrap_or_default(),
    )
    .to_lowercase()
}

/// Keeps the rows that match `query`, or all of them when the agent's name does.
fn narrowed(rows: Vec<ModelRow>, heading: &str, query: &str) -> Vec<ModelRow> {
    if heading.to_lowercase().contains(query) {
        return rows;
    }
    rows.into_iter()
        .filter(|row| haystack(row).contains(query))
        .collect()
}

/// `rows` with `heading` above the first, and "Older models" above the first legacy one when
/// `older` is set.
fn headed(rows: Vec<ModelRow>, heading: Option<Heading>, older: bool) -> Vec<ListRow> {
    let first_legacy = rows
        .iter()
        .position(|r| r.item.is_legacy)
        .filter(|&i| older && i > 0);
    rows.into_iter()
        .enumerate()
        .map(|(i, row)| ListRow {
            row,
            heading: if i == 0 {
                heading
            } else {
                (Some(i) == first_legacy).then_some(Heading::Older)
            },
        })
        .collect()
}

impl Composer {
    /// `kind`'s models, its CLI default first and its legacy models last.
    fn rows_of(&self, kind: AgentKind) -> Vec<ModelRow> {
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
    fn starred(&self, agents: &[AgentKind], favorites: &[String]) -> Vec<ModelRow> {
        agents
            .iter()
            .flat_map(|&kind| self.rows_of(kind))
            .filter(|row| favorites.contains(&row.key))
            .collect()
    }

    /// The rail's tabs: Favorites when a model is starred, then each agent.
    pub(in crate::composer) fn picker_tabs(&self, agents: &[AgentKind]) -> Vec<Tab> {
        let favorites = self
            .picker
            .as_ref()
            .map_or(&self.favorites, |p| &p.favorites);
        let starred = !self.starred(agents, favorites).is_empty();
        starred
            .then_some(Tab::Favorites)
            .into_iter()
            .chain(agents.iter().map(|&kind| Tab::Agent(kind)))
            .collect()
    }

    /// The list as it is drawn: the tab's models, or while searching every agent's matches
    /// under its name.
    pub(in crate::composer) fn picker_rows(
        &self,
        agents: &[AgentKind],
        cx: &gpui::App,
    ) -> Vec<ListRow> {
        let Some(picker) = &self.picker else {
            return Vec::new();
        };
        let query = picker.search.read(cx).content.trim().to_lowercase();
        if !query.is_empty() {
            return agents
                .iter()
                .flat_map(|&kind| {
                    let rows = narrowed(self.rows_of(kind), agent_label(kind), &query);
                    headed(rows, Some(Heading::Agent(kind)), false)
                })
                .collect();
        }
        match picker.tab {
            Tab::Favorites => headed(self.starred(agents, &picker.favorites), None, false),
            Tab::Agent(kind) => headed(self.rows_of(kind), None, true),
        }
    }

    /// Stars the model with `key`, or unstars it, and saves the list off the UI thread.
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
        let core = std::sync::Arc::clone(patchwork_client::AppState::get(cx).core());
        let favorites = self.favorites.clone();
        cx.background_spawn(async move { save_favorites(&core, &favorites) })
            .detach();
        cx.notify();
    }
}

/// The agents a model can come from, in the pickers' order.
#[must_use]
pub fn ordered(agents: &[AgentKind]) -> Vec<AgentKind> {
    AGENT_KINDS
        .into_iter()
        .filter(|k| agents.contains(k))
        .collect()
}
