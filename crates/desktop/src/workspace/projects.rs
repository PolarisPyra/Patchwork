//! The open project: its git state, its folder, and the sidebar's sections.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;
use gpui::{Window, prelude::*};
use patchwork_server::projects::Project;
use patchwork_server::workspace::ProjectChanged;
use crate::workspace::add_project::{AddProject, Closed as AddProjectClosed};
use super::state::{View, Workspace};

impl Workspace {
    /// Reads the project's branch and changed files off the main thread, and watches the project
    /// so they stay current whoever changes its files: an agent, an editor, or git in a terminal.
    pub fn load_git(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self.project() else {
            self.branch = None;
            self.changes.clear();
            self.changes_view
                .update(cx, |v, cx| v.refresh(None, true, cx));
            return;
        };
        let (id, root) = (project.id.clone(), project.path.clone());
        let events = self.project_events.clone();
        if let Err(e) = self.server.watch_project(&id, move |changed| {
            let _ = events.unbounded_send(changed);
        }) {
            self.error = Some(e.to_string());
        }
        let task = cx.background_spawn(async move {
            let branch = patchwork_server::workspace::git_branch(&root);
            let changes = patchwork_server::workspace::git_changes(&root).unwrap_or_default();
            (branch, changes)
        });
        cx.spawn(async move |this, cx| {
            let (branch, changes) = task.await;
            this.update(cx, |this, cx| {
                if this.project().map(|p| &p.id) != Some(&id) {
                    return;
                }
                this.branch = branch;
                this.changes = changes;
                this.changes_view
                    .update(cx, |v, cx| v.refresh(Some(id), true, cx));
                cx.notify();
            })
        })
        .detach();
    }

    /// Git's state after a settled burst of changes in the watched project.
    pub(super) fn on_project_changed(&mut self, changed: ProjectChanged, cx: &mut Context<Self>) {
        if self.project().map(|p| &p.id) != Some(&changed.project_id) {
            return;
        }
        if let Some(canvas) = &self.canvas {
            canvas.update(cx, |c, cx| c.on_paths(&changed.paths, cx));
        }
        if changed
            .paths
            .iter()
            .any(|p| patchwork_server::android::is_marker(p))
        {
            self.detect_mobile(cx);
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
        let id = changed.project_id;
        self.changes_view
            .update(cx, |v, cx| v.refresh(Some(id), git, cx));
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
        let server = Arc::clone(&self.server);
        cx.background_spawn(async move {
            if let Err(e) = server.set_collapsed_sections(&ids) {
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
        if let Err(e) = self
            .server
            .store
            .update_settings(|s| s.active_project_id = id)
        {
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
        match first {
            Some(thread) => self.open_thread(&thread, cx),
            None => self.new_chat(cx),
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

    /// Opens the dialog that picks a folder to add as a project.
    pub fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.add_project.is_some() {
            return;
        }
        let return_focus = window.focused(cx);
        let server = Arc::clone(&self.server);
        let dialog = cx.new(|cx| AddProject::new(server, window, cx));
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

    /// Adds the folders dropped on the sidebar as projects, off the main thread, and opens the
    /// last one added.
    pub fn add_dropped_folders(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let server = Arc::clone(&self.server);
        let paths = paths.to_vec();
        let task = cx.background_spawn(async move {
            paths
                .iter()
                .map(|path| server.add_project(&path.to_string_lossy()))
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
        self.settings = self.server.store.settings();
        self.select_project(&project.id, cx);
    }
}
