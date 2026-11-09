//! What the project's own checks made of a reply's turn, under the files it changed
//! (`transcript/components/ChecksCard.tsx`). Shown only for a reply whose checks ran, or that the
//! person asked to check. "Passed" is only ever a command that exited zero having run something.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui::{AnyElement, Context, Div, Hsla, Role, SharedString, div, prelude::*, px};
use patchwork_server::threads::{CheckStatus, Message, TurnChecks};

use crate::ui::theme::{FONT_MONO, FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme};
use crate::ui::{BtnKind, btn, button, full_elapsed, icon, spinner, tooltip};
use crate::workspace::Workspace;

/// The cards' open details and output, and the runs asked for, by reply.
#[derive(Default)]
pub struct ChecksState {
    open: HashSet<String>,
    output_open: HashSet<String>,
    rerunning: HashSet<String>,
    errors: HashMap<String, String>,
}

impl Workspace {
    /// Runs the checks again for a reply. The result replaces the card's data as it arrives.
    pub(crate) fn rerun_checks(&mut self, message_id: String, cx: &mut Context<Self>) {
        let Some(thread_id) = self.thread.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        self.checks.errors.remove(&message_id);
        self.checks.rerunning.insert(message_id.clone());
        self.checks.open.insert(message_id.clone());
        let server = Arc::clone(&self.server);
        let id = message_id.clone();
        let task = cx.background_spawn(async move { server.run_checks(&thread_id, &id) });
        cx.spawn(async move |this, cx| {
            let started = task.await;
            let _ = this.update(cx, |this, cx| {
                this.checks.rerunning.remove(&message_id);
                if let Err(e) = started {
                    this.checks.errors.insert(message_id, e);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Turns the project's checks after every task on or off.
    fn set_project_checks(&mut self, project_id: &str, on: bool, cx: &mut Context<Self>) {
        self.settings
            .checks
            .projects
            .insert(project_id.to_string(), on);
        let id = project_id.to_string();
        if let Err(e) = self
            .server
            .store
            .update_settings(|s| _ = s.checks.projects.insert(id, on))
        {
            log::warn!("could not save the checks setting: {e}");
        }
        cx.notify();
    }
}

fn status_icon(status: CheckStatus, t: &Theme) -> AnyElement {
    let (name, color) = match status {
        CheckStatus::Running => return spinner(14., t.text_muted).into_any_element(),
        CheckStatus::Passed => ("check-circle", t.success),
        CheckStatus::Failed => ("x-circle", t.danger),
        CheckStatus::TimedOut => ("clock", t.warning),
        CheckStatus::NotRun => ("minus-circle", t.text_subtle),
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

pub fn checks_card(
    w: &Workspace,
    m: &Message,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Div> {
    let checks = m.checks.as_ref()?;
    let project_id = w.thread.as_ref()?.project_id.clone()?;
    let id = m.id.clone();
    let open = w.checks.open.contains(&id);
    let rerunning = w.checks.rerunning.contains(&id);
    let running = checks.status == CheckStatus::Running;
    let auto_on = w.settings.checks.on(&project_id);
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
            .font_family(FONT_MONO)
            .text_size(px(FS_XS))
            .text_color(t.text_muted)
            .tooltip(tooltip(command.clone()))
            .child(command)
    }))
    .child(
        icon(if open { "caret-down" } else { "caret-right" }, 12.)
            .flex_none()
            .text_color(t.text_subtle),
    )
    .on_click(cx.listener({
        let id = id.clone();
        move |w, _, _, cx| {
            if !w.checks.open.remove(&id) {
                w.checks.open.insert(id.clone());
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
        .tooltip(tooltip(tip))
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
            .text_size(px(FS_XS))
            .text_color(t.text_muted)
            .child(note)
    });

    let details = open.then(|| {
        let reasons = if !checks.failures.is_empty() {
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
                                        .font_family(FONT_MONO)
                                        .text_size(px(FS_XS))
                                        .text_color(t.text_muted)
                                        .child(failure.detail.clone()),
                                )
                            })
                    })),
            )
        } else {
            checks.note.is_none().then(|| {
                div()
                    .text_size(px(FS_XS))
                    .text_color(t.text_muted)
                    .child(say_more(checks))
            })
        };
        let output = checks.output.clone().map(|output| {
            let shown = w.checks.output_open.contains(&id);
            let toggle = button(SharedString::from(format!("checks-output-{id}")), "Output")
                .aria_expanded(shown)
                .gap(px(4.))
                .text_size(px(FS_XS))
                .text_color(t.text_muted)
                .hover(move |s| s.text_color(fg))
                .child(icon(if shown { "caret-down" } else { "caret-right" }, 11.))
                .child("Output")
                .on_click(cx.listener({
                    let id = id.clone();
                    move |w, _, _, cx| {
                        if !w.checks.output_open.remove(&id) {
                            w.checks.output_open.insert(id.clone());
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
                        .rounded(px(RADIUS_SM))
                        .bg(t.bg_editor)
                        .px(px(8.))
                        .py(px(6.))
                        .font_family(FONT_MONO)
                        .text_size(px(FS_XS))
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
            .child(div().flex().items_center().text_size(px(FS_XS)).child(auto))
    });

    let error = w.checks.errors.get(&id).map(|e| {
        div()
            .id(SharedString::from(format!("checks-error-{id}")))
            .role(Role::Alert)
            .text_size(px(FS_XS))
            .text_color(t.danger)
            .child(e.clone())
    });

    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .px(px(12.))
            .py(px(8.))
            .text_size(px(FS_SM))
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
