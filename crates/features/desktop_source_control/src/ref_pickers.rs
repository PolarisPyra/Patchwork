//! The lists a repository action picks from: branches, tags and stash entries, each narrowed by
//! typing, and what is done with the one picked.

// Rust guideline compliant 2026-10-09

use gpui::{Pixels, Point, PromptLevel, Window, prelude::*};
use patchwork_client::local::core::source_control::{GitBranch, RepoAction};

use super::header_menu::item;
use super::git_status::ChangesView;
use patchwork_design_components::menu::MenuItem;

/// What a branch or tag picked from a list is for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pick {
    Checkout,
    Delete,
    Upstream,
    Merge,
    Rebase,
    DeleteTag,
}

impl Pick {
    fn title(self) -> &'static str {
        match self {
            Self::Checkout => "Check out",
            Self::Delete => "Delete branch",
            Self::Upstream => "Set upstream",
            Self::Merge => "Merge into the current branch",
            Self::Rebase => "Rebase the current branch onto",
            Self::DeleteTag => "Delete tag",
        }
    }

    fn offers(self, r: &GitBranch) -> bool {
        match self {
            Self::Checkout | Self::Merge | Self::Rebase => !r.current,
            Self::Delete => r.kind == "local" && !r.current,
            Self::Upstream => r.kind == "remote",
            Self::DeleteTag => r.kind == "tag",
        }
    }

    /// What to say when there is nothing to pick.
    fn none(self) -> &'static str {
        match self {
            Self::Upstream => "No remote branches",
            Self::DeleteTag => "No tags",
            _ => "No other branches",
        }
    }
}

/// What a stash entry picked from the list is for.
#[derive(Clone, Copy)]
pub(super) enum StashPick {
    Apply,
    Pop,
    Drop,
}

impl ChangesView {
    /// Opens a list to pick one of, which typing narrows.
    fn open_picker(
        &mut self,
        items: Vec<MenuItem>,
        label: &str,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_repo(items, label, at, window, cx);
        if let Some((menu, ..)) = &self.menu {
            menu.update(cx, |menu, _| menu.take_filter());
        }
    }

    /// Lists the branches and tags `pick` can take, each kind under a line, and acts on the one
    /// picked.
    pub(super) fn pick_ref(
        &mut self,
        pick: Pick,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let read = cx.background_spawn(async move {
            patchwork_client::local::core::source_control::git_branches(&root)
        });
        cx.spawn_in(window, async move |this, cx| {
            let refs = read.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let refs: Vec<GitBranch> = match refs {
                    Ok(refs) => refs.into_iter().filter(|r| pick.offers(r)).collect(),
                    Err(e) => {
                        this.error = Some(e.to_string());
                        return cx.notify();
                    }
                };
                if refs.is_empty() {
                    return this.set_result(Some((pick.none().into(), None)), cx);
                }
                let view = cx.entity().downgrade();
                let mut kind = refs.first().map(|r| r.kind.clone()).unwrap_or_default();
                let items = refs
                    .into_iter()
                    .map(|r| {
                        let name = r.name.clone();
                        let row = item(&view, r.name, move |this, window, cx| {
                            this.picked(pick, name.clone(), window, cx);
                        });
                        if r.kind == kind {
                            row
                        } else {
                            kind = r.kind;
                            row.separated()
                        }
                    })
                    .collect();
                this.open_picker(items, pick.title(), at, window, cx);
            });
        })
        .detach();
    }

    fn picked(&mut self, pick: Pick, name: String, window: &mut Window, cx: &mut Context<Self>) {
        match pick {
            Pick::Checkout => {
                let action = RepoAction::Checkout {
                    name,
                    detached: false,
                };
                self.act("Checking out…", action, cx);
            }
            Pick::Upstream => {
                self.act(
                    "Setting upstream…",
                    RepoAction::SetUpstream { upstream: name },
                    cx,
                );
            }
            Pick::Merge => self.act("Merging…", RepoAction::Merge { name }, cx),
            Pick::Rebase => self.act("Rebasing…", RepoAction::Rebase { onto: name }, cx),
            Pick::DeleteTag => self.confirm(
                "Delete tag",
                &format!("Delete the tag {name}? The remote keeps its own."),
                "Delete",
                move |this, cx| this.act("Deleting…", RepoAction::DeleteTag { name }, cx),
                window,
                cx,
            ),
            Pick::Delete => {
                let answer = window.prompt(
                    PromptLevel::Warning,
                    "Delete branch",
                    Some(&format!(
                        "Delete the branch {name}? Commits only it has are kept unless you \
                         delete it even if unmerged."
                    )),
                    &["Delete", "Delete Even If Unmerged", "Cancel"],
                    cx,
                );
                cx.spawn(async move |this, cx| {
                    let force = match answer.await {
                        Ok(0) => false,
                        Ok(1) => true,
                        _ => return,
                    };
                    let _ = this.update(cx, |this, cx| {
                        this.act("Deleting…", RepoAction::DeleteBranch { name, force }, cx);
                    });
                })
                .detach();
            }
        }
    }

    /// Lists the stash's entries, and applies, pops or drops the one picked.
    pub(super) fn pick_stash(
        &mut self,
        pick: StashPick,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let read = cx.background_spawn(async move {
            patchwork_client::local::core::source_control::git_stashes(&root)
        });
        cx.spawn_in(window, async move |this, cx| {
            let stashes = read.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let stashes = match stashes {
                    Ok(stashes) => stashes,
                    Err(e) => {
                        this.error = Some(e.to_string());
                        return cx.notify();
                    }
                };
                if stashes.is_empty() {
                    return this.set_result(Some(("Nothing is stashed".into(), None)), cx);
                }
                let view = cx.entity().downgrade();
                let items = stashes
                    .into_iter()
                    .map(|entry| {
                        let (index, message) = (entry.index, entry.message.clone());
                        item(&view, entry.message, move |this, window, cx| {
                            this.picked_stash(pick, index, &message, window, cx);
                        })
                    })
                    .collect();
                let label = match pick {
                    StashPick::Apply => "Apply stash",
                    StashPick::Pop => "Pop stash",
                    StashPick::Drop => "Drop stash",
                };
                this.open_picker(items, label, at, window, cx);
            });
        })
        .detach();
    }

    fn picked_stash(
        &mut self,
        pick: StashPick,
        index: usize,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match pick {
            StashPick::Apply | StashPick::Pop => {
                let drop = matches!(pick, StashPick::Pop);
                self.act("Applying…", RepoAction::StashApply { index, drop }, cx);
            }
            StashPick::Drop => self.confirm(
                "Drop stash",
                &format!("Delete the stash “{message}”? Its changes are lost."),
                "Drop",
                move |this, cx| this.act("Dropping…", RepoAction::StashDrop { index }, cx),
                window,
                cx,
            ),
        }
    }
}
