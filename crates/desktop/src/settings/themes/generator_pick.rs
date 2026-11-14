//! The agent, model and effort of the chats that write themes and backgrounds, picked beside Generate.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, ClickEvent, SharedString, prelude::*};
use patchwork_server::threads::AgentPick;

use crate::chat::composer::{effort_label, efforts};
use crate::settings::SettingsDialog;
use crate::settings::controls::choice;
use crate::ui::menu::MenuItem;
use crate::ui::theme::Theme;
use crate::ui::{AGENT_KINDS, agent_label, dropdown_chip};

/// What a chip picks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Agent,
    Model,
    Effort,
}

impl SettingsDialog {
    /// What the chats that write themes and backgrounds run on: the pick, else the agent of new chats.
    fn generator_pick(&self) -> AgentPick {
        self.settings
            .generator_pick()
            .cloned()
            .unwrap_or_else(|| AgentPick {
                agent: self.settings.default_agent(),
                model: None,
                effort: None,
            })
    }

    /// Saves the pick. One that names only the agent of new chats is not kept, so it follows
    /// that agent.
    fn set_generator(&mut self, pick: AgentPick, cx: &mut Context<Self>) {
        self.update(
            |s| {
                let own = pick.agent == s.default_agent()
                    && pick.model.is_none()
                    && pick.effort.is_none();
                s.generator = (!own).then_some(pick);
            },
            cx,
        );
    }

    /// The rows of a chip's dropdown.
    fn generator_items(&self, part: Part, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let this = cx.entity().downgrade();
        let pick = self.generator_pick();
        let agent = pick.agent;
        let models = self.tokens.models.get(&agent).map(Vec::as_slice);
        match part {
            Part::Agent => AGENT_KINDS
                .into_iter()
                .filter(|kind| self.settings.agents.contains(kind))
                .map(|kind| {
                    choice(
                        this.clone(),
                        agent_label(kind),
                        kind == agent,
                        move |d, cx| {
                            let pick = AgentPick {
                                agent: kind,
                                model: None,
                                effort: None,
                            };
                            d.set_generator(pick, cx);
                        },
                    )
                })
                .collect(),
            Part::Model => {
                // A model keeps the effort only when it takes that level.
                let to_model = |id: Option<String>| {
                    let takes = efforts(agent, id.as_deref(), models);
                    let effort = pick.effort.clone().filter(|e| takes.contains(e));
                    move |d: &mut Self, cx: &mut Context<Self>| {
                        let pick = AgentPick {
                            agent,
                            model: id.clone(),
                            effort: effort.clone(),
                        };
                        d.set_generator(pick, cx);
                    }
                };
                let default = choice(
                    this.clone(),
                    "Default",
                    pick.model.is_none(),
                    to_model(None),
                );
                let listed = models
                    .unwrap_or_default()
                    .iter()
                    .filter(|m| !m.is_legacy || m.id == pick.model)
                    .filter_map(|m| {
                        let id = m.id.clone()?;
                        let checked = pick.model.as_deref() == Some(id.as_str());
                        Some(choice(
                            this.clone(),
                            m.label.clone(),
                            checked,
                            to_model(Some(id)),
                        ))
                    });
                std::iter::once(default).chain(listed).collect()
            }
            Part::Effort => {
                let to_effort = |effort: Option<String>| {
                    let model = pick.model.clone();
                    move |d: &mut Self, cx: &mut Context<Self>| {
                        let pick = AgentPick {
                            agent,
                            model: model.clone(),
                            effort: effort.clone(),
                        };
                        d.set_generator(pick, cx);
                    }
                };
                let default = choice(
                    this.clone(),
                    "Default",
                    pick.effort.is_none(),
                    to_effort(None),
                );
                let levels = efforts(agent, pick.model.as_deref(), models)
                    .into_iter()
                    .map(|level| {
                        let checked = pick.effort.as_deref() == Some(level.as_str());
                        choice(
                            this.clone(),
                            effort_label(&level),
                            checked,
                            to_effort(Some(level)),
                        )
                    });
                std::iter::once(default).chain(levels).collect()
            }
        }
    }
}

/// The chips beside Generate for the `noun` described in the field `field`: the agent when more
/// than one is connected, its model, and the effort when the model takes one.
pub(super) fn generator_chips(
    d: &mut SettingsDialog,
    field: &'static str,
    noun: &'static str,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let pick = d.generator_pick();
    d.load_models(pick.agent, cx);
    let models = d.tokens.models.get(&pick.agent).map(Vec::as_slice);
    let model = pick.model.as_deref().map_or_else(
        || "Default model".to_string(),
        |id| {
            models
                .and_then(|models| models.iter().find(|m| m.id.as_deref() == Some(id)))
                .map_or_else(|| id.to_string(), |m| m.label.clone())
        },
    );
    let effort = pick
        .effort
        .as_deref()
        .map_or_else(|| "Default effort".to_string(), effort_label);
    let takes_effort = !efforts(pick.agent, pick.model.as_deref(), models).is_empty();
    let chips = [
        (
            Part::Agent,
            "Agent",
            agent_label(pick.agent).to_string(),
            d.settings.agents.len() > 1,
        ),
        (Part::Model, "Model", model, true),
        (Part::Effort, "Effort", effort, takes_effort),
    ];
    chips
        .into_iter()
        .filter(|(.., shown)| *shown)
        .map(|(part, name, current, _)| {
            let key = SharedString::from(format!("{field}-{name}"));
            let title = format!("{name} for the {noun}");
            let open = d.menu_open(&key);
            dropdown_chip(key.clone(), format!("{title}: {current}"), current, open, t)
                .on_click(cx.listener(move |d, e: &ClickEvent, window, cx| {
                    let items = d.generator_items(part, cx);
                    d.toggle_menu(key.clone(), title.clone(), items, e.position(), window, cx);
                }))
                .into_any_element()
        })
        .collect()
}
