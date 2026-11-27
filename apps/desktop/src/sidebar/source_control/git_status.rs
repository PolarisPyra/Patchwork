//! What Source Control holds, and reading it: the changed files, the branch and its history.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui::{Entity, Pixels, Point, ScrollHandle, WeakEntity, Window, prelude::*};
use patchwork_backend::worktree::BranchCompare;
use patchwork_backend::source_control::{
    CommitDetail, CommitOptions, GitCommit, GitSyncState, Operation, PullRequest,
};

use patchwork_ui_editor::{Changed, Submitted, TextArea};
use patchwork_ui_components::menu::Menu;
use crate::workspace::Workspace;
use super::header_menu::Asking;
use super::file_groups::reply_files;

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
    Pull,
    Push,
    OpenPr,
    ViewPr,
    Sync,
    /// Goes on with the rebase, merge, cherry-pick or revert that stopped, or gives it up.
    Continue,
    Abort,
}

impl GitAction {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Commit => "Commit",
            Self::CommitPush => "Commit & Push",
            Self::CommitPushPr => "Commit, Push & Open Pull Request",
            Self::Pull => "Pull",
            Self::Push => "Push",
            Self::OpenPr => "Open Pull Request",
            Self::ViewPr => "View Pull Request",
            Self::Sync => "Sync",
            Self::Continue => "Continue",
            Self::Abort => "Abort",
        }
    }
}

/// A stopped operation's name as a button says it.
pub(super) fn operation_title(operation: Operation) -> &'static str {
    match operation {
        Operation::Rebase => "Rebase",
        Operation::Merge => "Merge",
        Operation::CherryPick => "Cherry-Pick",
        Operation::Revert => "Revert",
    }
}

/// The big button's action and label (None leaves it disabled), and the menu's actions.
pub(super) fn git_actions(
    state: Option<&GitSyncState>,
    has_changes: bool,
) -> ((Option<GitAction>, String), Vec<GitAction>) {
    let commit = || "Commit".to_string();
    // A stopped operation is finished or given up before anything else; a rebase has no branch
    // checked out meanwhile.
    if let Some(operation) = state.and_then(|s| s.operation) {
        let mut menu = Vec::new();
        if has_changes {
            menu.push(GitAction::Commit);
        }
        menu.push(GitAction::Abort);
        let label = format!("Continue {}", operation_title(operation));
        return ((Some(GitAction::Continue), label), menu);
    }
    let Some(state) = state.filter(|s| s.branch.is_some()) else {
        return (
            (has_changes.then_some(GitAction::Commit), commit()),
            Vec::new(),
        );
    };
    let on_default = state.branch == state.default_branch;
    let pr = state.pull_request.as_ref();
    // A merged branch has nothing to open a pull request for, or to publish again.
    let can_pr = state.has_remote
        && !on_default
        && !state.merged
        && state.default_branch.is_some()
        && pr.is_none();
    let can_push = state.has_remote
        && if state.has_upstream {
            state.ahead > 0
        } else {
            !state.merged
        };
    // Git pulls onto uncommitted changes unless the pull would overwrite one of them.
    let can_pull = state.has_upstream && state.behind > 0;
    let can_sync = can_pull && state.ahead > 0;
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
    if can_pull {
        menu.push(GitAction::Pull);
    }
    if can_push {
        menu.push(GitAction::Push);
    }
    if can_sync {
        menu.push(GitAction::Sync);
    }
    if can_pr && state.has_upstream && !has_changes {
        menu.push(GitAction::OpenPr);
    }
    if pr.is_some() {
        menu.push(GitAction::ViewPr);
    }
    let primary = if has_changes {
        (Some(GitAction::Commit), commit())
    } else if can_sync {
        let label = format!("Sync ↓{} ↑{}", state.behind, state.ahead);
        (Some(GitAction::Sync), label)
    } else if can_pull {
        // A push would be refused until the upstream's commits are in.
        (Some(GitAction::Pull), format!("Pull ↓{}", state.behind))
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
    /// The folder shown: the open task's worktree, or the project's folder.
    pub(super) root: Option<String>,
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
    /// Unsent commit messages by folder.
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
    /// The changed files, as the list that draws the ones in view, the entries it draws, and
    /// what their heights last depended on.
    pub(super) list: gpui::ListState,
    pub(super) entries: std::rc::Rc<Vec<super::file_list::Entry>>,
    pub(super) list_shape: u64,
    /// What the entries are drawn with: whether the chat may act unasked, so rows need no
    /// buttons, and whether there is anything to commit.
    pub(super) full_access: bool,
    pub(super) has_changes: bool,
    pub(super) graph_scroll: ScrollHandle,
    /// The name or message a repository action waits for.
    pub(super) asking: Option<Asking>,
    /// The message git prepared that was last put in the box, so it is put there once.
    prefilled: Option<String>,
    /// The commit whose message and files show under its row, once they are read.
    pub(super) detail: Option<(String, Option<Result<CommitDetail, String>>)>,
    /// The open task's worktree against the branch it goes back into, and whether its files
    /// show.
    pub(super) compare: Option<BranchCompare>,
    pub(super) compare_open: bool,
}

impl ChangesView {
    pub fn new(
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let message = cx.new(|cx| {
            let mut area = TextArea::new("Message (Ctrl+Enter to commit)", "Commit message", cx);
            area.enter_newline = true;
            area
        });
        cx.subscribe_in(&message, window, |this, _, _: &Submitted, _, cx| {
            if this.has_changes(cx) && this.busy.is_none() {
                this.run(GitAction::Commit, None, CommitOptions::default(), cx);
            }
        })
        .detach();
        cx.subscribe(&message, |this, area, _: &Changed, cx| {
            if let Some(id) = &this.root {
                this.drafts
                    .insert(id.clone(), area.read(cx).content.clone());
            }
            cx.notify();
        })
        .detach();
        Self {
            workspace,
            project_id: None,
            root: None,
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
            list: gpui::ListState::new(0, gpui::ListAlignment::Top, gpui::px(200.)),
            entries: std::rc::Rc::new(Vec::new()),
            list_shape: 0,
            full_access: false,
            has_changes: false,
            graph_scroll: ScrollHandle::new(),
            asking: None,
            prefilled: None,
            detail: None,
            compare: None,
            compare_open: false,
        }
    }

    pub(super) fn core(&self, cx: &Context<Self>) -> Option<Arc<patchwork_backend::Core>> {
        Some(Arc::clone(&self.workspace.upgrade()?.read(cx).core))
    }

    fn has_changes(&self, cx: &Context<Self>) -> bool {
        self.workspace
            .upgrade()
            .is_some_and(|w| !w.read(cx).changes.is_empty())
    }

    /// Reads git's state again for `project`, shown at `root`: the graph always, and where the
    /// branch stands (which asks `gh` over the network) when `sync` is set or the folder changed.
    pub fn refresh(
        &mut self,
        project: Option<String>,
        root: Option<String>,
        sync: bool,
        cx: &mut Context<Self>,
    ) {
        let switched = root != self.root;
        if switched {
            self.pages = 1;
            self.sync = None;
            self.commits.clear();
            self.more = false;
            self.log_error = None;
            self.error = None;
            self.result = None;
            self.busy = None;
            self.pr_error = None;
            self.missing.clear();
            self.asking = None;
            self.prefilled = None;
            self.detail = None;
            self.compare = None;
            let draft = root
                .as_ref()
                .and_then(|r| self.drafts.get(r).cloned())
                .unwrap_or_default();
            self.message.update(cx, |m, cx| m.set_text(&draft, cx));
            self.root = root;
        }
        self.project_id = project;
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
                this.load_compare(cx);
                this.load_missing(cx);
            });
        })
        .detach();
    }

    pub(super) fn load_sync(&mut self, cx: &mut Context<Self>) {
        // A folder just opened shows where its branch stands before the remote is asked.
        self.read_sync(self.sync.is_some(), cx);
    }

    /// Reads where the branch stands, after fetching its remote when `fetch` is set.
    fn read_sync(&mut self, fetch: bool, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let task = cx.background_spawn(async move {
            if fetch {
                patchwork_backend::source_control::fetch_remote(&root);
            }
            let state = patchwork_backend::source_control::git_sync_state(&root);
            (root, state)
        });
        cx.spawn(async move |this, cx| {
            let (root, state) = task.await;
            this.update(cx, |this, cx| {
                if Some(&root) != this.root.as_ref() {
                    return;
                }
                let branch = state.branch.clone();
                // A merge or a revert leaves its message ready; it is offered once.
                let prepared = state.prepared_message.clone();
                if prepared != this.prefilled {
                    if let Some(text) = prepared.as_deref()
                        && this.message.read(cx).content.trim().is_empty()
                    {
                        this.message.update(cx, |m, cx| m.set_text(text, cx));
                    }
                    this.prefilled = prepared;
                }
                this.sync = Some(state);
                this.message.update(cx, |m, cx| {
                    m.placeholder = match branch {
                        Some(b) => format!("Message (Ctrl+Enter to commit on \"{b}\")"),
                        None => "Message (Ctrl+Enter to commit)".into(),
                    }
                    .into();
                    cx.notify();
                });
                if !fetch {
                    this.read_sync(true, cx);
                }
                cx.notify();
            })
        })
        .detach();
    }

    pub(super) fn load_log(&mut self, cx: &mut Context<Self>) {
        let (Some(core), Some(id), Some(root)) =
            (self.core(cx), self.project_id.clone(), self.root.clone())
        else {
            return;
        };
        let limit = PAGE * self.pages;
        let chat = self.chat_id(cx);
        let task = cx.background_spawn(async move {
            let log = core.git_log(&id, chat.as_deref(), 0, limit);
            (root, log)
        });
        cx.spawn(async move |this, cx| {
            let (root, log) = task.await;
            this.update(cx, |this, cx| {
                // Another folder, or a longer page, was asked for since.
                if Some(&root) != this.root.as_ref() || limit != PAGE * this.pages {
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

    /// Reads what the open task's worktree changed since its branch left the project's branch,
    /// when Source Control shows that worktree.
    fn load_compare(&mut self, cx: &mut Context<Self>) {
        let (Some(core), Some(root)) = (self.core(cx), self.root.clone()) else {
            return;
        };
        let Some(chat) = self.chat_id(cx) else {
            self.compare = None;
            return;
        };
        patchwork_ui_theme::off_thread(
            cx,
            move || core.branch_compare(&chat),
            move |this, compare, cx| {
                // Another folder was opened since.
                if this.root.as_ref() != Some(&root) {
                    return;
                }
                this.compare = match compare {
                    Ok(compare) => compare.filter(|c| c.root == root),
                    Err(e) => {
                        log::warn!(error:% = e; "could not compare the task's branch: {{error}}");
                        None
                    }
                };
                cx.notify();
            },
        );
    }

    /// Which of the focused reply's files are gone from the folder its task works in
    /// (`useMissingFiles`).
    fn load_missing(&mut self, cx: &mut Context<Self>) {
        let (Some(workspace), Some(core)) = (self.workspace.upgrade(), self.core(cx)) else {
            return;
        };
        let w = workspace.read(cx);
        let files = reply_files(w);
        let Some((chat, _)) = w.change_focus.clone() else {
            self.missing.clear();
            return;
        };
        if files.is_empty() {
            self.missing.clear();
            return;
        }
        let focus = w.change_focus.clone();
        patchwork_ui_theme::off_thread(
            cx,
            move || {
                let root = core.chat_root(&chat)?;
                Ok::<_, patchwork_backend::Error>(patchwork_backend::projects::missing_files(
                    &root, &files,
                ))
            },
            move |this, missing, cx| {
                // Another reply may have been focused since.
                let current = this
                    .workspace
                    .upgrade()
                    .and_then(|w| w.read(cx).change_focus.clone());
                if current != focus {
                    return;
                }
                match missing {
                    Ok(missing) => this.missing = missing.into_iter().collect(),
                    Err(e) => {
                        log::warn!(error:% = e; "could not tell which files are gone: {{error}}");
                        this.missing.clear();
                    }
                }
                cx.notify();
            },
        );
    }

    pub fn load_more(&mut self, cx: &mut Context<Self>) {
        self.pages += 1;
        self.load_log(cx);
    }

    /// Reads the changes and the branch again, as the header's Refresh does.
    pub(super) fn refresh_all(&mut self, cx: &mut Context<Self>) {
        let _ = self
            .workspace
            .update(cx, crate::workspace::Workspace::load_git);
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
        patchwork_ui_theme::after(cx, RESULT_FOR, move |this, cx| {
            if this.result_seq == seq {
                this.result = None;
                cx.notify();
            }
        });
    }
}
