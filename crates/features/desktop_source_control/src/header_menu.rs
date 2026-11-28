//! The repository's own actions, from the header's menu: fetching and syncing, and its branches,
//! stashes, tags and remotes, with the field that takes a name for one.

// Rust guideline compliant 2026-10-09

use gpui::{
    Div, Entity, Focusable, KeyDownEvent, Pixels, Point, Role, Stateful, WeakEntity, Window, div,
    prelude::*, px,
};
use patchwork_client::local::core::source_control::{CommitOptions, RepoAction};

use super::ref_pickers::{Pick, StashPick};
use super::git_status::{ChangesView, GitAction};
use patchwork_design_components::menu::MenuItem;
use patchwork_design_editor::{Changed, Submitted, TextArea};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{Theme, fs_md, radius_sm};
use patchwork_design_components::{button, tooltip};
use patchwork_design_icons::icon;

/// What marks the repository's menus among the view's menus.
pub(super) const REPO_MENU: &str = "repository";

/// What a typed name or message is for.
pub(super) enum Ask {
    /// A new branch, at the commit `from` or the one checked out.
    NewBranch {
        from: Option<String>,
    },
    RenameBranch,
    /// A stash entry's message, which may be left empty.
    Stash {
        untracked: bool,
        staged_only: bool,
    },
    /// A new tag, on the commit `at` or the one checked out.
    Tag {
        at: Option<String>,
    },
    RemoteName,
    RemoteUrl {
        name: String,
    },
}

impl Ask {
    fn label(&self) -> &'static str {
        match self {
            Self::NewBranch { .. } => "New branch name",
            Self::RenameBranch => "Branch name",
            Self::Stash { .. } => "Stash message",
            Self::Tag { .. } => "Tag name",
            Self::RemoteName => "Remote name",
            Self::RemoteUrl { .. } => "Remote URL",
        }
    }
}

/// The field a repository action waits on, and what it is for.
pub(super) struct Asking {
    field: Entity<TextArea>,
    what: Ask,
}

/// A menu row that runs `run` on the view.
pub(super) fn item(
    view: &WeakEntity<ChangesView>,
    label: impl Into<String>,
    run: impl Fn(&mut ChangesView, &mut Window, &mut Context<ChangesView>) + 'static,
) -> MenuItem {
    let view = view.clone();
    MenuItem::new(label.into(), move |window, cx| {
        let _ = view.update(cx, |this, cx| run(this, window, cx));
    })
}

impl ChangesView {
    /// Does `action` in the repository off the main thread, showing `busy` meanwhile and what it
    /// did after, then reads the changes, the branch and the graph again.
    pub fn act(&mut self, busy: &'static str, action: RepoAction, cx: &mut Context<Self>) {
        let (Some(core), Some(id)) = (self.core(cx), self.project_id.clone()) else {
            return;
        };
        let chat = self.chat_id(cx);
        let shown = self.root.clone();
        self.error = None;
        self.result = None;
        self.busy = Some(busy.into());
        cx.notify();
        let task =
            cx.background_spawn(async move { core.repo_action(&id, chat.as_deref(), action) });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            let _ = this.update(cx, |this, cx| {
                // Another task's folder shows its own state; this one's result is not its.
                if this.root == shown {
                    match done {
                        Ok(note) => this.set_result(Some((note, None)), cx),
                        Err(e) => this.error = Some(e.to_string()),
                    }
                    this.busy = None;
                }
                // A failed action may have changed the files too, as a merge that conflicts.
                this.refresh_all(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens the repository's menu at `at`.
    pub(super) fn open_repo_menu(
        &mut self,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity().downgrade();
        let items = vec![
            item(&view, "Fetch", |this, _, cx| {
                this.act("Fetching…", RepoAction::Fetch { all: false }, cx);
            })
            .icon(IconName::ArrowDown),
            item(&view, "Fetch All Remotes", |this, _, cx| {
                this.act("Fetching…", RepoAction::Fetch { all: true }, cx);
            }),
            item(&view, "Pull", |this, _, cx| {
                this.run(GitAction::Pull, None, CommitOptions::default(), cx);
            }),
            item(&view, "Push", move |this, window, cx| {
                this.start(GitAction::Push, at, window, cx);
            })
            .icon(IconName::ArrowUp),
            item(&view, "Sync", |this, _, cx| {
                this.act("Syncing…", RepoAction::Sync, cx);
            })
            .icon(IconName::ArrowsClockwise),
            item(&view, "Force Push…", |this, window, cx| {
                this.confirm(
                    "Force push",
                    "Replace the remote branch with yours? Commits it has that you don't are \
                     lost, unless someone pushed since you last fetched.",
                    "Force Push",
                    |this, cx| {
                        let action = RepoAction::Push {
                            remote: None,
                            force: true,
                        };
                        this.act("Pushing…", action, cx);
                    },
                    window,
                    cx,
                );
            })
            .danger(),
            item(&view, "Branch…", move |this, window, cx| {
                this.open_branch_menu(at, window, cx);
            })
            .icon(IconName::GitBranch)
            .separated(),
            item(&view, "Stash…", move |this, window, cx| {
                this.open_stash_menu(at, window, cx);
            }),
            item(&view, "Tag…", move |this, window, cx| {
                this.open_tag_menu(at, window, cx);
            }),
            item(&view, "Remote…", move |this, window, cx| {
                this.open_remote_menu(at, window, cx);
            }),
        ];
        self.open_repo(items, "Repository actions", at, window, cx);
    }

    pub(super) fn open_repo(
        &mut self,
        items: Vec<MenuItem>,
        label: &str,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_menu(label, items, at, Some(REPO_MENU.into()), window, cx);
    }

    fn open_branch_menu(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        let pick = |label: &str, pick: Pick| {
            item(&view, label, move |this, window, cx| {
                this.pick_ref(pick, at, window, cx);
            })
        };
        let items = vec![
            item(&view, "New Branch…", |this, window, cx| {
                this.ask(Ask::NewBranch { from: None }, window, cx);
            })
            .icon(IconName::Plus),
            pick("Check Out…", Pick::Checkout),
            item(&view, "Rename Branch…", |this, window, cx| {
                this.ask(Ask::RenameBranch, window, cx);
            }),
            pick("Delete Branch…", Pick::Delete).danger(),
            pick("Set Upstream…", Pick::Upstream).separated(),
            pick("Merge into Current…", Pick::Merge),
            pick("Rebase Current onto…", Pick::Rebase),
        ];
        self.open_repo(items, "Branch", at, window, cx);
    }

    fn open_stash_menu(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        let save = |label: &str, untracked: bool, staged_only: bool| {
            item(&view, label, move |this, window, cx| {
                let what = Ask::Stash {
                    untracked,
                    staged_only,
                };
                this.ask(what, window, cx);
            })
        };
        let pick = |label: &str, pick: StashPick| {
            item(&view, label, move |this, window, cx| {
                this.pick_stash(pick, at, window, cx);
            })
        };
        let items = vec![
            save("Stash Changes…", false, false),
            save("Stash Including New Files…", true, false),
            save("Stash Staged…", false, true),
            pick("Apply Stash…", StashPick::Apply).separated(),
            pick("Pop Stash…", StashPick::Pop),
            pick("Drop Stash…", StashPick::Drop).danger(),
        ];
        self.open_repo(items, "Stash", at, window, cx);
    }

    fn open_tag_menu(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        let items = vec![
            item(&view, "New Tag…", |this, window, cx| {
                this.ask(Ask::Tag { at: None }, window, cx);
            })
            .icon(IconName::Plus),
            item(&view, "Delete Tag…", move |this, window, cx| {
                this.pick_ref(Pick::DeleteTag, at, window, cx);
            })
            .danger(),
            item(&view, "Push Tags", |this, _, cx| {
                this.act("Pushing tags…", RepoAction::PushTags, cx);
            })
            .separated(),
        ];
        self.open_repo(items, "Tag", at, window, cx);
    }

    fn open_remote_menu(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        let remotes = self
            .sync
            .as_ref()
            .map_or_else(Vec::new, |s| s.remotes.clone());
        let mut items = vec![
            item(&view, "Add Remote…", |this, window, cx| {
                this.ask(Ask::RemoteName, window, cx);
            })
            .icon(IconName::Plus),
        ];
        items.extend(remotes.into_iter().enumerate().map(|(i, name)| {
            let row = item(
                &view,
                format!("Remove {name}…"),
                move |this, window, cx| {
                    let name = name.clone();
                    this.confirm(
                        "Remove remote",
                        &format!("Remove the remote {name}, with the branches fetched from it?"),
                        "Remove",
                        move |this, cx| {
                            this.act("Removing…", RepoAction::RemoveRemote { name }, cx);
                        },
                        window,
                        cx,
                    );
                },
            )
            .danger();
            if i == 0 { row.separated() } else { row }
        }));
        self.open_repo(items, "Remote", at, window, cx);
    }

    /// Shows a field for the name or message `what` needs, and moves focus into it.
    pub(super) fn ask(&mut self, what: Ask, window: &mut Window, cx: &mut Context<Self>) {
        let label = what.label();
        let current = match &what {
            Ask::RenameBranch => self.sync.as_ref().and_then(|s| s.branch.clone()),
            _ => None,
        };
        let field = cx.new(|cx| {
            let mut field = TextArea::new(label, label, cx);
            field.single_line = true;
            if let Some(current) = &current {
                field.set_text(current, cx);
            }
            field
        });
        cx.subscribe_in(&field, window, |this, _, _: &Submitted, window, cx| {
            this.answer(window, cx);
        })
        .detach();
        cx.subscribe(&field, |_, _, _: &Changed, cx| cx.notify())
            .detach();
        window.focus(&field.read(cx).focus_handle(cx), cx);
        self.asking = Some(Asking { field, what });
        // The field is at the top of Changes, which may be closed or scrolled away.
        self.changes_open = true;
        self.list.scroll_to(gpui::ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        cx.notify();
    }

    /// Acts on what was typed. A blank name drops the question; a stash needs no message.
    fn answer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(asking) = self.asking.take() else {
            return;
        };
        let text = asking.field.read(cx).content.trim().to_string();
        match asking.what {
            Ask::Stash {
                untracked,
                staged_only,
            } => {
                let action = RepoAction::Stash {
                    message: text,
                    untracked,
                    staged_only,
                };
                self.act("Stashing…", action, cx);
            }
            _ if text.is_empty() => {}
            Ask::NewBranch { from } => {
                self.act(
                    "Creating…",
                    RepoAction::CreateBranch { name: text, from },
                    cx,
                );
            }
            Ask::RenameBranch => {
                self.act("Renaming…", RepoAction::RenameBranch { name: text }, cx);
            }
            Ask::Tag { at } => {
                let action = RepoAction::CreateTag {
                    name: text,
                    message: String::new(),
                    at,
                };
                self.act("Tagging…", action, cx);
            }
            Ask::RemoteName => self.ask(Ask::RemoteUrl { name: text }, window, cx),
            Ask::RemoteUrl { name } => {
                self.act("Adding…", RepoAction::AddRemote { name, url: text }, cx);
            }
        }
        cx.notify();
    }

    /// The field a repository action waits on, with a button that drops the question. Enter
    /// answers and Escape drops it.
    pub(super) fn ask_field(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let asking = self.asking.as_ref()?;
        let (hover, fg) = (t.bg_hover, t.text);
        Some(
            div()
                .id("repository-ask")
                .role(Role::Group)
                .aria_label(asking.what.label())
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(4.))
                .pl(px(8.))
                .pr(px(4.))
                .rounded(px(radius_sm()))
                .border_1()
                .border_color(t.accent)
                .bg(t.bg_input)
                .text_size(px(fs_md()))
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                    if e.keystroke.key == "escape" {
                        this.asking = None;
                        cx.stop_propagation();
                        cx.notify();
                    }
                }))
                .child(div().flex_1().min_w_0().child(asking.field.clone()))
                .child(
                    button("repository-ask-cancel", "Cancel")
                        .size(px(20.))
                        .flex_none()
                        .justify_center()
                        .rounded(px(radius_sm()))
                        .text_color(t.text_muted)
                        .hover(move |s| s.bg(hover).text_color(fg))
                        .tooltip(tooltip("Cancel"))
                        .child(icon(IconName::X, 12.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.asking = None;
                            cx.notify();
                        })),
                ),
        )
    }
}
