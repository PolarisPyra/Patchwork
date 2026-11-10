//! The commit box with its button and menu, and the pull request bar under it.

// Rust guideline compliant 2026-10-09

use gpui::{ClickEvent, Div, Focusable, Role, SharedString, Stateful, Window, div, prelude::*, px};
use patchwork_server::workspace::PullRequest;

use crate::ui::menu::MenuItem;
use crate::ui::theme::{RADIUS_SM, Theme};
use crate::ui::{button, icon, spinner, tooltip};
use super::rows::group_button;
use super::state::{ChangesView, FS_SM, FS_XS, GitAction, git_actions};

impl ChangesView {
    /// The commit message box and the Commit button with its menu (`CommitBox`).
    pub(super) fn commit_box(
        &self,
        has_changes: bool,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let ((primary, primary_label), menu) = git_actions(self.sync.as_ref(), has_changes);
        let busy = self.busy.is_some();
        let lead_icon = if primary_label == "Publish Branch" {
            "cloud-arrow-up"
        } else {
            match primary {
                Some(GitAction::Push) => "arrow-up",
                Some(GitAction::OpenPr | GitAction::ViewPr) => "git-pull-request",
                _ => "check",
            }
        };
        let focused = self.message.read(cx).focus_handle(cx).is_focused(window);
        let lines = self.message.read(cx).content.lines().count().clamp(3, 10) as f32;
        let (hover, fg, accent_hover) = (t.bg_hover, t.text, t.accent_hover);
        let primary_enabled = primary.is_some() && !busy;
        let menu_enabled = !menu.is_empty() && !busy;
        let menu_open = self.menu.as_ref().is_some_and(|(_, _, c)| c.is_none());
        let pr_number = self
            .sync
            .as_ref()
            .and_then(|s| s.pull_request.as_ref())
            .map(|p| p.number);
        let write_enabled = has_changes && !busy;
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(8.))
            .pt(px(4.))
            .pb(px(8.))
            .child(
                div()
                    .relative()
                    .flex()
                    .rounded(px(RADIUS_SM))
                    .border_1()
                    .border_color(if focused { t.accent } else { t.border })
                    .bg(t.bg_input)
                    .child(
                        div()
                            .id("commit-message")
                            .flex_1()
                            .min_w_0()
                            .min_h(px(18. * 3. + 8.))
                            .max_h(px(18. * 10. + 8.))
                            .h(px(18. * lines + 8.))
                            .overflow_y_scroll()
                            .py(px(4.))
                            .pl(px(8.))
                            .pr(px(28.))
                            .text_size(px(FS_SM))
                            .line_height(px(18.))
                            .child(self.message.clone()),
                    )
                    .child(
                        button("write-message", "Write commit message")
                            .absolute()
                            .top(px(2.))
                            .right(px(2.))
                            .size(px(24.))
                            .justify_center()
                            .rounded(px(RADIUS_SM))
                            .text_color(t.text_muted)
                            .tooltip(tooltip("Write commit message"))
                            .when(write_enabled, |b| {
                                b.hover(move |s| s.bg(hover).text_color(fg))
                            })
                            .when(!write_enabled, |b| b.opacity(0.5).cursor_default())
                            .child(icon("sparkle", 14.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if write_enabled {
                                    this.write_message(cx);
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .h(px(28.))
                    .flex()
                    .child(
                        button(
                            "commit",
                            self.busy.clone().unwrap_or_else(|| primary_label.clone()),
                        )
                        .flex_1()
                        .min_w_0()
                        .justify_center()
                        .gap(px(6.))
                        .rounded_l(px(RADIUS_SM))
                        .bg(t.accent)
                        .px(px(8.))
                        .text_size(px(FS_SM))
                        .text_color(t.text_on_accent)
                        .when(primary_enabled, |b| b.hover(move |s| s.bg(accent_hover)))
                        .when(!primary_enabled, |b| b.opacity(0.5).cursor_default())
                        .child(if busy {
                            spinner(12., t.text_on_accent).into_any_element()
                        } else {
                            icon(lead_icon, 14.).into_any_element()
                        })
                        .child(
                            div()
                                .truncate()
                                .child(self.busy.clone().unwrap_or(primary_label)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(action) = primary.filter(|_| primary_enabled) {
                                this.run(action, cx);
                            }
                        })),
                    )
                    .child(
                        button("commit-actions", "More actions")
                            .h(px(28.))
                            .w(px(24.))
                            .justify_center()
                            .rounded_r(px(RADIUS_SM))
                            .border_l_1()
                            .border_color(gpui::black().opacity(0.2))
                            .bg(t.accent)
                            .text_color(t.text_on_accent)
                            .aria_expanded(menu_open)
                            .tooltip(tooltip("More actions"))
                            .when(menu_enabled, |b| b.hover(move |s| s.bg(accent_hover)))
                            .when(!menu_enabled, |b| b.opacity(0.5).cursor_default())
                            .child(icon("caret-down", 12.))
                            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                                if !menu_enabled || this.just_dismissed(e) {
                                    return;
                                }
                                let entity = cx.entity().downgrade();
                                let items = menu
                                    .iter()
                                    .map(|&action| {
                                        let label = match (action, pr_number) {
                                            (GitAction::ViewPr, Some(n)) => {
                                                format!("{} #{n}", action.label())
                                            }
                                            _ => action.label().to_string(),
                                        };
                                        let entity = entity.clone();
                                        MenuItem::new(label, move |_, cx| {
                                            let _ =
                                                entity.update(cx, |this, cx| this.run(action, cx));
                                        })
                                    })
                                    .collect();
                                let at = e.position() + gpui::point(px(-160.), px(12.));
                                this.open_menu("Commit actions", items, at, None, window, cx);
                            })),
                    ),
            )
            .when_some(self.result.clone(), |d, (text, pr)| {
                d.child(
                    div()
                        .id("commit-result")
                        .role(Role::Status)
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(2.))
                        .text_size(px(FS_XS))
                        .text_color(t.text_muted)
                        .child(div().min_w_0().truncate().child(text))
                        .when_some(pr, |d, pr| {
                            d.child(
                                button("open-pr", "Open")
                                    .flex_none()
                                    .text_color(t.accent_text)
                                    .hover(gpui::Styled::underline)
                                    .child("Open")
                                    .on_click(move |_, _, cx| cx.open_url(&pr.url)),
                            )
                        }),
                )
            })
            .when_some(self.error.clone(), |d, e| d.child(alert(e, FS_XS, t)))
    }

    /// The branch's open pull request: its checks, its comments, and sending them to the agent.
    pub(super) fn pull_request_bar(
        &self,
        pr: &PullRequest,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let checks = if pr.failed > 0 {
            Some(format!("{} failed", pr.failed))
        } else if pr.pending > 0 {
            Some(format!("{} running", pr.pending))
        } else if pr.passed > 0 {
            Some("Checks passed".to_string())
        } else {
            None
        };
        let comments = (pr.comments > 0).then(|| {
            format!(
                "{} {}",
                pr.comments,
                if pr.comments == 1 {
                    "comment"
                } else {
                    "comments"
                }
            )
        });
        let url = pr.url.clone();
        let number = pr.number;
        let failed = pr.failed > 0;
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(FS_XS))
                    .text_color(t.text_muted)
                    .child(
                        button("pr-link", format!("Pull request #{number}"))
                            .flex_none()
                            .gap(px(4.))
                            .text_color(t.text)
                            .hover(gpui::Styled::underline)
                            .tooltip(tooltip(url.clone()))
                            .child(icon("git-pull-request", 14.))
                            .child(format!("#{number}"))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
                    .child(
                        div()
                            .id("pr-status")
                            .role(Role::Status)
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when_some(checks.clone(), |d, c| {
                                d.child(div().when(failed, |d| d.text_color(t.danger)).child(c))
                            })
                            .when(checks.is_some() && comments.is_some(), |d| {
                                d.child("\u{a0}·\u{a0}")
                            })
                            .children(comments),
                    )
                    .when(pr.failed > 0 || pr.comments > 0, |d| {
                        let busy = self.pr_busy;
                        d.child(
                            group_button(
                                "send-feedback",
                                "Send to agent",
                                if busy {
                                    spinner(12., t.text_subtle).into_any_element()
                                } else {
                                    icon("paper-plane-right", 14.).into_any_element()
                                },
                                t,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if !this.pr_busy {
                                        this.send_feedback(number, cx);
                                    }
                                },
                            )),
                        )
                    }),
            )
            .when_some(self.pr_error.clone(), |d, e| d.child(alert(e, FS_XS, t)))
    }
}

/// An error under what failed.
fn alert(text: String, size: f32, t: &Theme) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("alert:{text}")))
        .role(Role::Alert)
        .px(px(2.))
        .text_size(px(size))
        .text_color(t.danger)
        .child(text)
}
