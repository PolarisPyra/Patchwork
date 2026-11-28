//! The model menu's rows: the starred models, each agent's models, the effort, and starring the
//! chat's model.
//!
//! An agent's models sit in a submenu of their own when there are several agents. Older models,
//! Antigravity's orchestrator models and other vendors' models each sit in a further submenu.

// Rust guideline compliant 2026-10-09

use gpui::{App, Context, Window};
use patchwork_design_components::menu::MenuItem;
use patchwork_design_icons::IconName;
use patchwork_model::provider::AgentKind;
use patchwork_views_logos::agent_label;

use super::effort_name;
use super::rows::{ModelRow, picker_agents, provider_of, row_key};
use crate::composer::Composer;
use crate::composer::menus::Pick;

/// More rows than this at the top and typing narrows the menu.
const FILTER_FROM: usize = 12;

impl Composer {
    /// The model menu's rows for `agents`, with `current`'s agent, model and effort checked, and
    /// whether typing should narrow them.
    pub(in crate::composer) fn model_items(
        &self,
        agents: &[AgentKind],
        current: (AgentKind, Option<&str>, Option<&str>),
        cx: &Context<Self>,
    ) -> (Vec<MenuItem>, bool) {
        let (agent, model, effort) = current;
        let agents = picker_agents(agents, agent);
        let pick = |pick: Pick| {
            let composer = cx.entity().downgrade();
            move |_: &mut Window, cx: &mut App| {
                let _ = composer.update(cx, |c, cx| {
                    c.commit(pick.clone(), cx);
                    cx.notify();
                });
            }
        };
        let model_row = |row: &ModelRow, named: bool| {
            let mut item = MenuItem::new(
                row.item.label.clone(),
                pick(Pick::Model(row.kind, row.item.id.clone())),
            )
            .radio(row.kind == agent && row.item.id.as_deref() == model);
            let place = if named {
                Some(agent_label(row.kind).to_string())
            } else if row.kind == AgentKind::Api {
                provider_of(&row.item)
            } else {
                None
            };
            if let Some(place) = place {
                item = item.detail(place);
            }
            item
        };
        let models_of = |kind: AgentKind| {
            let rows = self.rows_of(kind);
            let (current, older): (Vec<_>, Vec<_>) =
                rows.iter().partition(|row| !row.item.is_legacy);
            let mut groups: [(&str, IconName, Vec<MenuItem>); 2] = [
                ("Orchestrator", IconName::Robot, Vec::new()),
                ("Other models", IconName::Brain, Vec::new()),
            ];
            let mut items = Vec::new();
            for row in &current {
                let item = model_row(row, false);
                match family(kind, &row.item.label) {
                    Family::Own => items.push(item),
                    Family::Orchestrator => groups[0].2.push(item),
                    Family::Other => groups[1].2.push(item),
                }
            }
            for (label, icon, rows) in groups {
                if !rows.is_empty() {
                    let group = MenuItem::submenu(label, rows).icon(icon);
                    items.push(if items.is_empty() {
                        group
                    } else {
                        group.separated()
                    });
                }
            }
            if !older.is_empty() {
                let older = older.iter().map(|row| model_row(row, false)).collect();
                let older = MenuItem::submenu("Older models", older).icon(IconName::Clock);
                items.push(if items.is_empty() {
                    older
                } else {
                    older.separated()
                });
            }
            items
        };

        let mut items: Vec<MenuItem> = self
            .starred(&agents)
            .iter()
            .map(|row| model_row(row, agents.len() > 1))
            .collect();
        let starred = !items.is_empty();
        let mut groups: Vec<MenuItem> = if agents.len() > 1 {
            agents
                .iter()
                .map(|&kind| {
                    MenuItem::submenu(agent_label(kind), models_of(kind)).radio(kind == agent)
                })
                .collect()
        } else {
            models_of(agents[0])
        };
        if starred && let Some(first) = groups.first_mut() {
            first.separated = true;
        }
        items.append(&mut groups);
        let filtered = items.len() > FILTER_FROM;

        let levels = agent.model_efforts(model, self.models_of(agent));
        let has_effort = !levels.is_empty();
        if has_effort {
            let efforts = std::iter::once(None)
                .chain(levels.into_iter().map(Some))
                .map(|level| {
                    let checked = level.as_deref() == effort;
                    MenuItem::new(effort_name(level.as_deref()), pick(Pick::Effort(level)))
                        .radio(checked)
                })
                .collect();
            items.push(
                MenuItem::submenu("Effort", efforts)
                    .icon(IconName::Gauge)
                    .detail(effort_name(effort))
                    .separated(),
            );
        }

        let chosen = self
            .models_of(agent)
            .and_then(|models| models.iter().find(|m| m.id.as_deref() == model))
            .map(|item| row_key(agent, item));
        if let Some(key) = chosen {
            let is_starred = self.favorites.contains(&key);
            let composer = cx.entity().downgrade();
            let star = MenuItem::new(
                if is_starred {
                    "Remove from favorites"
                } else {
                    "Add to favorites"
                },
                move |_, cx| {
                    let _ = composer.update(cx, |c, cx| c.toggle_favorite(&key, cx));
                },
            )
            .icon(if is_starred {
                IconName::StarFill
            } else {
                IconName::Star
            });
            let star = if has_effort { star } else { star.separated() };
            items.push(star);
        }
        (items, filtered)
    }
}

/// Which group of its agent's menu a model sits in.
enum Family {
    /// The agent's own models, listed at the top.
    Own,
    /// Antigravity's orchestrator models, in a submenu of their own.
    Orchestrator,
    /// Models from other vendors that the agent also serves, in a submenu of their own.
    Other,
}

/// The group the model labelled `label` sits in among `kind`'s models. Only Antigravity mixes
/// in orchestrator and other vendors' models; every other agent's models are its own.
fn family(kind: AgentKind, label: &str) -> Family {
    if kind != AgentKind::Antigravity {
        Family::Own
    } else if label.starts_with("Orchestrator") {
        Family::Orchestrator
    } else if label.starts_with("Gemini") || label == "Default" {
        Family::Own
    } else {
        Family::Other
    }
}
