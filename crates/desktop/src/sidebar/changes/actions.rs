//! Source Control's actions: committing, pushing, staging, discarding, and handing work to an agent.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{ClickEvent, Pixels, Point, PromptLevel, WeakEntity, Window, prelude::*};
use patchwork_core::workspace::{CommitOptions, PullRequest, RepoAction};

use crate::ui::menu::{Dismissed, Menu, MenuItem};
use crate::ui::base_name;
use crate::workspace::Workspace;
use super::state::{ChangesView, GitAction, PANE_MIN};

impl ChangesView {
    /// Starts one of the Commit button's actions, after asking what it needs first: which remote
    /// a branch is published to, or whether to give an operation up.
    pub(super) fn start(
        &mut self,
        action: GitAction,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self.sync.as_ref();
        match action {
            GitAction::Sync => self.act("Syncing…", RepoAction::Sync, cx),
            GitAction::Continue => self.act("Continuing…", RepoAction::Continue, cx),
            GitAction::Abort => {
                let name = state
                    .and_then(|s| s.operation)
                    .map_or("operation", |o| o.name());
                self.confirm(
                    "Abort",
                    &format!("Give up the {name}? The files go back to how they were before it."),
                    "Abort",
                    |this, cx| this.act("Aborting…", RepoAction::Abort, cx),
                    window,
                    cx,
                );
            }
            GitAction::CommitPush | GitAction::CommitPushPr | GitAction::Push => {
                // A branch never pushed goes to origin, or to the only remote; among others
                // there is no telling which.
                let remotes = state
                    .filter(|s| !s.has_upstream && s.remotes.len() > 1)
                    .map(|s| s.remotes.clone())
                    .filter(|remotes| !remotes.iter().any(|r| r == "origin"))
                    .unwrap_or_default();
                if remotes.is_empty() {
                    return self.run(action, None, CommitOptions::default(), cx);
                }
                let entity = cx.entity().downgrade();
                let items = remotes
                    .into_iter()
                    .map(|remote| {
                        let entity = entity.clone();
                        MenuItem::new(remote.clone(), move |_, cx| {
                            let remote = Some(remote.clone());
                            let _ = entity.update(cx, |this, cx| {
                                this.run(action, remote, CommitOptions::default(), cx);
                            });
                        })
                    })
                    .collect();
                self.open_menu("Publish to", items, at, None, window, cx);
            }
            _ => self.run(action, None, CommitOptions::default(), cx),
        }
    }

    /// Asks before something that cannot be taken back, then does it.
    pub(super) fn confirm(
        &mut self,
        title: &str,
        detail: &str,
        yes: &str,
        then: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = window.prompt(
            PromptLevel::Warning,
            title,
            Some(detail),
            &[yes, "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, then);
            }
        })
        .detach();
    }

    /// Runs one of the Commit button's actions, step by step, off the main thread. A push goes
    /// to `remote` when the branch has none yet, and a commit is made with `options`.
    pub(super) fn run(
        &mut self,
        action: GitAction,
        remote: Option<String>,
        options: CommitOptions,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        self.result = None;
        if action == GitAction::ViewPr {
            if let Some(pr) = self.sync.as_ref().and_then(|s| s.pull_request.as_ref()) {
                cx.open_url(&pr.url);
            }
            return;
        }
        let (Some(core), Some(id)) = (self.core(cx), self.project_id.clone()) else {
            return;
        };
        let thread = self.thread_id(cx);
        let shown = self.root.clone();
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
        let pull = action == GitAction::Pull;
        self.busy = Some(
            if pull {
                "Pulling…"
            } else if !commit {
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
            if pull {
                let (core, id, thread) = (Arc::clone(&core), id.clone(), thread.clone());
                match cx
                    .background_spawn(async move { core.pull_changes(&id, thread.as_deref()) })
                    .await
                {
                    Ok(from) => done = Some((format!("Pulled from {from}"), None)),
                    Err(e) => failed = Some(e),
                }
            }
            if commit {
                let (core, id, thread) = (Arc::clone(&core), id.clone(), thread.clone());
                match cx
                    .background_spawn(async move {
                        core.commit_changes(&id, thread.as_deref(), &message, options)
                    })
                    .await
                {
                    Ok((hash, subject)) => {
                        done = Some((format!("Committed {hash}: {subject}"), None));
                        let _ = this.update(cx, |this, cx| {
                            // The folder showing now may not be the one that was committed.
                            if let Some(root) = &shown {
                                this.drafts.remove(root);
                            }
                            if this.root == shown {
                                this.message.update(cx, |m, cx| m.set_text("", cx));
                            }
                        });
                    }
                    Err(e) => failed = Some(e),
                }
            }
            if failed.is_none() && push {
                let _ = this.update(cx, |this, cx| {
                    if this.root == shown {
                        this.busy = Some("Pushing…".into());
                        cx.notify();
                    }
                });
                let (core, id, thread) = (Arc::clone(&core), id.clone(), thread.clone());
                match cx
                    .background_spawn(async move {
                        let root = core.source_root(&id, thread.as_deref())?;
                        patchwork_core::workspace::push(&root, remote.as_deref(), false)
                    })
                    .await
                {
                    Ok(to) => done = Some((format!("Pushed to {to}"), None)),
                    Err(e) => failed = Some(e),
                }
            }
            if failed.is_none() && pr {
                let _ = this.update(cx, |this, cx| {
                    if this.root == shown {
                        this.busy = Some("Opening pull request…".into());
                        cx.notify();
                    }
                });
                let (core, id) = (Arc::clone(&core), id.clone());
                match cx
                    .background_spawn(async move { core.open_pull_request(&id, thread.as_deref()) })
                    .await
                {
                    Ok(pr) => done = Some((format!("Pull request #{}", pr.number), Some(pr))),
                    Err(e) => failed = Some(e),
                }
            }
            let _ = this.update(cx, |this, cx| {
                // Another task's folder shows its own state; this one's result is not its.
                if this.root == shown {
                    // What finished before a failure still shows, above the error.
                    this.set_result(done, cx);
                    this.error = failed.map(|e| e.to_string());
                    this.busy = None;
                }
                this.refresh_all(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// The open chat or task: its model writes a message when no title model is set, and its
    /// worktree, when it has one, is the folder Source Control works in.
    pub(super) fn thread_id(&self, cx: &Context<Self>) -> Option<String> {
        self.workspace
            .upgrade()?
            .read(cx)
            .thread
            .as_ref()
            .map(|t| t.id.clone())
    }

    pub(super) fn write_message(&mut self, cx: &mut Context<Self>) {
        let (Some(core), Some(id)) = (self.core(cx), self.project_id.clone()) else {
            return;
        };
        let thread = self.thread_id(cx);
        self.error = None;
        self.busy = Some("Writing message…".into());
        cx.notify();
        let shown = self.root.clone();
        let task =
            cx.background_spawn(async move { core.write_commit_message(&id, thread.as_deref()) });
        cx.spawn(async move |this, cx| {
            let written = task.await;
            this.update(cx, |this, cx| {
                let here = this.root == shown;
                match written {
                    Ok(text) => {
                        // The folder showing now may not be the one the message is for.
                        if here {
                            this.message.update(cx, |m, cx| m.set_text(&text, cx));
                        }
                        if let Some(root) = shown {
                            this.drafts.insert(root, text);
                        }
                    }
                    Err(e) if here => this.error = Some(e.to_string()),
                    Err(_) => {}
                }
                if here {
                    this.busy = None;
                }
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
        step: fn(&str, &str) -> Result<(), patchwork_core::Error>,
        cx: &mut Context<Self>,
    ) {
        let (Some(core), Some(id)) = (self.core(cx), self.project_id.clone()) else {
            return;
        };
        let thread = self.thread_id(cx);
        self.error = None;
        let shown = self.root.clone();
        crate::ui::off_thread(
            cx,
            move || {
                let root = core.source_root(&id, thread.as_deref())?;
                for file in &files {
                    step(&root, file)?;
                }
                Ok::<_, patchwork_core::Error>(())
            },
            move |this, done, cx| {
                if this.root == shown {
                    this.error = done.err().map(|e| e.to_string());
                }
                let _ = this
                    .workspace
                    .update(cx, crate::workspace::Workspace::load_git);
                cx.notify();
            },
        );
    }

    /// Stages the files. One that was in conflict and still has conflict markers is asked about
    /// first, since staging it says the conflict is resolved.
    pub(super) fn stage(
        &mut self,
        files: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stage = patchwork_core::workspace::accept_file;
        let conflicted: Vec<String> = self.workspace.upgrade().map_or_else(Vec::new, |w| {
            let changes = &w.read(cx).changes;
            let in_conflict = |file: &&String| {
                changes
                    .iter()
                    .any(|c| &c.path == *file && c.status == "conflicted")
            };
            files.iter().filter(in_conflict).cloned().collect()
        });
        let Some(root) = self.root.clone().filter(|_| !conflicted.is_empty()) else {
            return self.each_file(files, stage, cx);
        };
        let check = cx.background_spawn(async move {
            conflicted
                .into_iter()
                .filter(|file| patchwork_core::workspace::has_conflict_markers(&root, file))
                .collect::<Vec<_>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let marked = check.await;
            if !marked.is_empty() {
                let what = match marked.as_slice() {
                    [one] => format!("{} still has", base_name(one)),
                    many => format!("{} files still have", many.len()),
                };
                let Ok(answer) = this.update_in(cx, |_, window, cx| {
                    window.prompt(
                        PromptLevel::Warning,
                        "Stage",
                        Some(&format!("{what} conflict markers. Stage anyway?")),
                        &["Stage", "Cancel"],
                        cx,
                    )
                }) else {
                    return;
                };
                if answer.await != Ok(0) {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| this.each_file(files, stage, cx));
        })
        .detach();
    }

    /// Takes the files back out of the index, keeping their changes.
    pub(super) fn unstage(&mut self, files: Vec<String>, cx: &mut Context<Self>) {
        self.each_file(files, patchwork_core::workspace::unstage_file, cx);
    }

    /// Discards the files with `step` after one confirmation.
    pub(super) fn discard(
        &mut self,
        files: Vec<String>,
        step: fn(&str, &str) -> Result<(), patchwork_core::Error>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if files.is_empty() {
            return;
        }
        let what = match files.as_slice() {
            [one] => base_name(one).to_string(),
            many => format!("{} files", many.len()),
        };
        // A new file has no earlier version to go back to: it is moved to the Trash.
        let all_new = self.workspace.upgrade().is_some_and(|w| {
            let changes = &w.read(cx).changes;
            files.iter().all(|file| {
                changes
                    .iter()
                    .any(|c| &c.path == file && c.status == "untracked")
            })
        });
        let (title, detail, yes) = if all_new {
            (
                "Move to Trash",
                format!("Move {what} to the Trash?"),
                "Move to Trash",
            )
        } else {
            ("Discard", format!("Discard changes to {what}?"), "Discard")
        };
        self.confirm(
            title,
            &detail,
            yes,
            move |this, cx| this.each_file(files, step, cx),
            window,
            cx,
        );
    }

    /// Starts a task whose agent resolves the conflicts, given both sides of each.
    pub(super) fn resolve_with_agent(&mut self, cx: &mut Context<Self>) {
        let (Some(core), Some(id)) = (self.core(cx), self.project_id.clone()) else {
            return;
        };
        self.error = None;
        self.resolving = true;
        cx.notify();
        let shown = self.root.clone();
        let thread = self.thread_id(cx);
        crate::ui::off_thread(
            cx,
            move || core.conflict_prompt(&id, thread.as_deref()),
            move |this, prompt, cx| {
                this.resolving = false;
                match prompt {
                    // The task would start in the folder showing now, with another's conflicts.
                    Ok(_) if this.root != shown => {}
                    Ok(text) => {
                        if let Some(w) = this.workspace.upgrade() {
                            w.update(cx, |w, cx| {
                                w.start_task("Resolve merge conflicts", text, cx);
                            });
                        }
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            },
        );
    }

    /// Sends the pull request's failures and comments to the thread that made the branch's newest
    /// commit, or starts a task with them when none did.
    pub(super) fn send_feedback(&mut self, number: u32, cx: &mut Context<Self>) {
        let (Some(core), Some(id)) = (self.core(cx), self.project_id.clone()) else {
            return;
        };
        self.pr_error = None;
        self.pr_busy = true;
        cx.notify();
        let shown = self.root.clone();
        let thread = self.thread_id(cx);
        let task =
            cx.background_spawn(async move { core.pull_request_feedback(&id, thread.as_deref()) });
        cx.spawn(async move |this, cx| {
            let feedback = task.await;
            this.update(cx, |this, cx| {
                this.pr_busy = false;
                match feedback {
                    // The task would start in the folder showing now, with another's feedback.
                    Ok(_) if this.root != shown => {}
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
                    Err(e) => this.pr_error = Some(e.to_string()),
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
            let Ok(Some(core)) = this.update(cx, |this, cx| this.core(cx)) else {
                return;
            };
            let undone = cx
                .background_spawn(async move { core.undo_reply(&thread_id, &message_id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.undo_error = undone.err().map(|e| e.to_string());
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
        cx.subscribe_in(&menu, window, |this, menu, e: &Dismissed, _, cx| {
            this.dismissed = e.0;
            // A row of this menu may have opened another in its place by now.
            if this.menu.as_ref().is_some_and(|(open, ..)| open == menu) {
                this.menu = None;
            }
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

    pub(super) fn set_graph_height(&mut self, height: f32, total: f32) {
        self.graph_height = height.min(total - PANE_MIN).max(PANE_MIN).round();
    }
}
