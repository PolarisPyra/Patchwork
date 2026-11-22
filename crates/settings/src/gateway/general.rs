//! The rows above the chat apps: what chats started from a chat app may do, the project they
//! open in, and who they talk as.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, prelude::*, px};

use crate::SettingsDialog;
use crate::controls::{ACCESS_MODES, choices, segmented, select_box, token_row};
use patchwork_ui::theme::theme;

/// What chats started from a chat app may do, the project they open in, and who they talk as.
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
    let options = std::iter::once((None, "No project".into()))
        .chain(
            projects
                .iter()
                .map(|p| (Some(p.id.clone()), p.name.clone().into())),
        )
        .collect();
    let project = token_row("Project", None, &t).child(
        select_box(
            "gw-project",
            "Project",
            current,
            d.menu_open("gw-project"),
            &t,
        )
        .w(px(200.))
        .on_click(choices(
            "gw-project",
            "Project",
            options,
            gateway.project_id.clone(),
            |d, id, cx| d.update(|s| s.gateway.project_id = id, cx),
            cx,
        )),
    );
    let chosen = gateway.personality;
    let current = chosen.clone().unwrap_or_else(|| "Patchwork".to_string());
    let options = std::iter::once((None, "Patchwork".into()))
        .chain(
            d.gateway
                .personalities
                .iter()
                .map(|p| (Some(p.name.clone()), p.name.clone().into())),
        )
        .collect();
    let personality = token_row("Personality", None, &t).child(
        select_box(
            "gw-personality",
            "Personality",
            current,
            d.menu_open("gw-personality"),
            &t,
        )
        .w(px(200.))
        .on_click(choices(
            "gw-personality",
            "Personality",
            options,
            chosen,
            |d, name, cx| d.update(|s| s.gateway.personality = name, cx),
            cx,
        )),
    );
    vec![
        access.into_any_element(),
        project.into_any_element(),
        personality.into_any_element(),
    ]
}
