//! The open project: its git state, its folder, and the sidebar's sections.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;
use gpui::{Entity, Window, prelude::*};
use patchwork_core::projects::Project;
use patchwork_core::workspace::ProjectChanged;
use crate::workspace::add_project::{AddProject, Closed as AddProjectClosed};
use super::state::{View, Workspace};
use crate::sidebar::devices::{DevicesView, OpenDevice};

impl Workspace {
    /// Reads the project's branch and changed files off the main thread, and watches the project
    /// so they stay current whoever changes its files: an agent, an editor, or git in a terminal.
    /// Source Control shows the open task's worktree when it works in one.
    pub fn load_git(&mut self, cx: &mut Context<Self>) {
        self.read_git(true, cx);
    }

    /// Like [`Self::load_git`], but asks where the branch stands (over the network) only when
    /// `sync` is set or Source Control's folder changed, as it does on moving to another task.
    pub(crate) fn read_git(&mut self, sync: bool, cx: &mut Context<Self>) {
        self.git_seq += 1;
        let seq = self.git_seq;
        let Some(project) = self.project() else {
            self.branch = None;
            self.changes.clear();
            self.git_root = None;
            self.changes_generation += 1;
            self.changes_view
                .update(cx, |v, cx| v.refresh(None, None, true, cx));
            return;
        };
        let id = project.id.clone();
        let thread = self.thread.as_ref().map(|t| t.id.clone());
        let (core, events) = (Arc::clone(&self.core), self.project_events.clone());
        let read = id.clone();
        let task = cx.background_spawn(async move {
            let root = core.source_root(&read, thread.as_deref())?;
            let watched = core.watch_project(&read, thread.as_deref(), seq, move |changed| {
                let _ = events.unbounded_send(changed);
            });
            let branch = patchwork_core::workspace::git_branch(&root);
            let changes = patchwork_core::workspace::git_changes(&root)
                .inspect_err(|e| {
                    log::warn!(error:% = e; "could not list the changed files: {{error}}");
                })
                .ok();
            Ok::<_, patchwork_core::Error>((root, watched, branch, changes))
        });
        cx.spawn(async move |this, cx| {
            let read = task.await;
            this.update(cx, |this, cx| {
                // A newer read, for another task or project, has started since.
                if this.git_seq != seq {
                    return;
                }
                let (root, watched, branch, changes) = match read {
                    Ok(read) => read,
                    Err(e) => {
                        this.error = Some(e.to_string());
                        return cx.notify();
                    }
                };
                if let Err(e) = watched {
                    this.error = Some(e.to_string());
                }
                let moved = this.git_root.as_ref() != Some(&root);
                match changes {
                    Some(changes) => this.changes = changes,
                    // A failed read keeps the last good list of the same folder, and shows none
                    // of another's.
                    None if moved => this.changes.clear(),
                    None => {}
                }
                this.git_root = Some(root.clone());
                this.branch = branch;
                this.changes_generation += 1;
                this.changes_view.update(cx, |v, cx| {
                    v.refresh(Some(id), Some(root), sync || moved, cx);
                });
                cx.notify();
            })
        })
        .detach();
    }

    /// Git's state after a settled burst of changes in the watched project.
    pub(super) fn on_project_changed(&mut self, changed: ProjectChanged, cx: &mut Context<Self>) {
        // A burst from a folder Source Control no longer shows, sent before its watch ended.
        if self.project().map(|p| &p.id) != Some(&changed.project_id)
            || self.git_root.as_ref() != Some(&changed.root)
        {
            return;
        }
        if let Some(canvas) = &self.canvas {
            canvas.update(cx, |c, cx| c.on_paths(&changed.paths, cx));
        }
        if changed
            .paths
            .iter()
            .any(|p| patchwork_core::android::is_marker(p))
        {
            self.detect_mobile(cx);
        }
        // A run that stays attached takes saved code without a rebuild.
        let reload = self
            .android
            .as_ref()
            .and_then(|android| android.read(cx).reload_on_save(&changed.paths));
        if let Some(keys) = reload {
            self.type_in_run(keys, cx);
        }
        let Some(files) = changed.changes else {
            return;
        };
        let had_changes = !self.changes.is_empty();
        let git = changed.branch != self.branch
            || had_changes == files.is_empty()
            || changed.paths.iter().any(|p| p.starts_with(".git/"));
        self.branch = changed.branch;
        self.changes = files;
        self.changes_generation += 1;
        let (id, root) = (changed.project_id, changed.root);
        self.changes_view
            .update(cx, |v, cx| v.refresh(Some(id), Some(root), git, cx));
        cx.notify();
    }

    pub fn toggle_collapsed(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_string());
        }
        self.save_collapsed(cx);
        cx.notify();
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        for project in &self.settings.projects {
            self.collapsed.insert(project.id.clone());
        }
        self.save_collapsed(cx);
        cx.notify();
    }

    /// Saves which projects and folders are folded away, off the UI thread.
    pub fn save_collapsed(&self, cx: &gpui::App) {
        let ids: Vec<String> = self.collapsed.iter().cloned().collect();
        let core = Arc::clone(&self.core);
        cx.background_spawn(async move {
            if let Err(e) = core.set_collapsed_sections(&ids) {
                log::warn!(error:% = e; "could not save the open sections: {{error}}");
            }
        })
        .detach();
    }
}

impl Workspace {
    /// Makes `id` the project new tasks go in.
    pub(super) fn remember_project(&mut self, id: Option<String>) {
        if self.settings.active_project_id == id {
            return;
        }
        self.settings.active_project_id.clone_from(&id);
        if let Err(e) = self.core.update_settings(|s| s.active_project_id = id) {
            log::warn!(error:% = e; "could not record the active project: {{error}}");
        }
    }

    /// Opens the project's newest chat, or an empty one in it when it has none.
    pub fn select_project(&mut self, id: &str, cx: &mut Context<Self>) {
        self.remember_project(Some(id.to_string()));
        let first = self
            .threads
            .iter()
            .find(|t| {
                t.project_id.as_deref() == Some(id) && t.archived_at.is_none() && t.parent.is_none()
            })
            .map(|t| t.id.clone());
        if let Some(thread) = first {
            self.open_thread(&thread, cx);
        } else {
            // No thread opens to read the project's branch and changes, so they are read here.
            self.new_chat(cx);
            self.load_git(cx);
        }
    }

    /// Shows the thread in the task list, opening its project and folder, and opens it.
    pub(super) fn reveal_thread(&mut self, id: &str, cx: &mut Context<Self>) {
        self.view = Some(View::Tasks);
        if let Some(thread) = self.threads.iter().find(|t| t.id == id) {
            let mut opened = false;
            if let Some(project) = &thread.project_id {
                opened |= self.collapsed.remove(project);
            }
            if let Some(folder) = &thread.folder_id {
                opened |= self.collapsed.remove(&format!("folder:{folder}"));
            }
            if opened {
                self.save_collapsed(cx);
            }
        }
        self.open_thread(id, cx);
    }

    /// Opens the dialog that picks a folder to add as a project, on the machine picked in the
    /// toolbar.
    pub fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.add_project.is_none() {
            let host = self.settings.default_machine.clone();
            self.open_folder_on(host, window, cx);
        }
    }

    /// Opens that dialog on `host`, or on this computer when it is none.
    fn open_folder_on(
        &mut self,
        host: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(dialog) = &self.add_project {
            dialog.update(cx, |d, cx| d.open_machine(host, cx));
            return;
        }
        let return_focus = window.focused(cx);
        let core = Arc::clone(&self.core);
        let dialog = cx.new(|cx| AddProject::new(core, host, window, cx));
        cx.subscribe_in(
            &dialog,
            window,
            move |this, _, closed: &AddProjectClosed, window, cx| {
                this.add_project = None;
                match &return_focus {
                    Some(focus) => window.focus(focus, cx),
                    None => window.focus(&this.focus, cx),
                }
                if let Some(project) = &closed.0 {
                    this.project_added(project, cx);
                }
                cx.notify();
            },
        )
        .detach();
        self.add_project = Some(dialog);
        cx.notify();
    }

    /// The Devices view, made and hooked up the first time it is shown.
    pub fn devices_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<DevicesView> {
        if let Some(view) = &self.devices_view {
            return view.clone();
        }
        let view = cx.new(DevicesView::new);
        // The toolbar's machine button shows what the view learns about each machine.
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&view, window, |this, _, open: &OpenDevice, window, cx| {
            this.open_folder_on(open.0.clone(), window, cx);
        })
        .detach();
        self.devices_view = Some(view.clone());
        view
    }

    /// Adds the folders dropped on the sidebar as projects, off the main thread, and opens the
    /// last one added.
    pub fn add_dropped_folders(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        let paths = paths.to_vec();
        let task = cx.background_spawn(async move {
            paths
                .iter()
                .map(|path| core.add_project(&path.to_string_lossy()))
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let added = task.await;
            this.update(cx, |this, cx| {
                for result in added {
                    match result {
                        Ok(project) => this.project_added(&project, cx),
                        Err(e) => this.error = Some(e.to_string()),
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    fn project_added(&mut self, project: &Project, cx: &mut Context<Self>) {
        // A new project starts closed like the rest of the list; one already there keeps its
        // state.
        if !self.settings.projects.iter().any(|p| p.id == project.id) {
            self.collapsed.insert(project.id.clone());
            self.save_collapsed(cx);
        }
        self.settings = self.core.settings();
        self.select_project(&project.id, cx);
    }
}
