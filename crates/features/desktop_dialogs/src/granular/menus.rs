//! The card's dropdowns: the rows each offers, and opening and closing them.

// Rust guideline compliant 2026-11-23

use patchwork_views_logos::{AGENT_KINDS, agent_label};
use gpui::{Entity, SharedString, WeakEntity, Window, anchored, deferred, div, point, prelude::*, px};
use patchwork_client::local::core::chats::Granular;
use patchwork_model::provider::ModelOption;

use super::card::{GranularDialog, Part, Slot, agent_of, edit, pick_of, set_agent};
use patchwork_design_text::effort_label;
use patchwork_design_components::{menu::Menu, menu::MenuItem};

/// A menu row that makes `change` to the card's settings.
fn choice(
    dialog: &WeakEntity<GranularDialog>,
    label: impl Into<SharedString>,
    checked: bool,
    change: impl Fn(&mut Granular) + 'static,
) -> MenuItem {
    let dialog = dialog.clone();
    MenuItem::new(label, move |_, cx| {
        let _ = dialog.update(cx, |d, cx| {
            change(&mut d.value);
            d.error = None;
            cx.notify();
        });
    })
    .radio(checked)
}

/// Lays `menu` over the window with its top-left corner 4px under the bottom-left of the element
/// this is a child of, which must be `relative`: a menu that drops from its select.
pub(super) fn below(menu: Entity<Menu>) -> gpui::Div {
    div().absolute().bottom_0().left_0().child(
        deferred(
            anchored()
                .offset(point(px(0.), px(4.)))
                .snap_to_window_with_margin(px(8.))
                .child(menu),
        )
        .with_priority(3),
    )
}

impl GranularDialog {
    /// The rows of the dropdown for one part of a slot.
    fn items(&self, slot: Slot, part: Part, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let value = &self.value;
        let this = cx.entity().downgrade();
        let pick = pick_of(value, slot);
        if part == Part::Agent {
            let current = pick.map(|p| p.agent);
            let none = (slot == Slot::Executor).then(|| {
                choice(&this, "None", current.is_none(), move |v| {
                    set_agent(v, slot, None);
                })
            });
            let agents = AGENT_KINDS
                .into_iter()
                .filter(|kind| self.settings.agents.contains(kind))
                .map(|kind| {
                    let to = self.default_pick(Some(kind));
                    choice(&this, agent_label(kind), current == Some(kind), move |v| {
                        set_agent(v, slot, Some(to.clone()));
                    })
                });
            return none.into_iter().chain(agents).collect();
        }
        let Some(agent) = agent_of(value, slot) else {
            return Vec::new();
        };
        let models = self.models_of(agent);
        let model = pick.and_then(|p| p.model.clone());
        if part == Part::Effort {
            let current = pick.and_then(|p| p.effort.clone());
            let default = choice(&this, "Default", current.is_none(), move |v| {
                edit(v, slot, |p| p.effort = None);
            });
            let levels = agent
                .model_efforts(model.as_deref(), models)
                .into_iter()
                .map(|level| {
                    let checked = current.as_deref() == Some(level.as_str());
                    choice(&this, effort_label(&level), checked, move |v| {
                        edit(v, slot, |p| p.effort = Some(level.clone()));
                    })
                });
            return std::iter::once(default).chain(levels).collect();
        }
        // A model keeps the effort only when it takes that level.
        let to_model = |id: Option<String>| {
            let takes = agent.model_efforts(id.as_deref(), models);
            move |v: &mut Granular| {
                edit(v, slot, |p| {
                    p.model.clone_from(&id);
                    p.effort = p.effort.take().filter(|e| takes.contains(e));
                });
            }
        };
        let default = choice(&this, "Default", model.is_none(), to_model(None));
        let listed = models
            .unwrap_or_default()
            .iter()
            .filter(|m| !m.is_legacy || m.id == model)
            .filter_map(|m: &ModelOption| {
                let id = m.id.clone()?;
                let checked = model.as_deref() == Some(id.as_str());
                Some(choice(&this, m.label.clone(), checked, to_model(Some(id))))
            });
        std::iter::once(default).chain(listed).collect()
    }

    /// Opens the dropdown of the chip `key`, or closes it when it is the one open.
    pub(super) fn toggle_menu(
        &mut self,
        key: SharedString,
        (slot, part): (Slot, Part),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu.as_ref().is_some_and(|(k, _)| *k == key) {
            self.menu = None;
            return cx.notify();
        }
        let items = self.items(slot, part, cx);
        let label = match part {
            Part::Agent => slot.name().to_string(),
            Part::Model => format!("{} model", slot.name()),
            Part::Effort => format!("{} effort", slot.name()),
        };
        let menu = Menu::open_for(label, items, window, cx, |this: &mut Self, window, cx| {
            this.menu = None;
            window.focus(&this.focus, cx);
            cx.notify();
        });
        self.menu = Some((key, menu));
        cx.notify();
    }
}
