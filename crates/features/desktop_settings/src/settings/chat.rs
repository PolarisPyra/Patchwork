//! Settings → General, its chat sections: the agent a new chat starts with, how a chat is answered and steered, how many steps and tokens a turn of
//! Patchwork's own agent may take, what a launch does with replies the last session cut off, and
//! whether a task opens in the open tab or a new one.

// Rust guideline compliant 2026-11-23

use gpui::{AnyElement, prelude::*};
use patchwork_client::local::core::settings::{ResumeAtLaunch, Settings};

use crate::settings::SettingsDialog;
use crate::settings::controls::{Section, segmented, switch_row, token_row};
use patchwork_design_theme::theme;

const RESUMES: [(ResumeAtLaunch, &str); 3] = [
    (ResumeAtLaunch::Always, "Always"),
    (ResumeAtLaunch::Ask, "Ask"),
    (ResumeAtLaunch::Never, "Never"),
];

/// Where an opened task shows: in the open task's tab, or in a tab of its own.
const TAB_CHOICES: [(bool, &str); 2] = [(false, "Current tab"), (true, "New tab")];

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
        "show-thoughts",
        "Show thoughts",
        |s| s.show_thoughts,
        |s, on| s.show_thoughts = on,
    ),
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

/// The ids of the switches the Transcript section holds; the rest are about the agent.
const TRANSCRIPT: [&str; 4] = [
    "show-thoughts",
    "confirm-answers",
    "steer-while-working",
    "granular-chats",
];

/// The chat sections of the General page: what a new chat starts with, what the transcript
/// shows, and how far an agent may go on its own.
pub fn sections(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> Vec<AnyElement> {
    let t = *theme(cx);
    let default = (!d.settings.agents.is_empty())
        .then(|| token_row("Default agent", &t).child(d.default_picker(cx)));
    let s = &d.settings;
    let (mut transcript, mut agent) = (Vec::new(), Vec::new());
    for &(id, title, get, set) in &FLAGS {
        let row = switch_row(
            id,
            title,
            get(s),
            move |d, on, cx| d.update(|s| set(s, on), cx),
            cx,
        );
        if TRANSCRIPT.contains(&id) {
            transcript.push(row);
        } else {
            agent.push(row);
        }
    }
    let new_chats =
        Section::new("New chats")
            .children(default)
            .child(token_row("Open tasks in", &t).child(segmented(
                "tasks-in-new-tab",
                "Open tasks in",
                s.tasks_in_new_tab,
                &TAB_CHOICES,
                &t,
                |d, new_tab, cx| d.update(|s| s.tasks_in_new_tab = new_tab, cx),
                cx,
            )));
    let agent = Section::new("Agents")
        .children(agent)
        .child(token_row("Token limit per turn", &t).child(segmented(
            "token-limit",
            "Token limit per turn",
            s.turn_token_limit,
            &TOKEN_LIMITS,
            &t,
            |d, limit, cx| d.update(|s| s.turn_token_limit = limit, cx),
            cx,
        )))
        .child(
            token_row("Resume interrupted replies at launch", &t).child(segmented(
                "resume-at-launch",
                "Resume interrupted replies at launch",
                s.resume_at_launch,
                &RESUMES,
                &t,
                |d, resume, cx| d.update(|s| s.resume_at_launch = resume, cx),
                cx,
            )),
        );
    vec![
        new_chats.into_any_element(),
        Section::new("Transcript")
            .children(transcript)
            .into_any_element(),
        agent.into_any_element(),
    ]
}
