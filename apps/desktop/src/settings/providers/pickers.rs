//! The vision and default model pickers, and the preset grid.

use gpui::{AnyElement, Context, Div, Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_server::provider::AgentKind;
use patchwork_server::settings::VisionModel;
use crate::settings::SettingsDialog;
use crate::settings::controls::{choice, select_box};
use crate::ui::theme::{FS_SM, RADIUS, Theme, theme};
use crate::ui::{AGENT_KINDS, agent_label, icon, provider_logo};
use super::state::VISION_AGENTS;
use super::{form};

/// A grid of provider tiles: the presets to add, and the servers on this computer.
pub(super) fn tile_grid(label: &'static str) -> Stateful<Div> {
    div()
        .id(label)
        .role(Role::Group)
        .aria_label(label)
        .grid()
        .grid_cols(6)
        .gap(px(8.))
        .w_full()
        .py(px(10.))
}

/// One tile of a grid: a mark over a name.
pub(super) fn tile(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, strong, fg) = (t.bg_hover, t.border_strong, t.text);
    crate::ui::button(id.into(), label)
        .relative()
        .h(px(72.))
        .min_w_0()
        .flex_col()
        .justify_center()
        .gap(px(8.))
        .rounded(px(RADIUS))
        .border_1()
        .px(px(8.))
        .text_size(px(FS_SM))
        .hover(move |s| s.border_color(strong).bg(hover).text_color(fg))
}

impl SettingsDialog {
    /// The model that reads a message's images for an API chat, from every connected agent that
    /// can read them; the first choice sends the images to the chat's own model.
    pub(super) fn vision_picker(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *theme(cx);
        let vision = self.settings.vision.clone();
        let kinds: Vec<AgentKind> = AGENT_KINDS
            .into_iter()
            .filter(|k| VISION_AGENTS.contains(k) && self.settings.agents.contains(k))
            .collect();
        let mut options: Vec<(Option<VisionModel>, String)> =
            vec![(None, "The chat's own model".into())];
        for kind in kinds {
            let mut listed: Vec<(String, String)> = self
                .providers
                .models
                .get(&kind)
                .into_iter()
                .flatten()
                .filter_map(|m| m.id.clone().map(|id| (id, m.label.clone())))
                .collect();
            // A saved model that is no longer listed still shows as picked.
            if let Some(v) = vision.as_ref().filter(|v| v.agent == kind)
                && !listed.iter().any(|(id, _)| *id == v.model)
            {
                listed.push((v.model.clone(), v.model.clone()));
            }
            for (model, name) in listed {
                options.push((
                    Some(VisionModel { agent: kind, model }),
                    format!("{} · {name}", agent_label(kind)),
                ));
            }
        }
        let current = options
            .iter()
            .find(|(v, _)| {
                v.as_ref().map(|v| (v.agent, &v.model))
                    == vision.as_ref().map(|v| (v.agent, &v.model))
            })
            .map_or_else(
                || "The chat's own model".to_string(),
                |(_, name)| name.clone(),
            );
        let open = self.menu_open("vision");
        select_box("vision-model", "Vision model", current, open, &t)
            .w(px(200.))
            .on_click(cx.listener(move |d, e: &gpui::ClickEvent, window, cx| {
                let weak = cx.entity().downgrade();
                let items = options
                    .iter()
                    .map(|(v, name)| {
                        let pick = v.clone();
                        let checked = v.as_ref().map(|v| (v.agent, &v.model))
                            == d.settings.vision.as_ref().map(|v| (v.agent, &v.model));
                        choice(weak.clone(), name.clone(), checked, move |d, cx| {
                            let pick = pick.clone();
                            d.menu = None;
                            d.update(|s| s.vision = pick, cx)
                        })
                    })
                    .collect();
                d.toggle_menu("vision", "Vision model", items, e.position(), window, cx)
            }))
            .into_any_element()
    }

    /// Which connected agent a new task starts with: the first of `agents`.
    pub(super) fn default_picker(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *theme(cx);
        let connected = self.settings.agents.clone();
        let current = connected.first().map_or("", |k| agent_label(*k));
        let open = self.menu_open("default-agent");
        select_box("default-agent", "Default for new tasks", current, open, &t)
            .w(px(200.))
            .on_click(cx.listener(move |d, e: &gpui::ClickEvent, window, cx| {
                let weak = cx.entity().downgrade();
                let first = d.settings.agents.first().copied();
                let items = AGENT_KINDS
                    .into_iter()
                    .filter(|k| connected.contains(k))
                    .map(|kind| {
                        choice(
                            weak.clone(),
                            agent_label(kind),
                            first == Some(kind),
                            move |d, cx| {
                                d.menu = None;
                                d.update(
                                    |s| {
                                        s.agents.retain(|k| *k != kind);
                                        s.agents.insert(0, kind);
                                    },
                                    cx,
                                )
                            },
                        )
                    })
                    .collect();
                d.toggle_menu(
                    "default-agent",
                    "Default for new tasks",
                    items,
                    e.position(),
                    window,
                    cx,
                )
            }))
            .into_any_element()
    }

    /// The presets to start a provider from, then Add a server for any other address.
    pub(super) fn preset_grid(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = *theme(cx);
        let tiles: Vec<_> = form::PRESETS
            .iter()
            .map(|&(id, name, base_url, models)| {
                tile(format!("preset-{id}"), format!("Add {name}"), &t)
                    .border_color(t.border)
                    .bg(t.bg_panel)
                    .text_color(t.text)
                    .child(provider_logo(id, None, 22.))
                    .child(div().max_w_full().truncate().child(name))
                    .on_click(cx.listener(move |d, _, window, cx| {
                        d.open_form(form::blank(id, name, base_url, models), false, window, cx)
                    }))
            })
            .collect();
        tile_grid("Add a provider").children(tiles).child(
            tile("preset-custom", "Add a server", &t)
                .border_dashed()
                .border_color(t.border_strong)
                .text_color(t.text_muted)
                .child(icon("plus", 22.))
                .child(div().max_w_full().truncate().child("Add a server"))
                .on_click(cx.listener(|d, _, window, cx| {
                    d.open_form(form::blank("custom", "", "", &[]), false, window, cx)
                })),
        )
    }
}
