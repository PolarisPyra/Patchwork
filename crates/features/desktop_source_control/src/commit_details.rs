//! A commit in the graph: what its menu offers, and its message and files under its row.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, ClipboardItem, Role, SharedString, div, prelude::*, px};
use patchwork_client::local::core::source_control::{GitCommit, RepoAction};

use super::header_menu::{Ask, item};
use super::git_status::ChangesView;
use patchwork_design_components::menu::MenuItem;
use patchwork_design_components::spinner;
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, fs_sm};

/// How far a commit's details sit in from the graph's edge.
const DETAIL_INSET: f32 = 28.;

impl ChangesView {
    /// The rows of a commit's menu: checking it out, branching or tagging there, bringing it
    /// onto the branch or taking it back out, moving the branch to it, and copying it.
    pub fn commit_items(&self, commit: &GitCommit, cx: &mut Context<Self>) -> Vec<MenuItem> {
        let view = cx.entity().downgrade();
        let hash = commit.hash.clone();
        let act = |label: &str, busy: &'static str, action: RepoAction| {
            item(&view, label, move |this, _, cx| {
                this.act(busy, action.clone(), cx);
            })
        };
        let (branch_at, tag_at, reset_at) = (hash.clone(), hash.clone(), hash.clone());
        let (copy_hash, copy_subject) = (hash.clone(), commit.subject.clone());
        vec![
            act(
                "Check Out (Detached)",
                "Checking out…",
                RepoAction::Checkout {
                    name: hash.clone(),
                    detached: true,
                },
            ),
            item(&view, "New Branch Here…", move |this, window, cx| {
                let from = Some(branch_at.clone());
                this.ask(Ask::NewBranch { from }, window, cx);
            })
            .icon(IconName::GitBranch),
            item(&view, "New Tag Here…", move |this, window, cx| {
                let at = Some(tag_at.clone());
                this.ask(Ask::Tag { at }, window, cx);
            }),
            act(
                "Cherry-Pick",
                "Cherry-picking…",
                RepoAction::CherryPick { hash: hash.clone() },
            )
            .separated(),
            act(
                "Revert",
                "Reverting…",
                RepoAction::Revert { hash: hash.clone() },
            ),
            act(
                "Reset Branch Here, Keeping Changes",
                "Resetting…",
                RepoAction::Reset { hash, hard: false },
            ),
            item(
                &view,
                "Reset Branch Here, Discarding Changes…",
                move |this, window, cx| {
                    let hash = reset_at.clone();
                    this.confirm(
                        "Reset",
                        "Move the branch to this commit and discard every uncommitted change? \
                         The commits after it leave the branch.",
                        "Reset",
                        move |this, cx| {
                            this.act("Resetting…", RepoAction::Reset { hash, hard: true }, cx);
                        },
                        window,
                        cx,
                    );
                },
            )
            .danger(),
            MenuItem::new("Copy Hash", move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy_hash.clone()));
            })
            .icon(IconName::Copy)
            .separated(),
            MenuItem::new("Copy Message", move |_, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy_subject.clone()));
            }),
        ]
    }

    /// Whether the commit's details show under its row.
    #[must_use]
    pub fn detail_open(&self, hash: &str) -> bool {
        self.detail.as_ref().is_some_and(|(open, _)| open == hash)
    }

    /// Shows the commit's message and files under its row, or hides them again.
    pub fn toggle_detail(&mut self, hash: &str, cx: &mut Context<Self>) {
        if self.detail_open(hash) {
            self.detail = None;
            return cx.notify();
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let hash = hash.to_string();
        self.detail = Some((hash.clone(), None));
        cx.notify();
        let wanted = hash.clone();
        let read = cx.background_spawn(async move {
            patchwork_client::local::core::source_control::commit_detail(&root, &wanted)
        });
        cx.spawn(async move |this, cx| {
            let detail = read.await.map_err(|e| e.to_string());
            let _ = this.update(cx, |this, cx| {
                // Another commit may have been opened since.
                if let Some((open, slot)) = &mut this.detail
                    && *open == hash
                {
                    *slot = Some(detail);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The open commit's message, its author's address, and the files it changed.
    #[must_use]
    pub fn detail_view(&self, hash: &str, t: &Theme) -> Option<AnyElement> {
        let (_, detail) = self.detail.as_ref().filter(|(open, _)| open == hash)?;
        let frame = div()
            .id(SharedString::from(format!("detail:{hash}")))
            .flex()
            .flex_col()
            .gap(px(2.))
            .pl(px(DETAIL_INSET))
            .pr(px(8.))
            .pb(px(4.))
            .text_size(px(fs_sm()))
            .text_color(t.text_muted);
        let view = match detail {
            None => frame
                .role(Role::Status)
                .aria_label("Reading the commit")
                .child(spinner(12., t.text_subtle)),
            Some(Err(e)) => frame
                .role(Role::Alert)
                .text_color(t.danger)
                .child(e.clone()),
            Some(Ok(detail)) => frame
                .role(Role::Group)
                .aria_label("Commit details")
                .when(!detail.body.trim().is_empty(), |d| {
                    d.child(
                        div()
                            .text_color(t.text)
                            .child(detail.body.trim().to_string()),
                    )
                })
                .child(div().truncate().child(detail.email.clone()))
                .child(
                    div()
                        .id("files")
                        .role(Role::List)
                        .aria_label("Changed files")
                        .flex()
                        .flex_col()
                        .children(detail.files.iter().map(|file| {
                            div()
                                .id(SharedString::from(format!("file:{}", file.path)))
                                .role(Role::ListItem)
                                .aria_label(format!(
                                    "{}, {}, {} added, {} deleted",
                                    file.path, file.status, file.additions, file.deletions
                                ))
                                .flex()
                                .gap(px(6.))
                                .child(div().flex_1().min_w_0().truncate().child(file.path.clone()))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(t.success)
                                        .child(format!("+{}", file.additions)),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(t.danger)
                                        .child(format!("−{}", file.deletions)),
                                )
                        })),
                ),
        };
        Some(view.into_any_element())
    }
}
