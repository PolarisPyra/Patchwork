//! What Source Control holds, and reading it: the changed files, the branch and its history.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui::{Entity, Pixels, Point, ScrollHandle, WeakEntity, Window, prelude::*};
use patchwork_server::workspace::{GitCommit, GitSyncState, PullRequest};

use crate::ui::text_area::{Changed, Submitted, TextArea};
use crate::ui::menu::Menu;
use crate::ui::theme::theme;
use crate::workspace::Workspace;
use super::rows::{reply_files};

/// The sidebar's type scale.
pub(super) const FS_XS: f32 = 12.;
pub(super) const FS_SM: f32 = 13.;

/// The Graph's height while Changes is open too, and the least either pane is dragged to.
const GRAPH_HEIGHT: f32 = 260.;
pub(super) const PANE_MIN: f32 = 88.;
/// How long a finished action's note stays.
const RESULT_FOR: Duration = Duration::from_secs(10);
/// Commits read per page.
const PAGE: usize = 50;

/// What the Commit button and its menu offer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum GitAction {
    Commit,
    CommitPush,
    CommitPushPr,
    Push,
    OpenPr,
    ViewPr,
}

impl GitAction {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Commit => "Commit",
            Self::CommitPush => "Commit & Push",
            Self::CommitPushPr => "Commit, Push & Open Pull Request",
            Self::Push => "Push",
            Self::OpenPr => "Open Pull Request",
            Self::ViewPr => "View Pull Request",
        }
    }
}

/// The big button's action and label (None leaves it disabled), and the menu's actions.
pub(super) fn git_actions(
    state: Option<&GitSyncState>,
    has_changes: bool,
) -> ((Option<GitAction>, String), Vec<GitAction>) {
    let commit = || "Commit".to_string();
    let Some(state) = state.filter(|s| s.branch.is_some()) else {
        return (
            (has_changes.then_some(GitAction::Commit), commit()),
            Vec::new(),
        );
    };
    let on_default = state.branch == state.default_branch;
    let pr = state.pull_request.as_ref();
    let can_pr = state.has_remote && !on_default && state.default_branch.is_some() && pr.is_none();
    let can_push = state.has_remote && (!state.has_upstream || state.ahead > 0);
    let mut menu = Vec::new();
    if has_changes {
        menu.push(GitAction::Commit);
        if state.has_remote {
            menu.push(GitAction::CommitPush);
        }
        if can_pr {
            menu.push(GitAction::CommitPushPr);
        }
    }
    if can_push {
        menu.push(GitAction::Push);
    }
    if can_pr && state.has_upstream && !has_changes {
        menu.push(GitAction::OpenPr);
    }
    if pr.is_some() {
        menu.push(GitAction::ViewPr);
    }
    let primary = if has_changes {
        (Some(GitAction::Commit), commit())
    } else if can_push && !state.has_upstream {
        (Some(GitAction::Push), "Publish Branch".into())
    } else if can_push {
        (Some(GitAction::Push), format!("Push ↑{}", state.ahead))
    } else if can_pr && state.has_upstream {
        (Some(GitAction::OpenPr), "Open Pull Request".into())
    } else if let Some(pr) = pr {
        (
            Some(GitAction::ViewPr),
            format!("View Pull Request #{}", pr.number),
        )
    } else {
        (None, commit())
    };
    (primary, menu)
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate piece of view state"
)]
pub struct ChangesView {
    pub(super) workspace: WeakEntity<Workspace>,
    pub(super) project_id: Option<String>,
    pub(super) sync: Option<GitSyncState>,
    pub commits: Vec<GitCommit>,
    pages: usize,
    pub more: bool,
    pub log_error: Option<String>,
    pub(super) changes_open: bool,
    pub(super) graph_open: bool,
    pub(super) graph_height: f32,
    /// Where a divider drag started: the pointer's y and the Graph's height then.
    pub(super) resizing: Option<(f32, f32)>,
    pub(super) message: Entity<TextArea>,
    /// Unsent commit messages by project.
    pub(super) drafts: HashMap<String, String>,
    pub(super) busy: Option<String>,
    pub(super) result: Option<(String, Option<PullRequest>)>,
    /// Counts results, so an old one's timer leaves a newer one alone.
    result_seq: usize,
    pub(super) error: Option<String>,
    pub(super) resolving: bool,
    pub(super) pr_busy: bool,
    pub(super) pr_error: Option<String>,
    pub undo_error: Option<String>,
    /// The reply files that no longer exist.
    pub(super) missing: HashSet<String>,
    /// The commit actions menu, or a commit's undo menu with its commit, and where it opened.
    pub(super) menu: Option<(Entity<Menu>, Point<Pixels>, Option<String>)>,
    pub(super) dismissed: Option<Point<Pixels>>,
    pub(super) scroll: ScrollHandle,
    pub(super) graph_scroll: ScrollHandle,
}

impl ChangesView {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let t = *theme(cx);
        let message = cx.new(|cx| {
            let mut area = TextArea::new("Message (Ctrl+Enter to commit)", "Commit message", cx);
            area.text_color = t.text;
            area.placeholder_color = t.text_subtle;
            area.selection_color = t.accent.opacity(0.3);
            area.enter_newline = true;
            area
        });
        cx.subscribe_in(&message, window, |this, _, _: &Submitted, _, cx| {
            if this.has_changes(cx) && this.busy.is_none() {
                this.run(GitAction::Commit, cx);
            }
        })
        .detach();
        cx.subscribe(&message, |this, area, _: &Changed, cx| {
            if let Some(id) = &this.project_id {
                this.drafts
                    .insert(id.clone(), area.read(cx).content.clone());
            }
            cx.notify();
        })
        .detach();
        Self {
            workspace,
            project_id: None,
            sync: None,
            commits: Vec::new(),
            pages: 1,
            more: false,
            log_error: None,
            changes_open: true,
            graph_open: true,
            graph_height: GRAPH_HEIGHT,
            resizing: None,
            message,
            drafts: HashMap::new(),
            busy: None,
            result: None,
            result_seq: 0,
            error: None,
            resolving: false,
            pr_busy: false,
            pr_error: None,
            undo_error: None,
            missing: HashSet::new(),
            menu: None,
            dismissed: None,
            scroll: ScrollHandle::new(),
            graph_scroll: ScrollHandle::new(),
        }
    }

    pub(super) fn server(&self, cx: &Context<Self>) -> Option<Arc<patchwork_server::Server>> {
        Some(Arc::clone(&self.workspace.upgrade()?.read(cx).server))
    }

    fn has_changes(&self, cx: &Context<Self>) -> bool {
        self.workspace
            .upgrade()
            .is_some_and(|w| !w.read(cx).changes.is_empty())
    }

    /// Reads git's state again for `project`: the graph always, and where the branch stands
    /// (which asks `gh` over the network) when `sync` is set or the project changed.
    pub fn refresh(&mut self, project: Option<String>, sync: bool, cx: &mut Context<Self>) {
        let switched = project != self.project_id;
        if switched {
            self.pages = 1;
            self.sync = None;
            self.commits.clear();
            self.error = None;
            self.result = None;
            let draft = project
                .as_ref()
                .and_then(|id| self.drafts.get(id).cloned())
                .unwrap_or_default();
            self.message.update(cx, |m, cx| m.set_text(&draft, cx));
            self.project_id = project;
        }
        if self.project_id.is_none() {
            return cx.notify();
        }
        let sync = sync || switched;
        // The workspace calls this while it is being updated, so the loads, which read it, wait
        // for that to end.
        cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |this, cx| {
                if sync {
                    this.load_sync(cx);
                }
                this.load_log(cx);
                this.load_missing(cx);
            });
        })
        .detach();
    }

    fn load_sync(&mut self, cx: &mut Context<Self>) {
        let (Some(server), Some(id)) = (self.server(cx), self.project_id.clone()) else {
            return;
        };
        let task = cx.background_spawn(async move {
            let root = server.project_root(&id)?;
            Ok::<_, patchwork_server::Error>((
                id,
                patchwork_server::workspace::git_sync_state(&root),
            ))
        });
        cx.spawn(async move |this, cx| {
            let read = task.await;
            this.update(cx, |this, cx| {
                match read {
                    Ok((id, state)) if Some(&id) == this.project_id.as_ref() => {
                        let branch = state.branch.clone();
                        this.sync = Some(state);
                        this.message.update(cx, |m, cx| {
                            m.placeholder = match branch {
                                Some(b) => format!("Message (Ctrl+Enter to commit on \"{b}\")"),
                                None => "Message (Ctrl+Enter to commit)".into(),
                            }
                            .into();
                            cx.notify();
                        });
                    }
                    Ok(_) => {}
                    Err(_) => this.sync = None,
                }
                cx.notify();
            })
        })
        .detach();
    }

    fn load_log(&mut self, cx: &mut Context<Self>) {
        let (Some(server), Some(id)) = (self.server(cx), self.project_id.clone()) else {
            return;
        };
        let limit = PAGE * self.pages;
        let task = cx.background_spawn(async move {
            let log = server.git_log(&id, 0, limit);
            (id, log)
        });
        cx.spawn(async move |this, cx| {
            let (id, log) = task.await;
            this.update(cx, |this, cx| {
                if Some(&id) != this.project_id.as_ref() {
                    return;
                }
                match log {
                    Ok(list) => {
                        this.more = list.len() == limit;
                        this.commits = list;
                        this.log_error = None;
                    }
                    Err(e) => this.log_error = Some(e.to_string()),
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Which of the focused reply's files are gone from the project (`useMissingFiles`).
    fn load_missing(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let w = workspace.read(cx);
        let files = reply_files(w);
        let Some(root) = w.project().map(|p| p.path.clone()) else {
            return;
        };
        if files.is_empty() {
            self.missing.clear();
            return;
        }
        let task = cx.background_spawn(async move {
            patchwork_server::workspace::missing_files(&root, &files)
        });
        cx.spawn(async move |this, cx| {
            let missing = task.await;
            this.update(cx, |this, cx| {
                this.missing = missing.into_iter().collect();
                cx.notify();
            })
        })
        .detach();
    }

    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        self.pages += 1;
        self.load_log(cx);
    }

    /// Reads the changes and the branch again, as the header's Refresh does.
    pub(super) fn refresh_all(&mut self, cx: &mut Context<Self>) {
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, crate::workspace::Workspace::load_git);
        }
        self.load_sync(cx);
    }

    pub(super) fn set_result(
        &mut self,
        result: Option<(String, Option<PullRequest>)>,
        cx: &mut Context<Self>,
    ) {
        self.result = result;
        self.result_seq += 1;
        let seq = self.result_seq;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RESULT_FOR).await;
            this.update(cx, |this, cx| {
                if this.result_seq == seq {
                    this.result = None;
                    cx.notify();
                }
            })
        })
        .detach();
    }
}
