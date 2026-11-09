//! Source Control (`components/ChangesView.tsx`): the commit box, the changed files, and the
//! commit graph, each pane closing on its header and the divider between them dragged.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, ClickEvent, Context, Div, Entity, Focusable, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, Pixels, Point, PromptLevel, Role, ScrollHandle, SharedString, Stateful,
    WeakEntity, Window, div, prelude::*, px,
};
use patchwork_server::threads::AccessMode;
use patchwork_server::workspace::{FileChange, GitCommit, GitSyncState, PullRequest};

use super::{graph};
use crate::chat::text_area::{Changed, Submitted, TextArea};
use crate::ui::menu::{Dismissed, Menu, MenuItem};
use crate::ui::theme::{RADIUS_SM, Theme, theme};
use crate::ui::{base_name, button, dir_name, icon, scrollbar, spinner, tooltip};
use crate::workspace::Workspace;

/// The sidebar's type scale.
const FS_XS: f32 = 12.;
const FS_SM: f32 = 13.;

/// The Graph's height while Changes is open too, and the least either pane is dragged to.
const GRAPH_HEIGHT: f32 = 260.;
const PANE_MIN: f32 = 88.;
/// How long a finished action's note stays.
const RESULT_FOR: Duration = Duration::from_secs(10);
/// Commits read per page.
const PAGE: usize = 50;

/// What the Commit button and its menu offer (`lib/gitActions.ts`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum GitAction {
    Commit,
    CommitPush,
    CommitPushPr,
    Push,
    OpenPr,
    ViewPr,
}

impl GitAction {
    fn label(self) -> &'static str {
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
fn git_actions(
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

pub struct ChangesView {
    workspace: WeakEntity<Workspace>,
    project_id: Option<String>,
    sync: Option<GitSyncState>,
    pub commits: Vec<GitCommit>,
    pages: usize,
    pub more: bool,
    pub log_error: Option<String>,
    changes_open: bool,
    graph_open: bool,
    graph_height: f32,
    /// Where a divider drag started: the pointer's y and the Graph's height then.
    resizing: Option<(f32, f32)>,
    message: Entity<TextArea>,
    /// Unsent commit messages by project.
    drafts: HashMap<String, String>,
    busy: Option<String>,
    result: Option<(String, Option<PullRequest>)>,
    /// Counts results, so an old one's timer leaves a newer one alone.
    result_seq: usize,
    error: Option<String>,
    resolving: bool,
    pr_busy: bool,
    pr_error: Option<String>,
    pub undo_error: Option<String>,
    /// The reply files that no longer exist.
    missing: HashSet<String>,
    /// The commit actions menu, or a commit's undo menu with its commit, and where it opened.
    menu: Option<(Entity<Menu>, Point<Pixels>, Option<String>)>,
    dismissed: Option<Point<Pixels>>,
    scroll: ScrollHandle,
    graph_scroll: ScrollHandle,
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

    fn server(&self, cx: &Context<Self>) -> Option<Arc<patchwork_server::Server>> {
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
            self.project_id = project.clone();
        }
        if project.is_none() {
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
            Ok::<_, String>((id, patchwork_server::workspace::git_sync_state(&root)))
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
                    Err(e) => this.log_error = Some(e),
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
    fn refresh_all(&mut self, cx: &mut Context<Self>) {
        if let Some(w) = self.workspace.upgrade() {
            w.update(cx, |w, cx| w.load_git(cx));
        }
        self.load_sync(cx);
    }

    fn set_result(
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

    /// Runs one of the Commit button's actions, step by step, off the main thread.
    fn run(&mut self, action: GitAction, cx: &mut Context<Self>) {
        self.error = None;
        self.result = None;
        if action == GitAction::ViewPr {
            if let Some(pr) = self.sync.as_ref().and_then(|s| s.pull_request.as_ref()) {
                cx.open_url(&pr.url);
            }
            return;
        }
        let (Some(server), Some(id)) = (self.server(cx), self.project_id.clone()) else {
            return;
        };
        let thread = self.thread_id(cx);
        let message = self.message.read(cx).content.clone();
        let commit = matches!(
            action,
            GitAction::Commit | GitAction::CommitPush | GitAction::CommitPushPr
        );
        let push = matches!(
            action,
            GitAction::CommitPush | GitAction::CommitPushPr | GitAction::Push
        );
        let pr = matches!(action, GitAction::CommitPushPr | GitAction::OpenPr);
        self.busy = Some(
            if !commit {
                if push {
                    "Pushing…"
                } else {
                    "Opening pull request…"
                }
            } else if message.trim().is_empty() {
                "Writing message…"
            } else {
                "Committing…"
            }
            .into(),
        );
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut done: Option<(String, Option<PullRequest>)> = None;
            let mut failed = None;
            if commit {
                let (server, id, thread) = (Arc::clone(&server), id.clone(), thread.clone());
                match cx
                    .background_spawn(async move {
                        server.commit_changes(&id, thread.as_deref(), &message)
                    })
                    .await
                {
                    Ok((hash, subject)) => {
                        done = Some((format!("Committed {hash}: {subject}"), None));
                        let _ = this.update(cx, |this, cx| {
                            this.message.update(cx, |m, cx| m.set_text("", cx));
                            if let Some(id) = &this.project_id {
                                this.drafts.remove(id);
                            }
                        });
                    }
                    Err(e) => failed = Some(e),
                }
            }
            if failed.is_none() && push {
                let _ = this.update(cx, |this, cx| {
                    this.busy = Some("Pushing…".into());
                    cx.notify();
                });
                let (server, id) = (Arc::clone(&server), id.clone());
                match cx
                    .background_spawn(async move {
                        let root = server.project_root(&id)?;
                        patchwork_server::workspace::push(&root).map_err(String::from)
                    })
                    .await
                {
                    Ok(to) => done = Some((format!("Pushed to {to}"), None)),
                    Err(e) => failed = Some(e),
                }
            }
            if failed.is_none() && pr {
                let _ = this.update(cx, |this, cx| {
                    this.busy = Some("Opening pull request…".into());
                    cx.notify();
                });
                let (server, id) = (Arc::clone(&server), id.clone());
                match cx
                    .background_spawn(
                        async move { server.open_pull_request(&id, thread.as_deref()) },
                    )
                    .await
                {
                    Ok(pr) => done = Some((format!("Pull request #{}", pr.number), Some(pr))),
                    Err(e) => failed = Some(e),
                }
            }
            let _ = this.update(cx, |this, cx| {
                // What finished before a failure still shows, above the error.
                this.set_result(done, cx);
                this.error = failed;
                this.busy = None;
                this.refresh_all(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The open chat or task, whose model writes a message when no title model is set.
    fn thread_id(&self, cx: &Context<Self>) -> Option<String> {
        self.workspace
            .upgrade()?
            .read(cx)
            .thread
            .as_ref()
            .map(|t| t.id.clone())
    }

    fn write_message(&mut self, cx: &mut Context<Self>) {
        let (Some(server), Some(id)) = (self.server(cx), self.project_id.clone()) else {
            return;
        };
        let thread = self.thread_id(cx);
        self.error = None;
        self.busy = Some("Writing message…".into());
        cx.notify();
        let task =
            cx.background_spawn(async move { server.write_commit_message(&id, thread.as_deref()) });
        cx.spawn(async move |this, cx| {
            let written = task.await;
            this.update(cx, |this, cx| {
                match written {
                    Ok(text) => {
                        if let Some(id) = &this.project_id {
                            this.drafts.insert(id.clone(), text.clone());
                        }
                        this.message.update(cx, |m, cx| m.set_text(&text, cx));
                    }
                    Err(e) => this.error = Some(e),
                }
                this.busy = None;
                this.refresh_all(cx);
                cx.notify();
            })
        })
        .detach();
    }

    /// Runs `step` on each file in turn, off the main thread, then reads the changes again.
    fn each_file(
        &mut self,
        files: Vec<String>,
        step: fn(&str, &str) -> Result<(), patchwork_server::error::Error>,
        cx: &mut Context<Self>,
    ) {
        let (Some(server), Some(id)) = (self.server(cx), self.project_id.clone()) else {
            return;
        };
        self.error = None;
        let task = cx.background_spawn(async move {
            let root = server.project_root(&id)?;
            for file in &files {
                step(&root, file).map_err(String::from)?;
            }
            Ok::<_, String>(())
        });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            this.update(cx, |this, cx| {
                this.error = done.err();
                if let Some(w) = this.workspace.upgrade() {
                    w.update(cx, |w, cx| w.load_git(cx));
                }
                cx.notify();
            })
        })
        .detach();
    }

    fn stage(&mut self, files: Vec<String>, cx: &mut Context<Self>) {
        self.each_file(files, patchwork_server::workspace::accept_file, cx);
    }

    /// Reverts the files after one confirmation.
    fn discard(&mut self, files: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        if files.is_empty() {
            return;
        }
        let what = match files.as_slice() {
            [one] => base_name(one).to_string(),
            many => format!("{} files", many.len()),
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            "Discard",
            Some(&format!("Discard changes to {what}?")),
            &["Discard", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.each_file(files, patchwork_server::workspace::revert_file, cx)
            });
        })
        .detach();
    }

    /// Starts a task whose agent resolves the conflicts, given both sides of each.
    fn resolve_with_agent(&mut self, cx: &mut Context<Self>) {
        let (Some(server), Some(id)) = (self.server(cx), self.project_id.clone()) else {
            return;
        };
        self.error = None;
        self.resolving = true;
        cx.notify();
        let task = cx.background_spawn(async move { server.conflict_prompt(&id) });
        cx.spawn(async move |this, cx| {
            let prompt = task.await;
            this.update(cx, |this, cx| {
                this.resolving = false;
                match prompt {
                    Ok(text) => {
                        if let Some(w) = this.workspace.upgrade() {
                            w.update(cx, |w, cx| {
                                w.start_task("Resolve merge conflicts", text, cx)
                            });
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Sends the pull request's failures and comments to the thread that made the branch's newest
    /// commit, or starts a task with them when none did.
    fn send_feedback(&mut self, number: u32, cx: &mut Context<Self>) {
        let (Some(server), Some(id)) = (self.server(cx), self.project_id.clone()) else {
            return;
        };
        self.pr_error = None;
        self.pr_busy = true;
        cx.notify();
        let task = cx.background_spawn(async move { server.pull_request_feedback(&id) });
        cx.spawn(async move |this, cx| {
            let feedback = task.await;
            this.update(cx, |this, cx| {
                this.pr_busy = false;
                match feedback {
                    Ok((thread, prompt)) => {
                        if let Some(w) = this.workspace.upgrade() {
                            w.update(cx, |w, cx| {
                                let live = thread.filter(|id| {
                                    w.threads
                                        .iter()
                                        .any(|t| &t.id == id && t.archived_at.is_none())
                                });
                                match live {
                                    Some(id) => w.send_to(&id, prompt, cx),
                                    None => w.start_task(
                                        &format!("Pull request #{number} feedback"),
                                        prompt,
                                        cx,
                                    ),
                                }
                            });
                        }
                    }
                    Err(e) => this.pr_error = Some(e),
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Takes a reply's changes back out of the files after asking; the commit stays.
    pub fn undo_reply(
        &mut self,
        thread_id: String,
        message_id: String,
        prompt: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.undo_error = None;
        let what = if prompt.is_empty() {
            "this reply".to_string()
        } else {
            prompt
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            "Undo reply",
            Some(&format!(
                "Take the changes of “{what}” back out of your files? The commit stays, and the undo shows as uncommitted changes."
            )),
            &["Undo reply", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let Ok(Some(server)) = this.update(cx, |this, cx| this.server(cx)) else {
                return;
            };
            let undone = cx
                .background_spawn(async move { server.undo_reply(&thread_id, &message_id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.undo_error = undone.err();
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens a menu at `at`; `commit` names the commit an undo menu is for.
    pub fn open_menu(
        &mut self,
        label: &str,
        items: Vec<MenuItem>,
        at: Point<Pixels>,
        commit: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let menu = Menu::open(label.to_string(), items, window, cx);
        cx.subscribe_in(&menu, window, |this, _, e: &Dismissed, _, cx| {
            this.dismissed = e.0;
            this.menu = None;
            cx.notify();
        })
        .detach();
        self.menu = Some((menu, at, commit));
        cx.notify();
    }

    /// Whether a click on a trigger is the same press that just closed its menu.
    pub fn just_dismissed(&mut self, e: &ClickEvent) -> bool {
        matches!(e, ClickEvent::Mouse(click) if self.dismissed.take() == Some(click.down.position))
    }

    pub fn workspace(&self) -> WeakEntity<Workspace> {
        self.workspace.clone()
    }

    pub fn menu_for(&self, commit: &str) -> bool {
        self.menu
            .as_ref()
            .is_some_and(|(_, _, c)| c.as_deref() == Some(commit))
    }

    fn set_graph_height(&mut self, height: f32, total: f32) {
        self.graph_height = height.min(total - PANE_MIN).max(PANE_MIN).round();
    }
}

/// The focused reply's files, listed whether or not they still have uncommitted changes.
fn reply_files(w: &Workspace) -> Vec<String> {
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
fn row_action(
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
        .group_hover(group, |s| s.visible())
        .child(icon(name, 14.))
}

/// A small text button beside a group's name: "Resolve with agent", "Project".
fn group_button(
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
fn group(
    title: &'static str,
    count: usize,
    actions: Vec<AnyElement>,
    rows: Vec<AnyElement>,
    t: &Theme,
) -> Div {
    let name = SharedString::from(format!("group:{title}"));
    div()
        .flex()
        .flex_col()
        .px(px(4.))
        .child(
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
        .children(rows)
}

impl ChangesView {
    /// One file (`ChangeRow`): its name and folder, the actions shown on hover, its line counts,
    /// and its status letter. Opens its diff.
    fn change_row(
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
                        this.discard(vec![discard.clone()], window, cx)
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
                            this.stage(vec![stage.clone()], cx)
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
                        .when(deleted, |d| d.line_through())
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

    /// The commit message box and the Commit button with its menu (`CommitBox`).
    fn commit_box(
        &self,
        has_changes: bool,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let ((primary, primary_label), menu) = git_actions(self.sync.as_ref(), has_changes);
        let busy = self.busy.is_some();
        let lead_icon = if primary_label == "Publish Branch" {
            "cloud-arrow-up"
        } else {
            match primary {
                Some(GitAction::Push) => "arrow-up",
                Some(GitAction::OpenPr | GitAction::ViewPr) => "git-pull-request",
                _ => "check",
            }
        };
        let focused = self.message.read(cx).focus_handle(cx).is_focused(window);
        let lines = self.message.read(cx).content.lines().count().clamp(3, 10) as f32;
        let (hover, fg, accent_hover) = (t.bg_hover, t.text, t.accent_hover);
        let primary_enabled = primary.is_some() && !busy;
        let menu_enabled = !menu.is_empty() && !busy;
        let menu_open = self.menu.as_ref().is_some_and(|(_, _, c)| c.is_none());
        let pr_number = self
            .sync
            .as_ref()
            .and_then(|s| s.pull_request.as_ref())
            .map(|p| p.number);
        let write_enabled = has_changes && !busy;
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(8.))
            .pt(px(4.))
            .pb(px(8.))
            .child(
                div()
                    .relative()
                    .flex()
                    .rounded(px(RADIUS_SM))
                    .border_1()
                    .border_color(if focused { t.accent } else { t.border })
                    .bg(t.bg_input)
                    .child(
                        div()
                            .id("commit-message")
                            .flex_1()
                            .min_w_0()
                            .min_h(px(18. * 3. + 8.))
                            .max_h(px(18. * 10. + 8.))
                            .h(px(18. * lines + 8.))
                            .overflow_y_scroll()
                            .py(px(4.))
                            .pl(px(8.))
                            .pr(px(28.))
                            .text_size(px(FS_SM))
                            .line_height(px(18.))
                            .child(self.message.clone()),
                    )
                    .child(
                        button("write-message", "Write commit message")
                            .absolute()
                            .top(px(2.))
                            .right(px(2.))
                            .size(px(24.))
                            .justify_center()
                            .rounded(px(RADIUS_SM))
                            .text_color(t.text_muted)
                            .tooltip(tooltip("Write commit message"))
                            .when(write_enabled, |b| {
                                b.hover(move |s| s.bg(hover).text_color(fg))
                            })
                            .when(!write_enabled, |b| b.opacity(0.5).cursor_default())
                            .child(icon("sparkle", 14.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if write_enabled {
                                    this.write_message(cx)
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .h(px(28.))
                    .flex()
                    .child(
                        button("commit", self.busy.clone().unwrap_or(primary_label.clone()))
                            .flex_1()
                            .min_w_0()
                            .justify_center()
                            .gap(px(6.))
                            .rounded_l(px(RADIUS_SM))
                            .bg(t.accent)
                            .px(px(8.))
                            .text_size(px(FS_SM))
                            .text_color(t.text_on_accent)
                            .when(primary_enabled, |b| b.hover(move |s| s.bg(accent_hover)))
                            .when(!primary_enabled, |b| b.opacity(0.5).cursor_default())
                            .child(if busy {
                                spinner(12., t.text_on_accent).into_any_element()
                            } else {
                                icon(lead_icon, 14.).into_any_element()
                            })
                            .child(
                                div()
                                    .truncate()
                                    .child(self.busy.clone().unwrap_or(primary_label)),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(action) = primary.filter(|_| primary_enabled) {
                                    this.run(action, cx)
                                }
                            })),
                    )
                    .child(
                        button("commit-actions", "More actions")
                            .h(px(28.))
                            .w(px(24.))
                            .justify_center()
                            .rounded_r(px(RADIUS_SM))
                            .border_l_1()
                            .border_color(gpui::black().opacity(0.2))
                            .bg(t.accent)
                            .text_color(t.text_on_accent)
                            .aria_expanded(menu_open)
                            .tooltip(tooltip("More actions"))
                            .when(menu_enabled, |b| b.hover(move |s| s.bg(accent_hover)))
                            .when(!menu_enabled, |b| b.opacity(0.5).cursor_default())
                            .child(icon("caret-down", 12.))
                            .on_click(cx.listener(move |this, e: &ClickEvent, window, cx| {
                                if !menu_enabled || this.just_dismissed(e) {
                                    return;
                                }
                                let entity = cx.entity().downgrade();
                                let items = menu
                                    .iter()
                                    .map(|&action| {
                                        let label = match (action, pr_number) {
                                            (GitAction::ViewPr, Some(n)) => {
                                                format!("{} #{n}", action.label())
                                            }
                                            _ => action.label().to_string(),
                                        };
                                        let entity = entity.clone();
                                        MenuItem::new(label, move |_, cx| {
                                            let _ =
                                                entity.update(cx, |this, cx| this.run(action, cx));
                                        })
                                    })
                                    .collect();
                                let at = e.position() + gpui::point(px(-160.), px(12.));
                                this.open_menu("Commit actions", items, at, None, window, cx);
                            })),
                    ),
            )
            .when_some(self.result.clone(), |d, (text, pr)| {
                d.child(
                    div()
                        .id("commit-result")
                        .role(Role::Status)
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(2.))
                        .text_size(px(FS_XS))
                        .text_color(t.text_muted)
                        .child(div().min_w_0().truncate().child(text))
                        .when_some(pr, |d, pr| {
                            d.child(
                                button("open-pr", "Open")
                                    .flex_none()
                                    .text_color(t.accent_text)
                                    .hover(|s| s.underline())
                                    .child("Open")
                                    .on_click(move |_, _, cx| cx.open_url(&pr.url)),
                            )
                        }),
                )
            })
            .when_some(self.error.clone(), |d, e| d.child(alert(e, FS_XS, t)))
    }

    /// The branch's open pull request: its checks, its comments, and sending them to the agent.
    fn pull_request_bar(&self, pr: &PullRequest, t: &Theme, cx: &mut Context<Self>) -> Div {
        let checks = if pr.failed > 0 {
            Some(format!("{} failed", pr.failed))
        } else if pr.pending > 0 {
            Some(format!("{} running", pr.pending))
        } else if pr.passed > 0 {
            Some("Checks passed".to_string())
        } else {
            None
        };
        let comments = (pr.comments > 0).then(|| {
            format!(
                "{} {}",
                pr.comments,
                if pr.comments == 1 {
                    "comment"
                } else {
                    "comments"
                }
            )
        });
        let url = pr.url.clone();
        let number = pr.number;
        let failed = pr.failed > 0;
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .px(px(8.))
            .pb(px(8.))
            .child(
                div()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(FS_XS))
                    .text_color(t.text_muted)
                    .child(
                        button("pr-link", format!("Pull request #{number}"))
                            .flex_none()
                            .gap(px(4.))
                            .text_color(t.text)
                            .hover(|s| s.underline())
                            .tooltip(tooltip(url.clone()))
                            .child(icon("git-pull-request", 14.))
                            .child(format!("#{number}"))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    )
                    .child(
                        div()
                            .id("pr-status")
                            .role(Role::Status)
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .when_some(checks.clone(), |d, c| {
                                d.child(div().when(failed, |d| d.text_color(t.danger)).child(c))
                            })
                            .when(checks.is_some() && comments.is_some(), |d| {
                                d.child("\u{a0}·\u{a0}")
                            })
                            .children(comments),
                    )
                    .when(pr.failed > 0 || pr.comments > 0, |d| {
                        let busy = self.pr_busy;
                        d.child(
                            group_button(
                                "send-feedback",
                                "Send to agent",
                                if busy {
                                    spinner(12., t.text_subtle).into_any_element()
                                } else {
                                    icon("paper-plane-right", 14.).into_any_element()
                                },
                                t,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if !this.pr_busy {
                                        this.send_feedback(number, cx)
                                    }
                                },
                            )),
                        )
                    }),
            )
            .when_some(self.pr_error.clone(), |d, e| d.child(alert(e, FS_XS, t)))
    }

    /// A collapsible part of the view: its header, and its body when open.
    #[allow(clippy::too_many_arguments)]
    fn pane(
        &self,
        title: &'static str,
        open: bool,
        height: Option<f32>,
        actions: Option<AnyElement>,
        body: AnyElement,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let caret = if open { "caret-down" } else { "caret-right" };
        div()
            .id(title)
            .role(Role::Region)
            .aria_label(title)
            .flex()
            .flex_col()
            .min_h_0()
            .when(title == "Graph", |d| d.border_t_1().border_color(t.border))
            .map(|d| match (open, height) {
                (true, Some(h)) => d.h(px(h)).flex_none(),
                (true, None) => d.flex_1(),
                _ => d.flex_none(),
            })
            .child(
                div()
                    .h(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pr(px(8.))
                    .child(
                        button(SharedString::from(format!("pane:{title}")), title)
                            .flex_1()
                            .min_w_0()
                            .gap(px(4.))
                            .px(px(4.))
                            .text_size(px(FS_XS))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.text)
                            .aria_expanded(open)
                            .child(icon(caret, 12.))
                            .child(title)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if title == "Graph" {
                                    this.graph_open = !this.graph_open;
                                } else {
                                    this.changes_open = !this.changes_open;
                                }
                                cx.notify();
                            })),
                    )
                    .children(actions.filter(|_| open)),
            )
            .when(open, |d| d.child(body))
    }
}

/// An error under what failed.
fn alert(text: String, size: f32, t: &Theme) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("alert:{text}")))
        .role(Role::Alert)
        .px(px(2.))
        .text_size(px(size))
        .text_color(t.danger)
        .child(text)
}

impl Render for ChangesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let Some(workspace) = self.workspace.upgrade() else {
            return div().into_any_element();
        };
        let w = workspace.read(cx);
        let changes = w.changes.clone();
        let has_project = w.project().is_some();
        let full_access = w
            .thread
            .as_ref()
            .is_some_and(|th| th.access_mode == AccessMode::FullAccess);
        let replies = reply_files(w);
        let live: Vec<_> = w
            .threads
            .iter()
            .filter(|th| th.archived_at.is_none() && th.parent.is_none())
            .cloned()
            .collect();
        let archived: Vec<_> = w
            .threads
            .iter()
            .filter(|th| th.archived_at.is_some() && th.parent.is_none())
            .cloned()
            .collect();
        let has_changes = !changes.is_empty();

        let conflicted: Vec<&FileChange> = changes
            .iter()
            .filter(|c| c.status == "conflicted")
            .collect();
        let pinned: Vec<&FileChange> = changes
            .iter()
            .filter(|c| replies.contains(&c.path) && c.status != "conflicted")
            .collect();
        let rest: Vec<&FileChange> = changes
            .iter()
            .filter(|c| !replies.contains(&c.path) && c.status != "conflicted")
            .collect();
        let staged: Vec<&FileChange> = rest.iter().copied().filter(|c| c.staged).collect();
        let unstaged: Vec<&FileChange> = rest.iter().copied().filter(|c| !c.staged).collect();

        let mut body = div().flex().flex_col().pb(px(4.));
        if has_project {
            body = body.child(self.commit_box(has_changes, &t, window, cx));
            if let Some(pr) = self.sync.as_ref().and_then(|s| s.pull_request.clone()) {
                body = body.child(self.pull_request_bar(&pr, &t, cx));
            }
        }
        if !conflicted.is_empty() {
            let rows = conflicted
                .iter()
                .map(|c| self.change_row(c, full_access, &t, cx))
                .collect();
            let lead = if self.resolving {
                spinner(12., t.text_subtle).into_any_element()
            } else {
                icon("sparkle", 14.).into_any_element()
            };
            let resolve = group_button("resolve", "Resolve with agent", lead, &t)
                .on_click(cx.listener(|this, _, _, cx| {
                    if !this.resolving {
                        this.resolve_with_agent(cx)
                    }
                }))
                .into_any_element();
            body = body.child(group(
                "Merge Changes",
                conflicted.len(),
                vec![resolve],
                rows,
                &t,
            ));
        }
        if !replies.is_empty() {
            let mut rows: Vec<AnyElement> = pinned
                .iter()
                .map(|c| self.change_row(c, full_access, &t, cx))
                .collect();
            let hover = t.bg_hover;
            for path in replies
                .iter()
                .filter(|p| !changes.iter().any(|c| &c.path == *p))
            {
                let missing = self.missing.contains(path);
                let open = path.clone();
                let workspace = self.workspace.clone();
                rows.push(
                    button(
                        SharedString::from(format!("reply-file:{path}")),
                        path.clone(),
                    )
                    .h(px(22.))
                    .gap(px(6.))
                    .rounded(px(RADIUS_SM))
                    .pr(px(6.))
                    .pl(px(20.))
                    .text_size(px(FS_SM))
                    .text_color(t.text_muted)
                    .hover(move |s| s.bg(hover))
                    .tooltip(tooltip(path.clone()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .when(missing, |d| d.line_through())
                            .child(base_name(path).to_string()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(FS_XS))
                            .text_color(t.text_subtle)
                            .child(if missing { "Deleted" } else { "No changes" }),
                    )
                    .on_click(move |_, window, cx| {
                        let open = open.clone();
                        let _ = workspace.update(cx, |w, cx| w.open_change(&open, window, cx));
                    })
                    .into_any_element(),
                );
            }
            let back = group_button(
                "back-to-project",
                "Project",
                icon("arrow-left", 14.).into_any_element(),
                &t,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(w) = this.workspace.upgrade() {
                    w.update(cx, |w, cx| {
                        w.change_focus = None;
                        cx.notify();
                    });
                }
                this.missing.clear();
            }))
            .into_any_element();
            body = body.child(group(
                "From this reply",
                replies.len(),
                vec![back],
                rows,
                &t,
            ));
        }
        if !staged.is_empty() {
            let rows = staged
                .iter()
                .map(|c| self.change_row(c, full_access, &t, cx))
                .collect();
            body = body.child(group("Staged Changes", staged.len(), Vec::new(), rows, &t));
        }
        if !unstaged.is_empty() {
            let rows = unstaged
                .iter()
                .map(|c| self.change_row(c, full_access, &t, cx))
                .collect();
            let paths: Vec<String> = unstaged.iter().map(|c| c.path.clone()).collect();
            let actions = if full_access {
                Vec::new()
            } else {
                let name = SharedString::from("group:Changes");
                let (discard, stage) = (paths.clone(), paths);
                vec![
                    row_action(
                        "discard-all",
                        "Discard all changes",
                        "arrow-counter-clockwise",
                        name.clone(),
                        &t,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.discard(discard.clone(), window, cx)
                    }))
                    .into_any_element(),
                    row_action("stage-all", "Stage all changes", "plus", name, &t)
                        .on_click(cx.listener(move |this, _, _, cx| this.stage(stage.clone(), cx)))
                        .into_any_element(),
                ]
            };
            body = body.child(group("Changes", unstaged.len(), actions, rows, &t));
        }

        let changes_body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("changes-scroll")
                    .track_scroll(&self.scroll)
                    .overflow_y_scroll()
                    .size_full()
                    .child(body),
            )
            .children(scrollbar(&self.scroll, 4., 0., t.border_strong))
            .into_any_element();
        let graph_body = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .id("graph-scroll")
                    .track_scroll(&self.graph_scroll)
                    .overflow_y_scroll()
                    .size_full()
                    .pb(px(4.))
                    .when(has_project, |d| {
                        d.child(graph::section(self, &live, &archived, &t, cx))
                    }),
            )
            .children(scrollbar(&self.graph_scroll, 4., 0., t.border_strong))
            .into_any_element();

        let (fg, hover) = (t.text, t.bg_hover);
        let graph_height = self.changes_open.then_some(self.graph_height);
        let divider = (self.changes_open && self.graph_open).then(|| {
            let height = self.graph_height;
            div()
                .id("graph-resize")
                .role(Role::Splitter)
                .aria_orientation(gpui::Orientation::Horizontal)
                .aria_label("Resize graph")
                .aria_numeric_value(height as f64)
                .aria_min_numeric_value(PANE_MIN as f64)
                .tab_index(0)
                .relative()
                .h(px(4.))
                .mb(px(-4.))
                .flex_none()
                .cursor_row_resize()
                .focus(|s| s.bg(t.accent))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                        this.resizing = Some((f32::from(e.position.y), this.graph_height));
                        cx.stop_propagation();
                    }),
                )
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                    let step = match e.keystroke.key.as_str() {
                        "up" => 16.,
                        "down" => -16.,
                        _ => return,
                    };
                    let total = f32::from(window.viewport_size().height);
                    this.set_graph_height(this.graph_height + step, total);
                    cx.notify();
                }))
        });

        let menu = self.menu.as_ref().map(|(m, at, _)| Menu::view(m, *at));
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .mt(px(6.))
                    .h(px(32.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(12.))
                    .text_size(px(FS_SM))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Source Control")
                    .child(
                        button("refresh-changes", "Refresh")
                            .size(px(28.))
                            .justify_center()
                            .rounded(px(RADIUS_SM))
                            .text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .tooltip(tooltip("Refresh"))
                            .child(icon("arrows-clockwise", 18.).mt(px(1.)))
                            .on_click(cx.listener(|this, _, _, cx| this.refresh_all(cx))),
                    ),
            )
            .child(
                div()
                    .id("panes")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .on_mouse_move(cx.listener(
                        move |this, e: &gpui::MouseMoveEvent, window, cx| {
                            if let Some((start, height)) = this.resizing {
                                let total = f32::from(window.viewport_size().height);
                                this.set_graph_height(
                                    height + start - f32::from(e.position.y),
                                    total,
                                );
                                cx.notify();
                            }
                        },
                    ))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if this.resizing.take().is_some() {
                                cx.notify();
                            }
                        }),
                    )
                    .child(self.pane(
                        "Changes",
                        self.changes_open,
                        None,
                        None,
                        changes_body,
                        &t,
                        cx,
                    ))
                    .children(divider)
                    .child(self.pane(
                        "Graph",
                        self.graph_open,
                        graph_height,
                        None,
                        graph_body,
                        &t,
                        cx,
                    )),
            )
            .children(menu)
            .into_any_element()
    }
}
