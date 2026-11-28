//! The rows of the changed-files list, and the groups they sit in.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Div, FontWeight, Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_client::local::core::source_control::{DiffSide, FileChange, RepoAction};

use patchwork_design_icons::IconName;
use patchwork_design_theme::{fs_md, fs_sm, radius_sm, Theme};
use patchwork_design_text::{base_name, dir_name};
use patchwork_design_components::{button, tooltip};
use patchwork_design_icons::icon;
use crate::host::SourceControlEvent;
use super::git_status::ChangesView;

/// The status letter, its name, and its colour.
fn status_of(status: &str, t: &Theme) -> (&'static str, &'static str, gpui::Hsla) {
    match status {
        "added" => ("A", "Added", t.success),
        "deleted" => ("D", "Deleted", t.danger),
        "renamed" => ("R", "Renamed", t.accent_text),
        "untracked" => ("U", "Untracked", t.success),
        "conflicted" => ("!", "Conflict", t.danger),
        _ => ("M", "Modified", t.warning),
    }
}

/// A small icon button for a row or group, shown on hover.
pub(super) fn row_action(
    id: impl Into<SharedString>,
    label: &str,
    name: IconName,
    group: SharedString,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(gpui::ElementId::Name(id.into()), label.to_string())
        .size(px(20.))
        .flex_none()
        .justify_center()
        .rounded(px(radius_sm()))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .tooltip(tooltip(label.to_string()))
        .opacity(0.)
        .group_hover(group, |s| s.opacity(1.))
        .focus(|s| s.opacity(1.))
        .child(icon(name, 14.))
}

/// A small text button beside a group's name: "Resolve with agent", "Project".
pub(super) fn group_button(
    id: &'static str,
    label: &'static str,
    lead: AnyElement,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(id, label)
        .flex_none()
        .gap(px(4.))
        .rounded(px(radius_sm()))
        .px(px(6.))
        .py(px(2.))
        .text_size(px(fs_sm()))
        .text_color(t.text_subtle)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(lead)
        .child(label)
}

/// A group of files under its name, its actions, and a count.
pub(super) fn group_header(
    title: &'static str,
    count: usize,
    actions: Vec<AnyElement>,
    t: &Theme,
) -> Div {
    let name = SharedString::from(format!("group:{title}"));
    div().flex().flex_col().px(px(4.)).child(
        div()
            .id(name.clone())
            .group(name)
            .role(Role::Group)
            .aria_label(title)
            .h(px(22.))
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(4.))
            .text_size(px(fs_md()))
            .child(div().flex_1().min_w_0().truncate().child(title))
            .children(actions)
            .child(
                div()
                    .min_w(px(18.))
                    .rounded_full()
                    .bg(t.bg_selected_muted)
                    .px(px(6.))
                    .flex()
                    .justify_center()
                    .text_size(px(fs_sm()))
                    .line_height(px(18.))
                    .child(count.to_string()),
            ),
    )
}

impl ChangesView {
    /// One file (`ChangeRow`): its name and folder, the actions shown on hover, its line counts,
    /// and its status letter. Opens its diff, or the `side` of it the row is for when the file is
    /// in both groups, or its diff from commit `since` for a file the task's branch changed.
    pub(super) fn change_row(
        &self,
        c: &FileChange,
        side: DiffSide,
        since: Option<String>,
        full_access: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (letter, label, color) = status_of(&c.status, t);
        let deleted = c.status == "deleted";
        // A file can be both staged and not, so it can be in two groups at once.
        let shown = side;
        let side = match (&since, c.staged) {
            (Some(_), _) => "branch",
            (None, true) => "staged",
            (None, false) => "unstaged",
        };
        // Where it was renamed from, or how it conflicts, said with its status.
        let label = match (&c.renamed_from, &c.conflict) {
            (_, Some(kind)) => format!("Conflict: {kind}"),
            (Some(from), None) => format!("Renamed from {from}"),
            (None, None) => label.to_string(),
        };
        // A file in both groups counts each group's lines.
        let (additions, deletions) = match (shown, c.parts) {
            (DiffSide::Staged, Some((staged, _))) => staged,
            (DiffSide::Unstaged, Some((_, unstaged))) => unstaged,
            _ => (c.additions, c.deletions),
        };
        let untracked = c.status == "untracked";
        let group = SharedString::from(format!("change:{side}:{}", c.path));
        let hover = t.bg_hover;
        let path = c.path.clone();
        let open = path.clone();
        let view = cx.weak_entity();
        // A staged file goes back to its last commit; a change not staged goes back to what is
        // staged, which it keeps.
        let undo: fn(&str, &str) -> Result<(), patchwork_client::local::core::Error> = if c.staged {
            patchwork_client::local::core::source_control::revert_file
        } else {
            patchwork_client::local::core::source_control::discard_unstaged
        };
        let actions = (!full_access).then(|| {
            let (discard, stage, ignore) = (path.clone(), path.clone(), path.clone());
            div()
                .flex()
                .flex_none()
                .child(
                    row_action(
                        format!("discard:{side}:{path}"),
                        "Discard changes",
                        IconName::Trash,
                        group.clone(),
                        t,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.discard(vec![discard.clone()], undo, window, cx);
                    })),
                )
                .when(untracked, |d| {
                    d.child(
                        row_action(
                            format!("ignore:{path}"),
                            "Add to .gitignore",
                            IconName::EyeSlash,
                            group.clone(),
                            t,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            let path = ignore.clone();
                            this.act("Ignoring…", RepoAction::Ignore { path }, cx);
                        })),
                    )
                })
                .child(if c.staged {
                    row_action(
                        format!("unstage:{path}"),
                        "Unstage changes",
                        IconName::Minus,
                        group.clone(),
                        t,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.unstage(vec![stage.clone()], cx);
                    }))
                } else {
                    row_action(
                        format!("stage:{path}"),
                        "Stage changes",
                        IconName::Plus,
                        group.clone(),
                        t,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.stage(vec![stage.clone()], window, cx);
                    }))
                })
        });
        div()
            .id(group.clone())
            .group(group)
            .h(px(22.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(radius_sm()))
            .pr(px(6.))
            .pl(px(20.))
            .hover(move |s| s.bg(hover))
            .child(
                button(
                    SharedString::from(format!("open:{side}:{path}")),
                    format!("{path} • {label}"),
                )
                .flex_1()
                .min_w_0()
                .items_baseline()
                .gap(px(6.))
                .text_size(px(fs_md()))
                .tooltip(tooltip(format!("{path} • {label}")))
                .child(
                    div()
                        .flex_none()
                        .max_w_full()
                        .truncate()
                        .text_color(if deleted { t.text_muted } else { t.text })
                        .when(deleted, gpui::Styled::line_through)
                        .child(base_name(&path).to_string()),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(fs_sm()))
                        .text_color(t.text_subtle)
                        .child(dir_name(&path).to_string()),
                )
                .on_click(move |_, _, cx| {
                    let path = open.clone();
                    let event = match since.clone() {
                        Some(since) => SourceControlEvent::OpenChangeSince { path, since },
                        None => SourceControlEvent::OpenChange {
                            path,
                            side: Some(shown),
                        },
                    };
                    let _ = view.update(cx, |_, cx| cx.emit(event));
                }),
            )
            .children(actions)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap(px(4.))
                    .text_size(px(fs_sm()))
                    .child(
                        div()
                            .min_w(px(28.))
                            .flex()
                            .justify_end()
                            .text_color(t.success)
                            .child(format!("+{additions}")),
                    )
                    .child(
                        div()
                            .min_w(px(28.))
                            .flex()
                            .justify_end()
                            .text_color(t.danger)
                            .child(format!("−{deletions}")),
                    ),
            )
            .child(
                div()
                    .id("status")
                    .role(Role::Image)
                    .aria_label(label)
                    .w(px(12.))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .text_size(px(fs_sm()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(letter),
            )
            .into_any_element()
    }
}
