//! The rows of the changed-files list, and the groups they sit in.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Div, FontWeight, Role, SharedString, Stateful, div, prelude::*, px};
use patchwork_server::workspace::FileChange;

use crate::ui::theme::{RADIUS_SM, Theme};
use crate::ui::{base_name, button, dir_name, icon, tooltip};
use crate::workspace::Workspace;
use super::state::{ChangesView, FS_SM, FS_XS};

/// The focused reply's files, listed whether or not they still have uncommitted changes.
pub(super) fn reply_files(w: &Workspace) -> Vec<String> {
    let Some((thread, message)) = &w.change_focus else {
        return Vec::new();
    };
    w.threads
        .iter()
        .chain(w.thread.iter())
        .find(|t| &t.id == thread)
        .and_then(|t| t.messages.iter().find(|m| &m.id == message))
        .map(|m| m.files.clone())
        .unwrap_or_default()
}

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
    name: &str,
    group: SharedString,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(gpui::ElementId::Name(id.into()), label.to_string())
        .size(px(20.))
        .flex_none()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .tooltip(tooltip(label.to_string()))
        .invisible()
        .group_hover(group, gpui::Styled::visible)
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
        .rounded(px(RADIUS_SM))
        .px(px(6.))
        .py(px(2.))
        .text_size(px(FS_XS))
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
            .text_size(px(FS_SM))
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
                    .text_size(px(FS_XS))
                    .line_height(px(18.))
                    .child(count.to_string()),
            ),
    )
}

impl ChangesView {
    /// One file (`ChangeRow`): its name and folder, the actions shown on hover, its line counts,
    /// and its status letter. Opens its diff.
    pub(super) fn change_row(
        &self,
        c: &FileChange,
        full_access: bool,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (letter, label, color) = status_of(&c.status, t);
        let deleted = c.status == "deleted";
        let group = SharedString::from(format!("change:{}", c.path));
        let hover = t.bg_hover;
        let path = c.path.clone();
        let open = path.clone();
        let workspace = self.workspace.clone();
        let actions = (!full_access).then(|| {
            let (discard, stage) = (path.clone(), path.clone());
            div()
                .flex()
                .flex_none()
                .child(
                    row_action(
                        format!("discard:{path}"),
                        "Discard changes",
                        "arrow-counter-clockwise",
                        group.clone(),
                        t,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.discard(vec![discard.clone()], window, cx);
                    })),
                )
                .when(!c.staged, |d| {
                    d.child(
                        row_action(
                            format!("stage:{path}"),
                            "Stage changes",
                            "plus",
                            group.clone(),
                            t,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.stage(vec![stage.clone()], cx);
                        })),
                    )
                })
        });
        div()
            .id(group.clone())
            .group(group)
            .h(px(22.))
            .flex()
            .items_center()
            .gap(px(4.))
            .rounded(px(RADIUS_SM))
            .pr(px(4.))
            .pl(px(20.))
            .hover(move |s| s.bg(hover))
            .child(
                button(
                    SharedString::from(format!("open:{path}")),
                    format!("{path} • {label}"),
                )
                .flex_1()
                .min_w_0()
                .items_baseline()
                .gap(px(6.))
                .text_size(px(FS_SM))
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
                        .text_size(px(FS_XS))
                        .text_color(t.text_subtle)
                        .child(dir_name(&path).to_string()),
                )
                .on_click(move |_, window, cx| {
                    let open = open.clone();
                    let _ = workspace.update(cx, |w, cx| w.open_change(&open, window, cx));
                }),
            )
            .children(actions)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap(px(4.))
                    .text_size(px(FS_XS))
                    .child(
                        div()
                            .min_w(px(28.))
                            .flex()
                            .justify_end()
                            .text_color(t.success)
                            .child(format!("+{}", c.additions)),
                    )
                    .child(
                        div()
                            .min_w(px(28.))
                            .flex()
                            .justify_end()
                            .text_color(t.danger)
                            .child(format!("−{}", c.deletions)),
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
                    .text_size(px(FS_XS))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(letter),
            )
            .into_any_element()
    }
}
