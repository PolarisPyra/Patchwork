//! The Issues view drawn: a section for the open project's GitHub issues and one for the Linear
//! issues assigned to the person, each row with "Start task" and "Open in browser".

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Div, FontWeight, Role, SharedString, div, prelude::*, px};
use patchwork_core::issues::{Issue, IssueSource};
use patchwork_ui::theme::{Theme, font_mono, fs_md, fs_sm, fs_xs, radius_sm, theme};
use patchwork_ui::{BtnKind, btn, icon, icon_button, scrollbar, spinner, tooltip};

use super::issue_lists::Listed;
use crate::workspace::Workspace;

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
        .pr(px(10.))
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
            None,
            listed(&w.issues.github, IssueSource::GitHub, &t, cx),
            &t,
        ),
        None => section(
            "issues-github",
            "GitHub".to_owned(),
            None,
            note("Open a project to see its repository's issues", &t).into_any_element(),
            &t,
        ),
    };
    let linear = if w.issues.linear_key == Some(true) {
        let forget = icon_button(
            "issues-linear-forget",
            "Remove Linear API key",
            "key",
            13.,
            cx,
        )
        .size(px(22.))
        .tooltip(tooltip("Remove Linear API key"))
        .on_click(cx.listener(|w, _, _, cx| w.forget_linear_key(cx)))
        .into_any_element();
        section(
            "issues-linear",
            "Linear · Assigned to you".to_owned(),
            Some(forget),
            listed(&w.issues.linear, IssueSource::Linear, &t, cx),
            &t,
        )
    } else {
        section(
            "issues-linear",
            "Linear".to_owned(),
            None,
            key_form(w, &t, cx),
            &t,
        )
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
        .child(github)
        .child(linear);
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

/// A tracker's section: its label, an action beside it, and its body.
fn section(
    id: &'static str,
    label: String,
    action: Option<AnyElement>,
    body: AnyElement,
    t: &Theme,
) -> impl IntoElement {
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
                .justify_between()
                .px(px(4.))
                .text_size(px(fs_sm()))
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.text_muted)
                .child(div().min_w_0().truncate().child(label))
                .children(action),
        )
        .child(body)
}

/// A quiet line in place of a list.
fn note(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .px(px(4.))
        .text_size(px(fs_sm()))
        .text_color(t.text_subtle)
        .child(text.into())
}

/// A tracker's list as loaded: a spinner, the error, the rows, or a line saying it is empty.
fn listed(
    list: &Listed,
    source: IssueSource,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let name = source.name();
    match list {
        Listed::Unread | Listed::Loading => div()
            .id(SharedString::from(format!("issues-loading-{name}")))
            .role(Role::Status)
            .aria_label(format!("Loading {name} issues"))
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(4.))
            .h(px(28.))
            .text_size(px(fs_sm()))
            .text_color(t.text_subtle)
            .child(spinner(12., t.text_subtle))
            .child("Loading")
            .into_any_element(),
        Listed::Failed(e) => div()
            .id(SharedString::from(format!("issues-error-{name}")))
            .role(Role::Alert)
            .px(px(4.))
            .text_size(px(fs_sm()))
            .text_color(t.danger)
            .child(e.clone())
            .into_any_element(),
        Listed::Read(issues) if issues.is_empty() => note("No open issues", t).into_any_element(),
        Listed::Read(issues) => div()
            .id(SharedString::from(format!("issues-list-{name}")))
            .role(Role::List)
            .aria_label(format!("{name} issues"))
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
    let id = format!("{}-{}", issue.source.name(), issue.key);
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

/// The form a Linear API key is added with, while none is saved.
fn key_form(w: &mut Workspace, t: &Theme, cx: &mut Context<Workspace>) -> AnyElement {
    let field = w.linear_key_field(cx);
    let empty = field.read(cx).content.trim().is_empty();
    let save = btn("issues-linear-save", "Save key", BtnKind::Primary, empty, t).child("Save key");
    let save = if empty {
        save
    } else {
        save.on_click(cx.listener(|w, _, _, cx| w.save_linear_key(cx)))
    };
    div()
        .id("issues-linear-key")
        .role(Role::Form)
        .aria_label("Add a Linear API key")
        .flex()
        .flex_col()
        .gap(px(8.))
        .px(px(4.))
        .text_size(px(fs_sm()))
        .child(
            div()
                .text_color(t.text_subtle)
                .child("Add a personal API key from Linear's settings to see the issues assigned to you. It is kept in the system's credential store."),
        )
        .child(
            div()
                .min_w_0()
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(t.border_strong)
                .bg(t.bg_input)
                .font_family(font_mono())
                .child(icon("key", 13.).flex_none().text_color(t.text_subtle))
                .child(div().flex_1().min_w_0().child(field)),
        )
        .children(w.issues.key_error.clone().map(|e| {
            div()
                .id("issues-linear-key-error")
                .role(Role::Alert)
                .text_color(t.danger)
                .child(e)
        }))
        .child(div().flex().justify_end().child(save))
        .into_any_element()
}
