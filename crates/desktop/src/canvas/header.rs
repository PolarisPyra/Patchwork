//! The canvas's header and tabs.

// Rust guideline compliant 2026-10-09

use gpui::{FontWeight, Role, SharedString, div, prelude::*, px};
use patchwork_server::threads::Message;
use crate::ui::highlight::file_label;
use crate::ui::theme::{FS_LG, FS_SM, radius, Theme};
use crate::ui::{BtnKind, badge, base_name, btn, dir_name, icon, icon_button, tooltip};
use super::diff::Review;
use super::state::{Canvas, CanvasTab};

impl Canvas {
    pub(super) fn header(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let name = self
            .path
            .as_deref()
            .map_or("No file selected", base_name)
            .to_string();
        let subtitle = self.path.as_deref().map(|path| {
            let dir = dir_name(path);
            let mut parts: Vec<String> = Vec::new();
            if !dir.is_empty() {
                parts.push(dir.to_string());
            }
            parts.push(file_label(path));
            if let Some(Ok(_)) = &self.file {
                parts.push(format!("{} lines", self.lines.len()));
            }
            parts.join(" · ")
        });
        let review = cx
            .try_global::<Review>()
            .map_or(0, |r| r.notes(&self.project_id).len());
        let has_content = matches!(self.file, Some(Ok(_)));
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .flex_none()
            .py(px(14.))
            .px(px(20.))
            .border_b_1()
            .border_color(t.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_none()
                    .size(px(36.))
                    .rounded(px(8.))
                    .bg(t.bg_accent_soft)
                    .text_color(t.accent_text)
                    .child(icon("code", 18.)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .id("canvas-name")
                            .text_size(px(FS_LG))
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .when_some(self.path.clone(), |d, path| d.tooltip(tooltip(path)))
                            .child(name),
                    )
                    .children(subtitle.map(|s| {
                        div()
                            .truncate()
                            .text_size(px(FS_SM))
                            .text_color(t.text_muted)
                            .child(s)
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .flex_none()
                    .when(review > 0, |d| {
                        d.child(
                            btn(
                                "send-review",
                                format!("Send review ({review})"),
                                BtnKind::Primary,
                                false,
                                t,
                            )
                            .child(icon("chat-text", 14.))
                            .child(format!("Send review ({review})"))
                            .on_click(cx.listener(|this, _, _, cx| this.send_review(cx))),
                        )
                    })
                    .when(
                        self.path.is_some()
                            && !patchwork_server::system::remote::is_remote(&self.root),
                        |d| {
                            d.child(
                                btn("open-in-editor", "Open in editor", BtnKind::Plain, false, t)
                                    .child(icon("arrow-square-out", 14.))
                                    .child("Open in editor")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.open_in_editor(cx)),
                                    ),
                            )
                            .child({
                                let label = if self.copied { "Copied" } else { "Copy" };
                                btn("copy-file", label, BtnKind::Plain, !has_content, t)
                                    .child(icon(if self.copied { "check" } else { "copy" }, 14.))
                                    .child(label)
                                    .when(has_content, |b| {
                                        b.on_click(cx.listener(|this, _, _, cx| this.copy(cx)))
                                    })
                            })
                        },
                    )
                    .child(
                        icon_button("close-canvas", "Close canvas", "x", 16., cx)
                            .tooltip(tooltip("Close"))
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    ),
            )
    }

    pub(super) fn tabs(
        &self,
        message: Option<&Message>,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let active = self.active_tab();
        let plan = message.map_or(0, |m| m.plan.len());
        let change = self.change(cx);
        let diff_count = if self.turn(cx).is_some() {
            match &self.diff {
                Some(Ok(diff)) => diff
                    .lines()
                    .filter(|l| {
                        (l.starts_with('+') || l.starts_with('-'))
                            && !l.starts_with("+++ ")
                            && !l.starts_with("--- ")
                    })
                    .count(),
                _ => 0,
            }
        } else {
            change.map_or(0, |c| (c.additions + c.deletions) as usize)
        };
        let tabs: Vec<(CanvasTab, &str, usize)> = match (self.path.is_some(), self.preview()) {
            (false, _) => vec![(CanvasTab::Plan, "Plan", plan)],
            (true, Some(preview)) => vec![
                (CanvasTab::Plan, "Plan", plan),
                (
                    preview,
                    if preview == CanvasTab::Image {
                        "Image"
                    } else {
                        "Video"
                    },
                    0,
                ),
            ],
            (true, None) => vec![
                (CanvasTab::Plan, "Plan", plan),
                (CanvasTab::Code, "Code", 0),
                (CanvasTab::Diff, "Diff", diff_count),
            ],
        };
        let (fg, muted, selected) = (t.text, t.text_muted, t.bg_selected_muted);
        div()
            .id("canvas-tabs")
            .role(Role::TabList)
            .aria_label("File views")
            .flex()
            .items_center()
            .gap(px(4.))
            .flex_none()
            .h(px(40.))
            .px(px(12.))
            .border_b_1()
            .border_color(t.border)
            .children(tabs.into_iter().map(|(id, label, count)| {
                let on = active == id;
                let aria = if count > 0 {
                    format!("{label} ({count})")
                } else {
                    label.to_string()
                };
                div()
                    .id(SharedString::from(format!("canvas-tab-{label}")))
                    .role(Role::Tab)
                    .aria_label(aria)
                    .aria_selected(on)
                    .tab_index(0)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(28.))
                    .px(px(10.))
                    .rounded(px(radius()))
                    .cursor_pointer()
                    .whitespace_nowrap()
                    .text_color(if on { fg } else { muted })
                    .when(on, |d| d.bg(selected))
                    .hover(move |s| s.text_color(fg))
                    .child(label.to_string())
                    .when(count > 0, |d| {
                        let b = badge(count.to_string(), t);
                        d.child(if on {
                            b.bg(t.border_strong).text_color(t.text)
                        } else {
                            b
                        })
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = id;
                        cx.notify();
                    }))
            }))
            .when(active == CanvasTab::Diff, |d| {
                d.child(
                    div()
                        .ml_auto()
                        .flex()
                        .flex_none()
                        .gap(px(4.))
                        .child(
                            icon_button("previous-change", "Previous change", "caret-up", 16., cx)
                                .tooltip(tooltip("Previous change"))
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.jump_to_change(-1, cx)),
                                ),
                        )
                        .child(
                            icon_button("next-change", "Next change", "caret-down", 16., cx)
                                .tooltip(tooltip("Next change"))
                                .on_click(cx.listener(|this, _, _, cx| this.jump_to_change(1, cx))),
                        ),
                )
            })
    }

    /// The file's entry in the project's uncommitted changes.
    pub(super) fn change(
        &self,
        cx: &Context<Self>,
    ) -> Option<patchwork_server::workspace::FileChange> {
        let path = self.path.as_ref()?;
        let w = self.workspace.upgrade()?;
        w.read(cx).changes.iter().find(|c| &c.path == path).cloned()
    }
}
