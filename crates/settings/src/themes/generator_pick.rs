//! The agent, model and effort of the chats that write themes and backgrounds, picked on one row
//! of each pane that generates them.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, ClickEvent, Role, SharedString, div, prelude::*, px};
use patchwork_core::threads::AgentPick;

use crate::SettingsDialog;
use crate::controls::{choice, token_row};
use patchwork_ui::menu::MenuItem;
use patchwork_ui::theme::Theme;
use patchwork_ui::{AGENT_KINDS, effort_label, agent_label, dropdown_chip, spinner};

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

    /// What a chat on `agent` runs when no model is picked: the agent's model for new chats, else
    /// its CLI's own, as "Default (name)".
    fn default_model_label(&self, agent: patchwork_core::provider::AgentKind) -> String {
        let models = self.tokens.models.get(&agent);
        let name = match self.settings.tokens.default_models.get(&agent) {
            Some(id) => Some(
                models
                    .and_then(|models| models.iter().find(|m| m.id.as_ref() == Some(id)))
                    .map_or_else(|| id.clone(), |m| m.label.clone()),
            ),
            None => models
                .and_then(|models| models.iter().find(|m| m.id.is_none()))
                .and_then(|m| m.default_model.clone()),
        };
        name.map_or_else(|| "Default".to_string(), |name| format!("Default ({name})"))
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
                    let takes = agent.model_efforts(id.as_deref(), models);
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
                    self.default_model_label(agent),
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
                let levels = agent
                    .model_efforts(pick.model.as_deref(), models)
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

/// The row that picks what the chats that write themes and backgrounds run on: the agent when
/// more than one is connected, its model, and the effort when the model takes one.
pub(super) fn generator_row(
    d: &mut SettingsDialog,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let pick = d.generator_pick();
    d.load_models(pick.agent, cx);
    let row = token_row("Generate with", None, t);
    // Until the models are listed, a spinner holds the model's place.
    let Some(models) = d.tokens.models.get(&pick.agent).map(Vec::as_slice) else {
        let agent = d.settings.agents.len() > 1;
        let loading = div()
            .id("generator-models-loading")
            .role(Role::Status)
            .aria_label("Loading models")
            .flex()
            .items_center()
            .h(px(28.))
            .px(px(6.))
            .child(spinner(14., t.text_muted));
        let chips = agent.then(|| chip(d, Part::Agent, agent_label(pick.agent).to_string(), t, cx));
        return row.children(chips).child(loading).into_any_element();
    };
    let model = pick.model.as_deref().map_or_else(
        || d.default_model_label(pick.agent),
        |id| {
            models
                .iter()
                .find(|m| m.id.as_deref() == Some(id))
                .map_or_else(|| id.to_string(), |m| m.label.clone())
        },
    );
    let effort = pick
        .effort
        .as_deref()
        .map_or_else(|| "Default".to_string(), effort_label);
    let takes_effort = !pick
        .agent
        .model_efforts(pick.model.as_deref(), Some(models))
        .is_empty();
    let chips = [
        (
            Part::Agent,
            agent_label(pick.agent).to_string(),
            d.settings.agents.len() > 1,
        ),
        (Part::Model, model, true),
        (Part::Effort, effort, takes_effort),
    ];
    let chips: Vec<_> = chips
        .into_iter()
        .filter(|(.., shown)| *shown)
        .map(|(part, current, _)| chip(d, part, current, t, cx))
        .collect();
    row.children(chips).into_any_element()
}

/// One chip of the row, showing `current` and opening the dropdown of `part`.
fn chip(
    d: &SettingsDialog,
    part: Part,
    current: String,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let name = match part {
        Part::Agent => "Agent",
        Part::Model => "Model",
        Part::Effort => "Effort",
    };
    let key = SharedString::from(format!("generator-{}", name.to_lowercase()));
    let title = format!("{name} that generates themes and backgrounds");
    let open = d.menu_open(&key);
    dropdown_chip(key.clone(), format!("{title}: {current}"), current, open, t)
        .on_click(cx.listener(move |d, e: &ClickEvent, window, cx| {
            let items = d.generator_items(part, cx);
            d.toggle_menu(key.clone(), title.clone(), items, e.position(), window, cx);
        }))
        .into_any_element()
}
