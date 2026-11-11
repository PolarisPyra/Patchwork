//! What the canvas's buttons do: stage, reject, open in the editor, copy, and review.

// Rust guideline compliant 2026-10-09

use std::time::Duration;
use gpui::{ClipboardItem, PromptLevel, Window, prelude::*, px};
use patchwork_server::workspace::HunkAction;
use crate::ui::base_name;
use super::diff::{Review, review_message};
use super::state::Canvas;

impl Canvas {
    /// Runs a git action on the file, then refreshes git's state and the panel.
    pub(super) fn run_action(
        &mut self,
        action: impl FnOnce(&str) -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.action_error = None;
        let root = self.root.clone();
        let task = cx.background_spawn(async move { action(&root) });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                if let Err(e) = done {
                    this.action_error = Some(e);
                }
                if let Some(w) = this.workspace.upgrade() {
                    w.update(cx, crate::workspace::Workspace::load_git);
                }
                this.load(cx);
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    pub(super) fn stage_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.run_action(
            move |root| {
                patchwork_server::workspace::accept_file(root, &path).map_err(|e| e.to_string())
            },
            cx,
        );
    }

    pub(super) fn reject(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            "Discard",
            Some(&format!("Discard changes to {}?", base_name(&path))),
            &["Discard", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.run_action(
                    move |root| {
                        patchwork_server::workspace::revert_file(root, &path)
                            .map_err(|e| e.to_string())
                    },
                    cx,
                );
            });
        })
        .detach();
    }

    /// Stages, unstages, or discards one change, asking before a discard.
    pub(super) fn act_on_hunk(
        &mut self,
        hunk: String,
        action: HunkAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let turn = self.turn(cx);
        let run = move |this: &mut Self, cx: &mut Context<Self>| {
            this.run_action(
                move |root| {
                    patchwork_server::workspace::apply_hunk(
                        root,
                        &path,
                        turn.as_ref(),
                        &hunk,
                        action,
                    )
                    .map_err(|e| e.to_string())
                },
                cx,
            );
        };
        if !matches!(action, HunkAction::Discard) {
            return run(self, cx);
        }
        let name = self.path.as_deref().map(base_name).unwrap_or_default();
        let answer = window.prompt(
            PromptLevel::Warning,
            "Discard",
            Some(&format!("Discard this change to {name}?")),
            &["Discard", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, run);
            }
        })
        .detach();
    }

    pub(super) fn open_in_editor(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.action_error = None;
        let root = self.root.clone();
        let task = cx.background_spawn(async move {
            patchwork_server::workspace::open_in_editor(&root, &path, None)
        });
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.action_error = Some(e.to_string());
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(super) fn copy(&mut self, cx: &mut Context<Self>) {
        let Some(Ok(content)) = &self.file else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(content.to_string()));
        self.copied = true;
        self.copied_seq += 1;
        let seq = self.copied_seq;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                // How long "Copied" shows before the button goes back to its icon.
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.copied_seq == seq {
                    this.copied = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Scrolls the next or previous change to the top of the view.
    pub(super) fn jump_to_change(&mut self, step: isize, cx: &mut Context<Self>) {
        let Some(parsed) = &self.diff_rows else {
            return;
        };
        // A change's header is item `row + 1` of the diff's list.
        let changes: Vec<usize> = parsed
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.kind == super::diff::Kind::Hunk)
            .map(|(row, _)| row + 1)
            .collect();
        let top = self.diff_list.logical_scroll_top();
        let target = if step > 0 {
            changes.iter().copied().find(|&item| item > top.item_ix)
        } else if changes.contains(&top.item_ix) && top.offset_in_item > px(1.) {
            Some(top.item_ix)
        } else {
            changes
                .iter()
                .rev()
                .copied()
                .find(|&item| item < top.item_ix)
        };
        if let Some(item_ix) = target {
            self.diff_list.scroll_to(gpui::ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
            cx.notify();
        }
    }

    pub(super) fn send_review(&mut self, cx: &mut Context<Self>) {
        let notes = cx
            .try_global::<Review>()
            .map(|r| r.notes(&self.project_id).to_vec())
            .unwrap_or_default();
        if notes.is_empty() {
            return;
        }
        let text = review_message(&notes);
        cx.default_global::<Review>().clear(&self.project_id);
        let project = self.project_id.clone();
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, |w, cx| w.send_review(&project, text, cx));
        }
        cx.notify();
    }
}
