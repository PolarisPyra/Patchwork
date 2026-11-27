//! The Issues view: the open project's GitHub issues, each with a button that starts a task on
//! it.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{ScrollHandle, prelude::*, AnyElement, Div, FontWeight, Role, SharedString, div, px};
use patchwork_backend::issues::Issue;
use patchwork_ui_theme::{Theme, font_mono, fs_md, fs_sm, fs_xs, radius_sm, theme};
use patchwork_ui_components::{icon_button, scrollbar, spinner, tooltip};

use crate::workspace::Workspace;

/// The repository's issue list.
#[derive(Default)]
pub enum Listed {
    /// Not asked for yet.
    #[default]
    Unread,
    Loading,
    Read(Vec<Issue>),
    Failed(String),
}

/// What the Issues view shows, kept between frames.
#[derive(Default)]
pub struct IssueLists {
    /// The project the GitHub list was read for.
    project: Option<String>,
    github: Listed,
    scroll: ScrollHandle,
}

impl Workspace {
    /// Reads the list the view shows if it has not been read, or was read for another project.
    fn load_issues(&mut self, cx: &mut Context<Self>) {
        let project = self.project().map(|p| p.id.clone());
        if self.issues.project != project {
            self.issues.project.clone_from(&project);
            self.issues.github = Listed::Unread;
        }
        if matches!(self.issues.github, Listed::Unread)
            && let Some(project) = project
        {
            self.issues.github = Listed::Loading;
            let core = Arc::clone(&self.core);
            let asked = project.clone();
            patchwork_ui_theme::off_thread(
                cx,
                move || core.github_issues(&asked),
                move |this, read, cx| {
                    // A list read for a project no longer open is not shown.
                    if this.issues.project.as_deref() == Some(project.as_str()) {
                        this.issues.github = match read {
                            Ok(issues) => Listed::Read(issues),
                            Err(e) => Listed::Failed(e.to_string()),
                        };
                        cx.notify();
                    }
                },
            );
        }
    }

    /// Reads the list again.
    fn refresh_issues(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.issues.github, Listed::Loading) {
            self.issues.github = Listed::Unread;
        }
        self.load_issues(cx);
        cx.notify();
    }

    /// Starts a task in the open project on `issue`, which the agent is asked to work on.
    fn start_on_issue(&mut self, issue: &Issue, cx: &mut Context<Self>) {
        if self.project().is_none() {
            self.error = Some("Open a project to start a task on an issue.".into());
            return cx.notify();
        }
        self.start_task(&issue.task_title(), issue.prompt(), cx);
    }
}

/// The Issues view.
pub fn issues_view(w: &mut Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    w.load_issues(cx);
    let t = *theme(cx);
    let heading = div()
        .h(px(32.))
        .flex_none()
        .flex()
        .items_center()
        .justify_between()
        .pl(px(12.))
        .pr(px(12.))
        .text_size(px(fs_md()))
        .font_weight(FontWeight::SEMIBOLD)
        .child("Issues")
        .child(
            icon_button(
                "issues-refresh",
                "Refresh issues",
                "arrows-clockwise",
                14.,
                cx,
            )
            .size(px(24.))
            .tooltip(tooltip("Refresh"))
            .on_click(cx.listener(|w, _, _, cx| w.refresh_issues(cx))),
        );
    let github = match w.project().map(|p| p.name.clone()) {
        Some(name) => section(
            "issues-github",
            format!("GitHub · {name}"),
            listed(&w.issues.github, &t, cx),
            &t,
        ),
        None => section(
            "issues-github",
            "GitHub".to_owned(),
            note("Open a project to see its repository's issues", &t).into_any_element(),
            &t,
        ),
    };
    let list = div()
        .id("issues")
        .track_scroll(&w.issues.scroll)
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(16.))
        .size_full()
        .p(px(8.))
        .child(github);
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(heading)
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(list)
                .children(scrollbar(&w.issues.scroll, 4., 0., t.border_strong)),
        )
}

/// The GitHub section: its label and its body.
fn section(id: &'static str, label: String, body: AnyElement, t: &Theme) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::Group)
        .aria_label(label.clone())
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .h(px(24.))
                .flex()
                .items_center()
                .px(px(6.))
                .text_size(px(fs_sm()))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.text_muted)
                .child(div().min_w_0().truncate().child(label)),
        )
        .child(body)
}

/// A quiet line in place of a list.
fn note(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .px(px(6.))
        .text_size(px(fs_sm()))
        .text_color(t.text_subtle)
        .child(text.into())
}

/// The list as loaded: a spinner, the error, the rows, or a line saying it is empty.
fn listed(list: &Listed, t: &Theme, cx: &mut Context<Workspace>) -> AnyElement {
    match list {
        Listed::Unread | Listed::Loading => div()
            .id("issues-loading-GitHub")
            .role(Role::Status)
            .aria_label("Loading GitHub issues")
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(6.))
            .h(px(28.))
            .text_size(px(fs_sm()))
            .text_color(t.text_subtle)
            .child(spinner(12., t.text_subtle))
            .child("Loading")
            .into_any_element(),
        Listed::Failed(e) => div()
            .id("issues-error-GitHub")
            .role(Role::Alert)
            .px(px(6.))
            .text_size(px(fs_sm()))
            .text_color(t.danger)
            .child(e.clone())
            .into_any_element(),
        Listed::Read(issues) if issues.is_empty() => note("No open issues", t).into_any_element(),
        Listed::Read(issues) => div()
            .id("issues-list-GitHub")
            .role(Role::List)
            .aria_label("GitHub issues")
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(issues.iter().map(|issue| row(issue, t, cx)))
            .into_any_element(),
    }
}

/// An issue's row: its title, key and labels, and the buttons that start a task on it and open
/// it in the browser. The buttons show on hover and on focus.
fn row(issue: &Issue, t: &Theme, cx: &mut Context<Workspace>) -> impl IntoElement + use<> {
    let id = format!("GitHub-{}", issue.key);
    let meta = std::iter::once(issue.key.clone())
        .chain(issue.labels.iter().cloned())
        .collect::<Vec<_>>()
        .join(" · ");
    let hover = t.bg_hover;
    let (start, open) = (issue.clone(), issue.url.clone());
    div()
        .id(SharedString::from(format!("issue-{id}")))
        .role(Role::ListItem)
        .aria_label(format!("{} {}", issue.key, issue.title))
        .group("issue-row")
        .flex()
        .items_center()
        .gap(px(4.))
        .min_h(px(44.))
        .px(px(6.))
        .py(px(4.))
        .rounded(px(radius_sm()))
        .hover(move |s| s.bg(hover))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(fs_sm()))
                        .text_color(t.text)
                        .child(issue.title.clone()),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_family(font_mono())
                        .text_size(px(fs_xs()))
                        .text_color(t.text_subtle)
                        .child(meta),
                ),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(2.))
                .child(
                    icon_button(
                        SharedString::from(format!("issue-start-{id}")),
                        format!("Start a task on {}", issue.key),
                        "play",
                        13.,
                        cx,
                    )
                    .size(px(24.))
                    .opacity(0.)
                    .group_hover("issue-row", |s| s.opacity(1.))
                    .focus(|s| s.opacity(1.))
                    .tooltip(tooltip("Start task"))
                    .on_click(cx.listener(move |w, _, _, cx| w.start_on_issue(&start, cx))),
                )
                .child(
                    icon_button(
                        SharedString::from(format!("issue-open-{id}")),
                        format!("Open {} in the browser", issue.key),
                        "arrow-square-out",
                        13.,
                        cx,
                    )
                    .size(px(24.))
                    .opacity(0.)
                    .group_hover("issue-row", |s| s.opacity(1.))
                    .focus(|s| s.opacity(1.))
                    .tooltip(tooltip("Open in browser"))
                    .on_click(move |_, _, cx| cx.open_url(&open)),
                ),
        )
}
