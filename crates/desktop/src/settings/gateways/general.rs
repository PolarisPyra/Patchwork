//! The rows above the chat apps: what chats started from a chat app may do, and the project
//! they open in.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, ClickEvent, prelude::*, px};

use crate::settings::SettingsDialog;
use crate::settings::controls::{ACCESS_MODES, choice, segmented, select_box, token_row};
use crate::ui::theme::theme;

/// What chats started from a chat app may do, and the project they open in.
pub(super) fn general(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> Vec<AnyElement> {
    let t = *theme(cx);
    let gateway = d.settings.gateway.clone();
    let access = token_row("Access", None, &t).child(
        segmented(
            "gw-access",
            "Access",
            gateway.access,
            &ACCESS_MODES,
            &t,
            |d, mode, cx| d.update(|s| s.gateway.access = mode, cx),
            cx,
        )
        // Each choice as wide as its name: the longest one would stretch the others.
        .flex(),
    );
    let projects = d.settings.projects.clone();
    let current = gateway
        .project_id
        .as_ref()
        .and_then(|id| projects.iter().find(|p| &p.id == id))
        .map_or_else(|| "No project".to_string(), |p| p.name.clone());
    let chosen = gateway.project_id.clone();
    let project = token_row("Project", None, &t).child(
        select_box(
            "gw-project",
            "Project",
            current,
            d.menu_open("gw-project"),
            &t,
        )
        .w(px(200.))
        .on_click(cx.listener(move |d, e: &ClickEvent, window, cx| {
            let weak = cx.entity().downgrade();
            let none = choice(weak.clone(), "No project", chosen.is_none(), |d, cx| {
                d.menu = None;
                d.update(|s| s.gateway.project_id = None, cx);
            });
            let items = std::iter::once(none)
                .chain(projects.iter().map(|p| {
                    let id = p.id.clone();
                    let checked = chosen.as_ref() == Some(&p.id);
                    choice(weak.clone(), p.name.clone(), checked, move |d, cx| {
                        let id = id.clone();
                        d.menu = None;
                        d.update(|s| s.gateway.project_id = Some(id), cx);
                    })
                }))
                .collect();
            d.toggle_menu("gw-project", "Project", items, e.position(), window, cx);
        })),
    );
    vec![access.into_any_element(), project.into_any_element()]
}
