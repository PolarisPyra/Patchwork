//! Source Control's actions: committing, pushing, staging, discarding, and handing work to an agent.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{ClickEvent, Pixels, Point, PromptLevel, WeakEntity, Window, prelude::*};
use patchwork_server::workspace::PullRequest;

use crate::ui::menu::{Dismissed, Menu, MenuItem};
use crate::ui::base_name;
use crate::workspace::Workspace;
use super::state::{ChangesView, GitAction, PANE_MIN};

impl ChangesView {
    /// Runs one of the Commit button's actions, step by step, off the main thread.
    pub(super) fn run(&mut self, action: GitAction, cx: &mut Context<Self>) {
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
                let project = id.clone();
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
                            // The project showing now may not be the one that was committed.
                            this.drafts.remove(&project);
                            if this.project_id.as_ref() == Some(&project) {
                                this.message.update(cx, |m, cx| m.set_text("", cx));
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
                        patchwork_server::workspace::push(&root)
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
                this.error = failed.map(|e| e.to_string());
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

    pub(super) fn write_message(&mut self, cx: &mut Context<Self>) {
        let (Some(server), Some(id)) = (self.server(cx), self.project_id.clone()) else {
            return;
        };
        let thread = self.thread_id(cx);
        self.error = None;
        self.busy = Some("Writing message…".into());
        cx.notify();
        let project = id.clone();
        let task =
            cx.background_spawn(async move { server.write_commit_message(&id, thread.as_deref()) });
        cx.spawn(async move |this, cx| {
            let written = task.await;
            this.update(cx, |this, cx| {
                match written {
                    Ok(text) => {
                        // The project showing now may not be the one the message is for.
                        if this.project_id.as_ref() == Some(&project) {
                            this.message.update(cx, |m, cx| m.set_text(&text, cx));
                        }
                        this.drafts.insert(project, text);
                    }
                    Err(e) => this.error = Some(e.to_string()),
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
        step: fn(&str, &str) -> Result<(), patchwork_server::Error>,
        cx: &mut Context<Self>,
    ) {
        let (Some(server), Some(id)) = (self.server(cx), self.project_id.clone()) else {
            return;
        };
        self.error = None;
        let task = cx.background_spawn(async move {
            let root = server.project_root(&id)?;
            for file in &files {
                step(&root, file)?;
            }
            Ok::<_, patchwork_server::Error>(())
        });
        cx.spawn(async move |this, cx| {
            let done = task.await;
            this.update(cx, |this, cx| {
                this.error = done.err().map(|e| e.to_string());
                if let Some(w) = this.workspace.upgrade() {
                    w.update(cx, crate::workspace::Workspace::load_git);
                }
                cx.notify();
            })
        })
        .detach();
    }

    pub(super) fn stage(&mut self, files: Vec<String>, cx: &mut Context<Self>) {
        self.each_file(files, patchwork_server::workspace::accept_file, cx);
    }

    /// Reverts the files after one confirmation.
    pub(super) fn discard(
        &mut self,
        files: Vec<String>,
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
                this.each_file(files, patchwork_server::workspace::revert_file, cx);
            });
        })
        .detach();
    }

    /// Starts a task whose agent resolves the conflicts, given both sides of each.
    pub(super) fn resolve_with_agent(&mut self, cx: &mut Context<Self>) {
        let (Some(server), Some(id)) = (self.server(cx), self.project_id.clone()) else {
            return;
        };
        self.error = None;
        self.resolving = true;
        cx.notify();
        let project = id.clone();
        let task = cx.background_spawn(async move { server.conflict_prompt(&id) });
        cx.spawn(async move |this, cx| {
            let prompt = task.await;
            this.update(cx, |this, cx| {
                this.resolving = false;
                match prompt {
                    // The task would start in the project showing now, with another's conflicts.
                    Ok(_) if this.project_id.as_ref() != Some(&project) => {}
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
            })
        })
        .detach();
    }

    /// Sends the pull request's failures and comments to the thread that made the branch's newest
    /// commit, or starts a task with them when none did.
    pub(super) fn send_feedback(&mut self, number: u32, cx: &mut Context<Self>) {
        let (Some(server), Some(id)) = (self.server(cx), self.project_id.clone()) else {
            return;
        };
        self.pr_error = None;
        self.pr_busy = true;
        cx.notify();
        let project = id.clone();
        let task = cx.background_spawn(async move { server.pull_request_feedback(&id) });
        cx.spawn(async move |this, cx| {
            let feedback = task.await;
            this.update(cx, |this, cx| {
                this.pr_busy = false;
                match feedback {
                    // The task would start in the project showing now, with another's feedback.
                    Ok(_) if this.project_id.as_ref() != Some(&project) => {}
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
            let Ok(Some(server)) = this.update(cx, |this, cx| this.server(cx)) else {
                return;
            };
            let undone = cx
                .background_spawn(async move { server.undo_reply(&thread_id, &message_id) })
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

    pub(super) fn set_graph_height(&mut self, height: f32, total: f32) {
        self.graph_height = height.min(total - PANE_MIN).max(PANE_MIN).round();
    }
}
