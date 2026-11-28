//! A file in conflict: its two sides marked in the code, and each conflict settled by taking one
//! side or both.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Hsla, Role, ScrollStrategy, SharedString, div, prelude::*, px};
use patchwork_client::local::core::source_control::{Conflict, Take, resolve_conflict};

use super::open_file::Canvas;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, font_ui, fs_xs, radius_sm};
use patchwork_design_components::{button, icon_button, tooltip};

impl Canvas {
    /// The conflict line `i` is in, and its place among the file's conflicts.
    fn conflict_at(&self, i: usize) -> Option<(usize, &Conflict)> {
        let at = self
            .conflicts
            .partition_point(|c| c.opens <= i)
            .checked_sub(1)?;
        self.conflicts
            .get(at)
            .filter(|c| i <= c.closes)
            .map(|c| (at, c))
    }

    /// The colour behind line `i` when it is part of a conflict: one for the branch's own lines,
    /// one for the incoming ones, and one for the markers between.
    pub(super) fn conflict_tint(&self, i: usize, t: &Theme) -> Option<Hsla> {
        let (_, conflict) = self.conflict_at(i)?;
        let own_end = conflict.ancestor.unwrap_or(conflict.divides);
        Some(if i > conflict.opens && i < own_end {
            t.diff_add_bg
        } else if i > conflict.divides && i < conflict.closes {
            t.bg_accent_soft
        } else {
            t.bg_panel
        })
    }

    /// The buttons on the line that opens a conflict, which settle it.
    pub(super) fn conflict_actions(
        &self,
        i: usize,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (index, conflict) = self.conflict_at(i).filter(|(_, c)| c.opens == i)?;
        let n = index + 1;
        let busy = self.busy;
        let (hover, fg) = (t.bg_hover, t.text);
        let side = |name: &str, fallback: &str| {
            if name.is_empty() {
                fallback.to_string()
            } else {
                name.to_string()
            }
        };
        let choices = [
            ("current", "Accept current", Take::Current),
            ("incoming", "Accept incoming", Take::Incoming),
            ("both", "Accept both", Take::Both),
        ];
        let about = [
            format!("Keep {}", side(&conflict.current, "this branch's lines")),
            format!("Keep {}", side(&conflict.incoming, "the incoming lines")),
            "Keep both sides, this branch's first".to_string(),
        ];
        Some(
            div()
                .id(("conflict-actions", i))
                .role(Role::Group)
                .aria_label(format!("Conflict {n} of {}", self.conflicts.len()))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .pr(px(12.))
                .font_family(font_ui())
                .text_size(px(fs_xs()))
                .children(
                    choices
                        .into_iter()
                        .zip(about)
                        .map(|((id, text, take), about)| {
                            button(
                                SharedString::from(format!("conflict-{n}-{id}")),
                                format!("{text} for conflict {n}"),
                            )
                            .rounded(px(radius_sm()))
                            .px(px(6.))
                            .py(px(2.))
                            .text_color(t.text_muted)
                            .tooltip(tooltip(about))
                            .when(busy, |b| b.opacity(0.5).cursor_default())
                            .when(!busy, |b| {
                                b.hover(move |s| s.bg(hover).text_color(fg))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.settle_conflict(index, take, cx);
                                    }))
                            })
                            .child(text)
                        }),
                )
                .into_any_element(),
        )
    }

    /// Settles conflict number `index` of the file by keeping `take`.
    fn settle_conflict(&mut self, index: usize, take: Take, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.run_action(
            move |root| {
                resolve_conflict(root, &path, index, take)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
            cx,
        );
    }

    /// Scrolls the next or the previous conflict to the top of the code.
    fn jump_to_conflict(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = self.conflicts.len();
        if count == 0 {
            return;
        }
        let at = match (self.conflict_shown, step > 0) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(at), true) => (at + 1) % count,
            (Some(at), false) => (at + count - 1) % count,
        };
        self.conflict_shown = Some(at);
        self.code_scroll
            .scroll_to_item(self.conflicts[at].opens, ScrollStrategy::Top);
        cx.notify();
    }

    /// How many conflicts the file has left, and the buttons that step through them.
    pub(super) fn conflict_nav(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let count = self.conflicts.len();
        if count == 0 {
            return None;
        }
        let left = if count == 1 {
            "1 conflict".to_string()
        } else {
            format!("{count} conflicts")
        };
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .id("conflicts-left")
                        .role(Role::Status)
                        .aria_label(left.clone())
                        .text_color(t.danger)
                        .child(left),
                )
                .child(
                    icon_button(
                        "previous-conflict",
                        "Previous conflict",
                        IconName::CaretUp,
                        16.,
                        cx,
                    )
                    .tooltip("Previous conflict")
                    .on_click(cx.listener(|this, _, _, cx| this.jump_to_conflict(-1, cx))),
                )
                .child(
                    icon_button(
                        "next-conflict",
                        "Next conflict",
                        IconName::CaretDown,
                        16.,
                        cx,
                    )
                    .tooltip("Next conflict")
                    .on_click(cx.listener(|this, _, _, cx| this.jump_to_conflict(1, cx))),
                )
                .into_any_element(),
        )
    }
}
