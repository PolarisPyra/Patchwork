//! What floats over the top right of a chat's transcript: search, Android devices, "Continue in
//! terminal", "Open in editor" and an agent's details, with the subagents card under them.

// Rust guideline compliant 2026-11-27

use crate::prelude::*;
use gpui::{Div, Window, div, prelude::*, px};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, fs_sm, radius};
use patchwork_design_components::button;
use patchwork_design_icons::icon;
use super::find_bar;
use super::subagent_list;

/// The soft shadow under the header's buttons.
fn shadow() -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: gpui::hsla(0., 0., 0., 0.12),
        offset: gpui::point(px(0.), px(1.)),
        blur_radius: px(3.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// Search, Android devices and "Open in editor" at the right, with the subagents card under them,
/// floating over the transcript, which scrolls under them.
///
/// Only the controls and the card take
/// clicks; the gaps between them pass through to the transcript.
pub fn header<W: ChatWindow>(
    w: &mut W,
    v: &crate::ChatView<W>,
    t: &Theme,
    window: &Window,
    cx: &mut Context<W>,
) -> Div {
    let lineage = subagent_list::lineage_card(w, t, window, cx);
    let hover = t.bg_hover;
    // A task with its own worktree opens that; a project on an SSH host has no folder here
    // for an editor to open.
    let open = w
        .project(cx)
        .map(|p| w.chat_folder().unwrap_or_else(|| p.path.clone()))
        .filter(|root| !patchwork_platform::ssh::is_remote(root))
        .map(|root| {
            button("open-in-editor", "Open in editor")
                .h(px(28.))
                .gap(px(6.))
                .px(px(12.))
                .rounded(px(radius()))
                .border_1()
                .border_color(t.border)
                .bg(t.bg_panel)
                .text_size(px(fs_sm()))
                .shadow(shadow())
                .hover(move |s| s.bg(hover))
                .child(icon(IconName::Code, 14.))
                .child("Open in editor")
                .on_click(cx.listener(move |_, _, _, cx| {
                    let root = root.clone();
                    cx.background_spawn(async move {
                        if let Err(e) = patchwork_platform::open_in_editor(&root, "", None) {
                            log::warn!(error:% = e; "could not open the editor: {{error}}");
                        }
                    })
                    .detach();
                }))
        });
    // An agent's chat without a project opens the agent's own folder.
    let agent = w.chat_agent(cx).map(|b| b.id.clone());
    let open = open.or_else(|| {
        let id = agent.clone()?;
        let project = w.chat.as_ref().and_then(|t| t.project_id.clone());
        Some(
            button("open-in-editor", "Open in editor")
                .h(px(28.))
                .gap(px(6.))
                .px(px(12.))
                .rounded(px(radius()))
                .border_1()
                .border_color(t.border)
                .bg(t.bg_panel)
                .text_size(px(fs_sm()))
                .shadow(shadow())
                .hover(move |s| s.bg(hover))
                .child(icon(IconName::Code, 14.))
                .child("Open in editor")
                .on_click(cx.listener(move |w, _, _, cx| {
                    let (core, id, project) =
                        (std::sync::Arc::clone(&w.core), id.clone(), project.clone());
                    cx.background_spawn(async move {
                        if let Err(e) = core.open_agent_in_editor(&id, project.as_deref()) {
                            log::warn!(error:% = e; "could not open the editor: {{error}}");
                        }
                    })
                    .detach();
                })),
        )
    });
    let details = agent.map(|_| {
        let shown = w.agent_details;
        button("agent-details-toggle", "Agent details")
            .aria_expanded(shown)
            .flex_none()
            .size(px(28.))
            .justify_center()
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .shadow(shadow())
            .hover(move |s| s.bg(hover))
            .child(icon(IconName::GearSix, 14.))
            .on_click(cx.listener(|w, _, _, cx| w.toggle_agent_details(cx)))
    });
    let android = w.mobile.is_some().then(|| {
        let shown = w.android_open(cx);
        let fg = t.text;
        button("android-devices", "Android devices")
            .aria_expanded(shown)
            .flex_none()
            .size(px(28.))
            .justify_center()
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .text_color(t.text_muted)
            .shadow(shadow())
            .hover(move |s| s.bg(hover).text_color(fg))
            .child(icon(IconName::DeviceMobile, 14.))
            .on_click(cx.listener(|w, _, window, cx| w.toggle_android(window, cx)))
    });
    let terminal = w.can_continue_in_cli().then(|| {
        let fg = t.text;
        button("continue-in-cli", "Continue in terminal")
            .flex_none()
            .size(px(28.))
            .justify_center()
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .text_color(t.text_muted)
            .shadow(shadow())
            .hover(move |s| s.bg(hover).text_color(fg))
            .child(icon(IconName::Terminal, 14.))
            .on_click(cx.listener(|w, _, _, cx| w.continue_in_cli(cx)))
    });
    let bar = div()
        .block_mouse_except_scroll()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(find_bar::find_field(w, v, t, cx))
        .children(android)
        .children(terminal)
        .children(open)
        .children(details);
    div()
        .absolute()
        .top_0()
        .right_0()
        .pt(px(12.))
        .px(px(16.))
        .flex()
        .flex_col()
        .items_end()
        .gap(px(8.))
        .child(bar)
        .children(lineage)
}
