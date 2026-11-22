//! What floats over the top right of a chat's transcript: search, Android devices, "Open in
//! editor" and a bot's details, with the subagents card under them.

// Rust guideline compliant 2026-10-09

use gpui::{Div, Window, div, prelude::*, px};
use patchwork_ui::theme::{Theme, fs_sm, radius};
use patchwork_ui::{button, icon, tooltip};
use crate::workspace::Workspace;
use super::find;
use super::lineage;

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
/// floating over the transcript, which scrolls under them. Only the controls and the card take
/// clicks; the gaps between them pass through to the transcript.
pub(super) fn header(
    w: &mut Workspace,
    t: &Theme,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Div {
    let lineage = lineage::lineage_card(w, t, window, cx);
    let hover = t.bg_hover;
    // A task with its own worktree opens that; a project on an SSH host has no folder here
    // for an editor to open.
    let open = w
        .project()
        .map(|p| w.thread_folder().unwrap_or_else(|| p.path.clone()))
        .filter(|root| !patchwork_core::system::ssh::is_remote(root))
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
                .child(icon("code", 14.))
                .child("Open in editor")
                .on_click(cx.listener(move |_, _, _, cx| {
                    let root = root.clone();
                    cx.background_spawn(async move {
                        if let Err(e) = patchwork_core::workspace::open_in_editor(&root, "", None) {
                            log::warn!(error:% = e; "could not open the editor: {{error}}");
                        }
                    })
                    .detach();
                }))
        });
    // A bot's chat without a project opens the bot's own folder.
    let bot = w.chat_bot().map(|b| b.id.clone());
    let open = open.or_else(|| {
        let id = bot.clone()?;
        let project = w.thread.as_ref().and_then(|t| t.project_id.clone());
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
                .child(icon("code", 14.))
                .child("Open in editor")
                .on_click(cx.listener(move |w, _, _, cx| {
                    let (core, id, project) =
                        (std::sync::Arc::clone(&w.core), id.clone(), project.clone());
                    cx.background_spawn(async move {
                        if let Err(e) = core.open_bot_in_editor(&id, project.as_deref()) {
                            log::warn!(error:% = e; "could not open the editor: {{error}}");
                        }
                    })
                    .detach();
                })),
        )
    });
    let details = bot.map(|_| {
        let shown = w.bot_details;
        button("bot-details-toggle", "Bot details")
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
            .tooltip(tooltip("Bot details"))
            .child(icon("gear-six", 14.))
            .on_click(cx.listener(|w, _, _, cx| w.toggle_bot_details(cx)))
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
            .tooltip(tooltip("Android devices"))
            .child(icon("device-mobile", 14.))
            .on_click(cx.listener(|w, _, window, cx| w.toggle_android(window, cx)))
    });
    let bar = div()
        .block_mouse_except_scroll()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(find::find_field(w, t, cx))
        .children(android)
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
