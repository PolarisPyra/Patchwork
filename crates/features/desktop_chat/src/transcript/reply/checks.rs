//! What the project's own checks made of a reply's turn, under the files it changed.
//!
//! Shown only for
//! a reply whose checks ran, or that the person asked to check. "Passed" is only ever a command
//! that exited zero having run something.

// Rust guideline compliant 2026-11-27

use crate::prelude::*;
use patchwork_client::AppState;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{AnyElement, Div, Hsla, Role, SharedString, div, prelude::*, px};
use patchwork_client::local::core::chats::{CheckStatus, Message, TurnChecks};

use patchwork_design_icons::IconName;
use patchwork_design_theme::{font_mono, fs_sm, fs_xs, radius, radius_sm, Theme};
use patchwork_design_components::{BtnKind, btn, button, spinner, tooltip};
use patchwork_design_text::full_elapsed;
use patchwork_design_icons::icon;

/// The cards' open details and output, and the runs asked for, by reply.
#[derive(Default)]
pub struct ChecksState {
    open: HashSet<String>,
    output_open: HashSet<String>,
    rerunning: HashSet<String>,
    errors: HashMap<String, String>,
}

/// The window's checks methods, for any [`ChatWindow`].
pub trait ChecksExt: ChatWindow {
    /// Runs the checks again for a reply. The result replaces the card's data as it arrives.
    fn rerun_checks(&mut self, message_id: String, cx: &mut Context<Self>) {
        let Some(chat_id) = self.chat.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        self.transcript.checks.errors.remove(&message_id);
        self.transcript.checks.rerunning.insert(message_id.clone());
        self.transcript.checks.open.insert(message_id.clone());
        let core = Arc::clone(&self.core);
        let id = message_id.clone();
        patchwork_design_tasks::off_thread(
            cx,
            move || core.run_checks(&chat_id, &id),
            move |this, started, cx| {
                this.transcript.checks.rerunning.remove(&message_id);
                if let Err(e) = started {
                    this.transcript
                        .checks
                        .errors
                        .insert(message_id, e.to_string());
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    /// Turns the project's checks after every task on or off.
    fn set_project_checks(&mut self, project_id: &str, on: bool, cx: &mut Context<Self>) {
        AppState::update(cx, |state| {
            state
                .settings
                .make_mut()
                .checks
                .projects
                .insert(project_id.to_string(), on);
        });
        let id = project_id.to_string();
        if let Err(e) = self
            .core
            .update_settings(|s| _ = s.checks.projects.insert(id, on))
        {
            log::warn!(error:% = e; "could not save the checks setting: {{error}}");
        }
        cx.notify();
    }
}

impl<W: ChatWindow> ChecksExt for W {}

fn status_icon(status: CheckStatus, t: &Theme) -> AnyElement {
    let (name, color) = match status {
        CheckStatus::Running => return spinner(14., t.text_muted).into_any_element(),
        CheckStatus::Passed => (IconName::CheckCircle, t.success),
        CheckStatus::Failed => (IconName::XCircle, t.danger),
        CheckStatus::TimedOut => (IconName::Clock, t.warning),
        CheckStatus::NotRun => (IconName::MinusCircle, t.text_subtle),
    };
    icon(name, 14.)
        .flex_none()
        .text_color(color)
        .into_any_element()
}

/// The row's words, and how they read.
fn say(checks: &TurnChecks, t: &Theme) -> (String, Hsla) {
    let ms = checks.duration_ms;
    // A test run that takes less than a second still took a moment; "0s" would read as nothing.
    let took = if ms >= 1000 {
        format!(" · {}", full_elapsed(ms))
    } else if ms > 0 {
        format!(" · {ms}ms")
    } else {
        String::new()
    };
    let counted = match checks.tests {
        Some(1) => " · 1 test".to_string(),
        Some(n) if n > 0 => format!(" · {n} tests"),
        _ => String::new(),
    };
    let failed = checks.failures.len();
    match checks.status {
        CheckStatus::Running => ("Running checks…".into(), t.text_muted),
        CheckStatus::Passed => (format!("Checks passed{counted}{took}"), t.success),
        CheckStatus::Failed if failed == 1 => (format!("1 check failed{took}"), t.danger),
        CheckStatus::Failed if failed > 0 => (format!("{failed} checks failed{took}"), t.danger),
        CheckStatus::Failed => (format!("Checks failed{took}"), t.danger),
        CheckStatus::TimedOut => ("Checks stopped before they finished".into(), t.warning),
        CheckStatus::NotRun => ("Checks not run".into(), t.text_muted),
    }
}

/// The line under the row, for a run that named nothing as failing.
fn say_more(checks: &TurnChecks) -> String {
    let ms = checks.duration_ms;
    let took = if ms >= 1000 {
        format!(" after {}", full_elapsed(ms))
    } else {
        String::new()
    };
    match (checks.status, &checks.command) {
        (CheckStatus::Running, Some(command)) => format!("Running {command} in the project…"),
        (CheckStatus::Running, None) => "Running checks…".into(),
        (CheckStatus::Passed, Some(command)) => format!("“{command}” exited zero{took}."),
        (CheckStatus::Passed, None) => "The command exited zero.".into(),
        (CheckStatus::TimedOut, _) => {
            "It was stopped before it finished, so the result is unknown.".into()
        }
        (CheckStatus::NotRun, _) => "Nothing ran.".into(),
        (CheckStatus::Failed, _) => {
            "The output named no failures; open it to see what it said.".into()
        }
    }
}

pub fn checks_card<W: ChatWindow>(
    w: &W,
    m: &Message,
    t: &Theme,
    cx: &mut Context<W>,
) -> Option<Div> {
    let checks = m.checks.as_ref()?;
    let project_id = w.chat.as_ref()?.project_id.clone()?;
    let id = m.id.clone();
    let open = w.transcript.checks.open.contains(&id);
    let rerunning = w.transcript.checks.rerunning.contains(&id);
    let running = checks.status == CheckStatus::Running;
    let auto_on = AppState::get(cx).settings.checks.on(&project_id);
    let (title, tone) = say(checks, t);
    let fg = t.text;

    let summary = button(
        SharedString::from(format!("checks-{id}")),
        if open { "Hide details" } else { "Show details" },
    )
    .aria_expanded(open)
    .min_w_0()
    .gap_x(px(8.))
    .tooltip(tooltip(if open { "Hide details" } else { "Show details" }))
    .child(status_icon(checks.status, t))
    .child(
        div()
            .flex_none()
            .whitespace_nowrap()
            .text_color(tone)
            .child(title),
    )
    .children(checks.command.clone().map(|command| {
        div()
            .id(SharedString::from(format!("checks-command-{id}")))
            .min_w_0()
            .truncate()
            .font_family(font_mono())
            .text_size(px(fs_xs()))
            .text_color(t.text_muted)
            .tooltip(tooltip(command.clone()))
            .child(command)
    }))
    .child(
        icon(
            if open {
                IconName::CaretDown
            } else {
                IconName::CaretRight
            },
            12.,
        )
        .flex_none()
        .text_color(t.text_subtle),
    )
    .on_click(cx.listener({
        let id = id.clone();
        move |w, _, _, cx| {
            if !w.transcript.checks.open.remove(&id) {
                w.transcript.checks.open.insert(id.clone());
            }
            cx.notify();
        }
    }));

    let rerun = (!running).then(|| {
        let label = if rerunning {
            "Starting…"
        } else if checks.status == CheckStatus::NotRun && checks.output.is_none() {
            "Run checks"
        } else {
            "Run again"
        };
        let tip = match &checks.command {
            Some(command) => format!("Run {command} in the project"),
            None => "Run the project's checks".into(),
        };
        let b = btn(
            SharedString::from(format!("checks-rerun-{id}")),
            label,
            BtnKind::Plain,
            rerunning,
            t,
        )
        .tooltip(tip)
        .when(rerunning, |b| b.child(spinner(13., t.text_muted)))
        .child(label);
        if rerunning {
            b
        } else {
            let id = id.clone();
            b.on_click(cx.listener(move |w, _, _, cx| w.rerun_checks(id.clone(), cx)))
        }
    });

    let note = checks.note.clone().map(|note| {
        div()
            .text_size(px(fs_xs()))
            .text_color(t.text_muted)
            .child(note)
    });

    let details = open.then(|| {
        let reasons = if checks.failures.is_empty() {
            checks.note.is_none().then(|| {
                div()
                    .text_size(px(fs_xs()))
                    .text_color(t.text_muted)
                    .child(say_more(checks))
            })
        } else {
            Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .children(checks.failures.iter().map(|failure| {
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(div().text_color(t.danger).child(failure.name.clone()))
                            .when(!failure.detail.is_empty(), |d| {
                                d.child(
                                    div()
                                        .font_family(font_mono())
                                        .text_size(px(fs_xs()))
                                        .text_color(t.text_muted)
                                        .child(failure.detail.clone()),
                                )
                            })
                    })),
            )
        };
        let output = checks.output.clone().map(|output| {
            let shown = w.transcript.checks.output_open.contains(&id);
            let toggle = button(SharedString::from(format!("checks-output-{id}")), "Output")
                .aria_expanded(shown)
                .gap(px(4.))
                .text_size(px(fs_xs()))
                .text_color(t.text_muted)
                .hover(move |s| s.text_color(fg))
                .child(icon(
                    if shown {
                        IconName::CaretDown
                    } else {
                        IconName::CaretRight
                    },
                    11.,
                ))
                .child("Output")
                .on_click(cx.listener({
                    let id = id.clone();
                    move |w, _, _, cx| {
                        if !w.transcript.checks.output_open.remove(&id) {
                            w.transcript.checks.output_open.insert(id.clone());
                        }
                        cx.notify();
                    }
                }));
            div().flex().flex_col().child(toggle).when(shown, |d| {
                d.child(
                    div()
                        .id(SharedString::from(format!("checks-output-text-{id}")))
                        .mt(px(6.))
                        .max_h(px(320.))
                        .overflow_y_scroll()
                        .rounded(px(radius_sm()))
                        .bg(t.bg_editor)
                        .px(px(8.))
                        .py(px(6.))
                        .font_family(font_mono())
                        .text_size(px(fs_xs()))
                        .child(output),
                )
            })
        });
        let project = project_id.clone();
        let auto = button(
            SharedString::from(format!("checks-auto-{id}")),
            if auto_on {
                "Stop running after every task"
            } else {
                "Run after every task"
            },
        )
        .text_color(t.text_muted)
        .hover(move |s| s.text_color(fg).underline())
        .child(if auto_on {
            "Stop running after every task"
        } else {
            "Run after every task"
        })
        .on_click(cx.listener(move |w, _, _, cx| w.set_project_checks(&project, !auto_on, cx)));
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .pt(px(2.))
            .children(reasons)
            .children(output)
            .child(
                div()
                    .flex()
                    .items_center()
                    .text_size(px(fs_xs()))
                    .child(auto),
            )
    });

    let error = w.transcript.checks.errors.get(&id).map(|e| {
        div()
            .id(SharedString::from(format!("checks-error-{id}")))
            .role(Role::Alert)
            .text_size(px(fs_xs()))
            .text_color(t.danger)
            .child(e.clone())
    });

    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .px(px(12.))
            .py(px(8.))
            .text_size(px(fs_sm()))
            .child(
                div()
                    .id(SharedString::from(format!("checks-row-{id}")))
                    .role(Role::Status)
                    .aria_label(say(checks, t).0)
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(px(12.))
                    .gap_y(px(8.))
                    .child(summary)
                    .child(div().flex_1())
                    .children(rerun),
            )
            .children(note)
            .children(details)
            .children(error),
    )
}
