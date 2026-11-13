//! Settings → Chat: how a chat is answered and steered, how many steps and tokens a turn of
//! Patchwork's own agent may take, and what a launch does with replies the last session cut off.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, div, prelude::*};
use patchwork_server::settings::{ResumeAtLaunch, Settings};

use crate::settings::SettingsDialog;
use crate::settings::controls::{segmented, switch, token_row};
use crate::ui::theme::theme;

const RESUMES: [(ResumeAtLaunch, &str); 3] = [
    (ResumeAtLaunch::Always, "Always"),
    (ResumeAtLaunch::Ask, "Ask"),
    (ResumeAtLaunch::Never, "Never"),
];

/// The token limits a turn of Patchwork's own agent can have, with 0 for none.
const TOKEN_LIMITS: [(u64, &str); 5] = [
    (0, "Off"),
    (1_000_000, "1M"),
    (5_000_000, "5M"),
    (20_000_000, "20M"),
    (50_000_000, "50M"),
];

/// The switches of the pane: their id, their title, whether each is on, and how to set it.
type Flag = (
    &'static str,
    &'static str,
    fn(&Settings) -> bool,
    fn(&mut Settings, bool),
);
const FLAGS: [Flag; 6] = [
    (
        "confirm-answers",
        "Click answers twice to send",
        |s| s.confirm_answers,
        |s, on| s.confirm_answers = on,
    ),
    (
        "steer-while-working",
        "Enter steers a running reply",
        |s| s.steer_while_working,
        |s, on| s.steer_while_working = on,
    ),
    (
        "granular-chats",
        "Granular chats",
        |s| s.granular_chats,
        |s, on| s.granular_chats = on,
    ),
    (
        "cross-model-review",
        "Cross-model review",
        |s| !s.review_off,
        |s, on| s.review_off = !on,
    ),
    (
        "chat-tools",
        "Agents can start and message chats",
        |s| !s.chat_tools_off,
        |s, on| s.chat_tools_off = !on,
    ),
    (
        "step-limit",
        "Stop agent after 200 tool steps",
        |s| !s.unlimited_steps,
        |s, on| s.unlimited_steps = !on,
    ),
];

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    let s = &d.settings;
    let flags = FLAGS.iter().map(|&(id, title, get, set)| {
        let on = get(s);
        token_row(title, None, &t).child(
            switch(id, title, on, &t)
                .on_click(cx.listener(move |d, _, _, cx| d.update(|s| set(s, !on), cx))),
        )
    });
    let flags: Vec<_> = flags.collect();
    div()
        .flex()
        .flex_col()
        .children(flags)
        .child(token_row("Token limit per turn", None, &t).child(segmented(
            "token-limit",
            "Token limit per turn",
            s.turn_token_limit,
            &TOKEN_LIMITS,
            &t,
            |d, limit, cx| d.update(|s| s.turn_token_limit = limit, cx),
            cx,
        )))
        .child(
            token_row("Resume interrupted replies at launch", None, &t).child(segmented(
                "resume-at-launch",
                "Resume interrupted replies at launch",
                s.resume_at_launch,
                &RESUMES,
                &t,
                |d, resume, cx| d.update(|s| s.resume_at_launch = resume, cx),
                cx,
            )),
        )
        .into_any_element()
}
