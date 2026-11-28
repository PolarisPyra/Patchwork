//! The model and project chips beside the orchestrator's field, with the menus under them.

// Rust guideline compliant 2026-10-09

use patchwork_client::AppState;
use patchwork_views_logos::agent_logo;
use gpui::{AnyElement, Window, div, point, prelude::*, px};
use gpui::{Anchor, anchored, deferred};
use super::layout::{PILL_W, ink};
use super::requests::Orchestrator;
use crate::host::{Launch, LaunchMenu};
use std::rc::Rc;
use patchwork_views_chat::pill::pill_chip;
use patchwork_design_icons::{IconName, icon};

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
    pub(super) fn model_chip(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let launch = self.host.launch(cx);
        let name = launch.model_name.clone();
        let open = launch.open == Some(LaunchMenu::Model);
        let effort_open = launch.open == Some(LaunchMenu::Effort);
        let picker = self.host.launch_menu(LaunchMenu::Model, window, cx);
        let levels = self.host.launch_menu(LaunchMenu::Effort, window, cx);
        let host = Rc::clone(&self.host);
        let model_chip = launch_chip("orchestrator-model", format!("Choose model: {name}"), open)
            .child(agent_logo(launch.agent, launch.model.as_deref(), 14.))
            .child(div().max_w(px(220.)).truncate().child(name))
            .child(icon(IconName::CaretDown, 12.))
            .on_click(move |event, window, cx| {
                host.toggle_launch_menu(LaunchMenu::Model, event, window, cx);
            });
        let host = Rc::clone(&self.host);
        let effort_chip = launch.effort.clone().map(|effort| {
            let chip = launch_chip(
                "orchestrator-effort",
                format!("Effort: {effort}"),
                effort_open,
            )
            .child(icon(IconName::Lightbulb, 14.))
            .child(effort)
            .child(icon(IconName::CaretDown, 12.))
            .on_click(move |event, window, cx| {
                host.toggle_launch_menu(LaunchMenu::Effort, event, window, cx);
            });
            div()
                .relative()
                .flex()
                .child(chip)
                .children(under(levels, Anchor::TopLeft))
        });
        let project_chip = self.project_chip(&launch, window, cx);
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
            .child(project_chip);
        row.into_any_element()
    }

    /// The chip at the end of the row under the pill: the project a request that names none
    /// starts its chat in, with the menu that changes it.
    fn project_chip(
        &self,
        launch: &Launch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = &AppState::get(cx).settings;
        let project = &launch.project;
        let name = project
            .as_ref()
            .and_then(|id| settings.projects.iter().find(|p| &p.id == id))
            .map_or_else(|| "No project".to_string(), |p| p.name.clone());
        let glyph = if project.is_some() {
            IconName::FolderFill
        } else {
            IconName::ChatCircle
        };
        let open = launch.open == Some(LaunchMenu::Project);
        let menu = self.host.launch_menu(LaunchMenu::Project, window, cx);
        let host = Rc::clone(&self.host);
        let chip = launch_chip("orchestrator-project", format!("Project: {name}"), open)
            .child(icon(glyph, 14.))
            .child(div().max_w(px(180.)).truncate().child(name))
            .child(icon(IconName::CaretDown, 12.))
            .on_click(move |event, window, cx| {
                host.toggle_launch_menu(LaunchMenu::Project, event, window, cx);
            });
        let chip = div()
            .relative()
            .flex()
            .child(chip)
            .children(under(menu, Anchor::TopRight));
        chip.into_any_element()
    }
}
