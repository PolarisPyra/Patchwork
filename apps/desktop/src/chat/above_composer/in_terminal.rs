//! A chat carried on in its agent's own CLI: "Continue in terminal" resumes its session in a
//! terminal tab, and while it is there a strip above the composer offers to bring it back, which
//! reads the turns taken in the CLI into the chat. Sending a message brings it back as well.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{Div, FontWeight, Role, div, prelude::*, px};
use patchwork_backend::import::hands_off;
use patchwork_ui_theme::icon;
use patchwork_ui_theme::{Theme, fs_sm, fs_xs, radius_sm};

use super::goal_bar::strip_action;
use crate::workspace::Workspace;

impl Workspace {
    /// Whether the open chat can be carried on in its agent's CLI now: its agent's CLI can
    /// resume it, it has a session, and it is not there already.
    pub(crate) fn can_continue_in_cli(&self) -> bool {
        self.chat.as_ref().is_some_and(|c| {
            hands_off(c.agent) && c.session_id.is_some() && !self.in_cli.contains(&c.id)
        })
    }

    /// Resumes the open chat's session in its agent's CLI, in a new terminal tab.
    pub(crate) fn continue_in_cli(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.chat.as_ref().map(|c| c.id.clone()) else {
            return;
        };
        let core = Arc::clone(&self.core);
        let chat = id.clone();
        patchwork_ui_theme::off_thread(
            cx,
            move || core.continue_in_cli(&chat),
            move |this, handoff, cx| match handoff {
                Ok(handoff) => {
                    this.in_cli.insert(id);
                    this.show_in_terminal(
                        patchwork_backend::terminal::Launch {
                            shell: handoff.program,
                            args: handoff.args,
                            cwd: PathBuf::from(handoff.cwd),
                            env: BTreeMap::new(),
                            host: None,
                        },
                        format!("{} CLI", handoff.label),
                        cx,
                    );
                }
                Err(e) => this.error = Some(e.to_string()),
            },
        );
    }

    /// Reads the turns taken in the CLI into the open chat, which Patchwork then carries on.
    fn bring_back_from_cli(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.chat.as_ref().map(|c| c.id.clone()) else {
            return;
        };
        let core = Arc::clone(&self.core);
        let chat = id.clone();
        patchwork_ui_theme::off_thread(
            cx,
            move || core.bring_back_from_cli(&chat),
            move |this, back, cx| {
                match back {
                    Ok(_) => {
                        this.in_cli.remove(&id);
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                this.reload(cx);
            },
        );
    }
}

/// The strip above the composer while the open chat is carried on in a terminal.
pub fn in_terminal(w: &Workspace, t: &Theme, cx: &mut Context<Workspace>) -> Option<Div> {
    let chat = w.chat.as_ref().filter(|c| w.in_cli.contains(&c.id))?;
    let cli = if chat.agent == patchwork_backend::provider::AgentKind::Codex {
        "Codex"
    } else {
        "Claude Code"
    };
    Some(
        div()
            .mb(px(6.))
            .overflow_hidden()
            .rounded(px(radius_sm()))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_panel)
            .text_size(px(fs_sm()))
            .shadow_sm()
            .child(
                div()
                    .id("in-terminal")
                    .role(Role::Status)
                    .flex()
                    .h(px(36.))
                    .min_w_0()
                    .items_center()
                    .gap(px(8.))
                    .bg(t.bg_app)
                    .px(px(10.))
                    .child(icon("terminal", 13.).flex_none().text_color(t.text_subtle))
                    .child(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text)
                            .child(format!("Continued in {cli}")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(fs_xs()))
                            .text_color(t.text_subtle)
                            .child("Exit it in the terminal, then bring the chat back here"),
                    )
                    .child(
                        strip_action("bring-back-from-cli", "Bring back", t)
                            .on_click(cx.listener(|w, _, _, cx| w.bring_back_from_cli(cx))),
                    ),
            ),
    )
}
