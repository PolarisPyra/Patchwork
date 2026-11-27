//! The model and project chips beside the orchestrator's field, with the menus under them.

// Rust guideline compliant 2026-10-09

use patchwork_desktop_shared::agent_logo;
use gpui::{AnyElement, Window, div, point, prelude::*, px};
use gpui::{Anchor, anchored, deferred};
use super::layout::{PILL_W, ink};
use super::requests::Orchestrator;
use crate::chat::composer::Composer;
use patchwork_ui_components::pill::pill_chip;
use patchwork_ui_theme::icon;

/// A chip under the pill that opens a menu.
fn launch_chip(id: &'static str, label: String, open: bool) -> gpui::Stateful<gpui::Div> {
    pill_chip(id, label, ink(1.))
        .aria_expanded(open)
        .when(open, |chip| chip.bg(ink(0.08)))
}

/// A chip's open menu, placed under the chip: lined up with its left edge, or with its right
/// edge for a chip at the row's end.
fn under(menu: Option<AnyElement>, anchor: Anchor) -> Option<gpui::Div> {
    let menu = menu?;
    let corner = div().absolute().bottom_0();
    let corner = if anchor == Anchor::TopRight {
        corner.right_0()
    } else {
        corner.left_0()
    };
    Some(
        corner.child(
            deferred(
                anchored()
                    .anchor(anchor)
                    .offset(point(px(0.), px(4.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(menu),
            )
            .with_priority(1),
        ),
    )
}

impl Orchestrator {
    /// The chips under the pill: the agent and model a request's new chat starts with, its
    /// effort, and at the end its project, each with the menu that changes it open under it.
    pub(super) fn model_chip(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let settings = &self.workspace.upgrade()?.read(cx).settings;
        let composer = self.composer.read(cx);
        let (agent, model) = composer.launch_pick(settings);
        let name = composer.model_name(agent, model.as_deref());
        let effort = composer.launch_effort_name(settings);
        let (open, effort_open) = (composer.launch_picker_open(), composer.launch_effort_open());
        let (picker, levels) = self.composer.update(cx, |composer, cx| {
            let picker = composer.launch_picker(window, cx);
            (picker, composer.launch_effort_menu(cx))
        });
        let composer = self.composer.clone();
        let model_chip = launch_chip("orchestrator-model", format!("Choose model: {name}"), open)
            .child(agent_logo(agent, model.as_deref(), 14.))
            .child(div().max_w(px(220.)).truncate().child(name))
            .child(icon("caret-down", 12.))
            .on_click(move |event, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.toggle_launch_picker(event, window, cx);
                });
            });
        let composer = self.composer.clone();
        let effort_chip = effort.map(|effort| {
            let chip = launch_chip(
                "orchestrator-effort",
                format!("Effort: {effort}"),
                effort_open,
            )
            .child(icon("lightbulb", 14.))
            .child(effort)
            .child(icon("caret-down", 12.))
            .on_click(move |event, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.toggle_launch_effort(event, window, cx);
                });
            });
            div()
                .relative()
                .flex()
                .child(chip)
                .children(under(levels, Anchor::TopLeft))
        });
        let project_chip = self.project_chip(cx);
        let row = div()
            .w_full()
            .max_w(px(PILL_W))
            .mt(px(6.))
            .px(px(12.))
            .flex()
            .gap(px(6.))
            .child(
                div()
                    .relative()
                    .flex()
                    .child(model_chip)
                    .children(under(picker, Anchor::TopLeft)),
            )
            .children(effort_chip)
            .child(div().flex_1())
            .children(project_chip);
        Some(row.into_any_element())
    }

    /// The chip at the end of the row under the pill: the project a request that names none
    /// starts its chat in, with the menu that changes it.
    pub(super) fn project_chip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let workspace = self.workspace.upgrade()?;
        let settings = &workspace.read(cx).settings;
        let composer = self.composer.read(cx);
        let project = composer.launch_project(settings);
        let name = project
            .as_ref()
            .and_then(|id| settings.projects.iter().find(|p| &p.id == id))
            .map_or_else(|| "No project".to_string(), |p| p.name.clone());
        let glyph = if project.is_some() {
            "folder-fill"
        } else {
            "chat-circle"
        };
        let open = composer.launch_project_open();
        let menu = self.composer.update(cx, Composer::launch_project_menu);
        let composer = self.composer.clone();
        let chip = launch_chip("orchestrator-project", format!("Project: {name}"), open)
            .child(icon(glyph, 14.))
            .child(div().max_w(px(180.)).truncate().child(name))
            .child(icon("caret-down", 12.))
            .on_click(move |event, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.toggle_launch_project(event, window, cx);
                });
            });
        let chip = div()
            .relative()
            .flex()
            .child(chip)
            .children(under(menu, Anchor::TopRight));
        Some(chip.into_any_element())
    }
}
