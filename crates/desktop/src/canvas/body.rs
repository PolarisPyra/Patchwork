//! The canvas's body: the diff, the code, or the plan, and its footer.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use gpui::{
    AnyElement, FontWeight, ListHorizontalSizingBehavior, ObjectFit, Role, StyledText, div, img,
    prelude::*, px, relative, uniform_list,
};
use patchwork_server::threads::{Message, StepStatus};
use crate::ui::theme::{font_mono, FS_MD, FS_SM, Theme, theme};
use crate::ui::{BtnKind, base_name, btn, icon, spinner, tooltip};
use super::state::{Canvas, CanvasTab};

impl Canvas {
    pub(super) fn body(&mut self, message: Option<&Message>, cx: &mut Context<Self>) -> AnyElement {
        let t = *theme(cx);
        let active = self.active_tab();
        let path = match (&self.path, active) {
            (Some(path), tab) if tab != CanvasTab::Plan => path.clone(),
            _ => return plan_view(message, &t).into_any_element(),
        };
        match active {
            CanvasTab::Image => {
                let Some(media) = &self.media else {
                    return centered(div()).into_any_element();
                };
                return div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(16.))
                    .child(
                        img(PathBuf::from(&media.absolute))
                            .id("canvas-image")
                            .role(Role::Image)
                            .aria_label(base_name(&path).to_string())
                            .max_w_full()
                            .max_h_full()
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element();
            }
            CanvasTab::Video => {
                let Some(media) = &self.media else {
                    return centered(div()).into_any_element();
                };
                let file = PathBuf::from(&media.absolute);
                return centered(div())
                    .flex_col()
                    .gap(px(12.))
                    .text_color(t.text_muted)
                    .child(icon("film-strip", 32.))
                    .child(base_name(&path).to_string())
                    .child(
                        btn("play-video", "Play", BtnKind::Plain, false, &t)
                            .child(icon("play", 14.))
                            .child("Play")
                            .on_click(move |_, _, cx| cx.open_with_system(&file)),
                    )
                    .into_any_element();
            }
            _ => {}
        }
        let result = if active == CanvasTab::Code {
            &self.file
        } else {
            &self.diff
        };
        match result {
            None => centered(div())
                .text_color(t.text_subtle)
                .child(spinner(18., t.text_subtle))
                .into_any_element(),
            Some(Err(e)) => centered(div())
                .id("canvas-error")
                .text_color(t.text_muted)
                .tooltip(tooltip(e.clone()))
                .child(if active == CanvasTab::Code {
                    "This file can't be displayed. It may have been deleted or is binary."
                } else {
                    "The diff for this file could not be loaded."
                })
                .into_any_element(),
            Some(Ok(text)) if active == CanvasTab::Code => {
                if text.is_empty() {
                    return centered(div())
                        .text_color(t.text_muted)
                        .child("This file is empty.")
                        .into_any_element();
                }
                self.code_view(&t, cx).into_any_element()
            }
            Some(Ok(_)) => self.diff_view(&path, &t, cx),
        }
    }

    pub(super) fn code_view(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = *t;
        let (subtle, hover) = (t.text_subtle, crate::ui::mix(t.bg_hover, t.bg_editor, 40.));
        uniform_list(
            "canvas-code",
            self.lines.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, _| {
                range
                    .map(|i| {
                        let line = this.lines[i].clone();
                        let runs = this.runs.get(i).map_or_else(Vec::new, |runs| {
                            runs.iter()
                                .map(|(range, token)| (range.clone(), token.style(&theme)))
                                .collect()
                        });
                        div()
                            .id(i)
                            .flex()
                            .line_height(relative(1.65))
                            .hover(move |s| s.bg(hover))
                            .child(
                                div()
                                    .flex_none()
                                    .min_w(px(48.))
                                    .pr(px(16.))
                                    .flex()
                                    .justify_end()
                                    .text_color(subtle)
                                    .child((i + 1).to_string()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .whitespace_nowrap()
                                    .pr(px(16.))
                                    .child(StyledText::new(line).with_highlights(runs)),
                            )
                    })
                    .collect()
            }),
        )
        .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
        .with_width_from_item(self.widest)
        .track_scroll(&self.code_scroll)
        .size_full()
        .py(px(12.))
        .font_family(font_mono())
        .text_size(px(FS_MD))
        .line_height(relative(1.65))
        .text_color(t.text)
    }

    pub(super) fn footer(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        self.path.as_ref()?;
        let change = self.change(cx)?;
        let busy = self.busy;
        let state = if change.staged {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(icon("check", 14.).text_color(t.success))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.success)
                        .child("Changes applied"),
                )
        } else {
            div().text_color(t.text_muted).child(format!(
                "Unstaged changes · +{} −{}",
                change.additions, change.deletions
            ))
        };
        Some(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .flex_none()
                .py(px(12.))
                .px(px(20.))
                .border_t_1()
                .border_color(t.border)
                .bg(t.bg_panel)
                .child(div().min_w_0().text_size(px(FS_SM)).child(state))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .gap(px(8.))
                        .child(
                            btn("reject-file", "Reject", BtnKind::Plain, busy, t)
                                .child("Reject")
                                .when(!busy, |b| {
                                    b.on_click(
                                        cx.listener(|this, _, window, cx| this.reject(window, cx)),
                                    )
                                }),
                        )
                        .child(
                            btn("stage-file", "Stage file", BtnKind::Success, busy, t)
                                .child(icon("plus", 14.))
                                .child("Stage file")
                                .when(!busy, |b| {
                                    b.on_click(cx.listener(|this, _, _, cx| this.stage_file(cx)))
                                }),
                        ),
                ),
        )
    }
}

/// A message in the middle of the body.
pub(super) fn centered(d: gpui::Div) -> gpui::Div {
    d.size_full()
        .min_h(px(120.))
        .flex()
        .items_center()
        .justify_center()
        .p(px(24.))
        .text_center()
}

/// The reply's plan and the files it changed.
pub(super) fn plan_view(message: Option<&Message>, t: &Theme) -> gpui::Div {
    let steps = message.map(|m| m.plan.as_slice()).unwrap_or_default();
    let files = message.map(|m| m.files.as_slice()).unwrap_or_default();
    div()
        .py(px(16.))
        .px(px(20.))
        .child(if steps.is_empty() {
            centered(div())
                .text_color(t.text_muted)
                .child("The agent hasn't shared a plan for this task.")
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(steps.iter().map(|step| {
                    let mark = match step.status {
                        StepStatus::Done => icon("check", 14.)
                            .text_color(t.accent_text)
                            .into_any_element(),
                        StepStatus::Running => spinner(14., t.accent_text).into_any_element(),
                        StepStatus::Error => icon("x", 14.).text_color(t.danger).into_any_element(),
                        StepStatus::Pending => icon("circle", 14.)
                            .text_color(t.text_subtle)
                            .into_any_element(),
                    };
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .child(div().mt(px(4.)).flex_none().child(mark))
                        .child(div().min_w_0().child(step.text.clone()))
                }))
        })
        .when(!files.is_empty(), |d| {
            d.child(
                div()
                    .mt(px(24.))
                    .mb(px(8.))
                    .text_size(px(crate::ui::theme::FS_XS))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.text_subtle)
                    .child("FILES CHANGED"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .children(files.iter().enumerate().map(|(i, file)| {
                        div()
                            .id(i)
                            .py(px(4.))
                            .px(px(8.))
                            .font_family(font_mono())
                            .text_size(px(FS_SM))
                            .truncate()
                            .tooltip(tooltip(file.clone()))
                            .child(file.clone())
                    })),
            )
        })
}
