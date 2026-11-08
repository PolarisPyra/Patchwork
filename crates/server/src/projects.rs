//! Projects, the folders that group their threads, and what the sidebar remembers about them.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::system::new_id;
use crate::threads::title::clean_name;
use crate::Server;
use crate::{watch, workspace};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "projects.ts")]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// A folder inside a project. Its threads are those whose `folder_id` names it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "projects.ts")]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub project_id: String,
    pub name: String,
}

/// `app_state` key for the projects and folders the sidebar has collapsed (JSON array).
const COLLAPSED_KEY: &str = "collapsed_sections";

impl Server {
    /// Watches the project for changes, in place of any project watched before. After each
    /// settled burst it runs one `git status`, adds what replies under way there changed to
    /// them, and passes the result to `on_change`.
    pub fn watch_project(
        self: &Arc<Self>,
        project_id: &str,
        on_change: impl Fn(crate::workspace::ProjectChanged) + Send + 'static,
    ) -> Result<(), String> {
        let mut current = self
            .project_watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current.as_ref().is_some_and(|(id, _)| id == project_id) {
            return Ok(());
        }
        let root = self.project_root(project_id)?;
        let watched = PathBuf::from(&root);
        // The watcher lives in the server, so it holds the server weakly.
        let server = Arc::downgrade(self);
        let id = project_id.to_string();
        let watcher = watch::ProjectWatcher::start(&watched, move |burst| {
            let Some(server) = server.upgrade() else {
                return;
            };
            let changes = workspace::git_changes(&root).ok();
            if let Some(changes) = &changes {
                server.note_project_changes(&id, changes, &burst);
            }
            on_change(crate::workspace::ProjectChanged {
                project_id: id.clone(),
                paths: burst.into_iter().map(|c| c.path).collect(),
                branch: workspace::git_branch(&root),
                changes,
            });
        })?;
        *current = Some((project_id.to_string(), watcher));
        Ok(())
    }

    pub fn project_root(&self, project_id: &str) -> Result<String, String> {
        self.store
            .settings()
            .projects
            .into_iter()
            .find(|p| p.id == project_id)
            .map(|p| p.path)
            .ok_or_else(|| format!("unknown project {project_id}"))
    }

    /// Adds `project` unless its path is already registered; returns the stored one.
    pub(crate) fn register_project(
        &self,
        project: Project,
        activate: bool,
    ) -> Result<Project, String> {
        self.store.update_settings(|s| {
            let p = match s.projects.iter().find(|p| p.path == project.path) {
                Some(existing) => existing.clone(),
                None => {
                    s.projects.push(project.clone());
                    project
                }
            };
            if activate {
                s.active_project_id = Some(p.id.clone());
            }
            p
        })
    }

    pub fn add_project(&self, path: &str) -> Result<Project, String> {
        self.register_project(workspace::make_project(path)?, true)
    }

    pub fn create_project(&self, parent: &str, name: &str) -> Result<Project, String> {
        self.register_project(workspace::create_project_dir(parent, name)?, true)
    }

    pub fn rename_project(&self, id: &str, name: &str) -> Result<(), String> {
        let name = clean_name(name)?;
        self.store.update_settings(|s| {
            let project = s.projects.iter_mut().find(|p| p.id == id);
            project.ok_or_else(|| format!("unknown project {id}"))?.name = name;
            Ok::<_, String>(())
        })?
    }

    /// Points a project at another folder, such as after it was moved or renamed. Agents keep
    /// their sessions by folder, so the project's tasks start new ones, sent their transcript.
    pub fn relocate_project(&self, id: &str, path: &str) -> Result<(), String> {
        let path = workspace::make_project(path)?.path;
        let threads: Vec<String> = self
            .store
            .threads()
            .into_iter()
            .filter(|t| t.project_id.as_deref() == Some(id))
            .map(|t| t.id)
            .collect();
        if threads.iter().any(|t| self.runner.is_running(t)) {
            return Err("Stop this project's running tasks before changing its folder".into());
        }
        let moved = self.store.update_settings(|s| {
            if let Some(other) = s.projects.iter().find(|p| p.path == path && p.id != id) {
                return Err(format!("{path} is already the folder of {}", other.name));
            }
            let project = s.projects.iter_mut().find(|p| p.id == id);
            let project = project.ok_or_else(|| format!("unknown project {id}"))?;
            let moved = project.path != path;
            project.path = path;
            Ok(moved)
        })??;
        if moved {
            // A watch on the old folder ends, so the next `watch_project` starts one on the new.
            let mut watch = self
                .project_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if watch.as_ref().is_some_and(|(watched, _)| watched == id) {
                *watch = None;
            }
            drop(watch);
            for thread_id in &threads {
                // An idle agent still runs in the old folder.
                self.runner.retire_idle(thread_id);
                self.store.update_thread(thread_id, |t| {
                    t.session_id = None;
                    t.context_pending = !t.messages.is_empty();
                })?;
            }
        }
        Ok(())
    }

    /// Makes a git worktree of the project's repository, on a new branch from its HEAD, and adds
    /// it as a project of its own, so that its tasks work apart from the project's. Ignored
    /// files, such as installed packages, are not in it.
    pub fn create_worktree(&self, project_id: &str) -> Result<Project, String> {
        let source = self
            .store
            .settings()
            .projects
            .into_iter()
            .find(|p| p.id == project_id)
            .ok_or_else(|| format!("unknown project {project_id}"))?;
        let top = workspace::run_git(&source.path, &["rev-parse", "--show-toplevel"])
            .map_err(|_| format!("{} isn't a git repository", source.name))?;
        let top = top.trim();
        if workspace::run_git(top, &["rev-parse", "--verify", "-q", "HEAD"]).is_err() {
            return Err("Make a first commit before starting a task in a worktree".into());
        }
        // A project in a folder of its repository opens the same folder of the worktree.
        let prefix = workspace::run_git(&source.path, &["rev-parse", "--show-prefix"])?;
        let short = &new_id()[..8];
        let folder = Path::new(top)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "worktree".into());
        std::fs::create_dir_all(&self.worktrees_dir)
            .map_err(|e| format!("Cannot create {}: {e}", self.worktrees_dir.display()))?;
        let root = self.worktrees_dir.join(format!("{folder}-{short}"));
        let branch = format!("patchwork/{short}");
        workspace::run_git(
            top,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                &root.to_string_lossy(),
                "HEAD",
            ],
        )?;
        let mut project = workspace::make_project(&root.join(prefix.trim()).to_string_lossy())?;
        project.name = format!("{} · {short}", source.name);
        self.register_project(project, true)
    }

    /// When the project at `path` is a worktree `create_worktree` made, removes the worktree,
    /// refused while it has changes that aren't committed. Its branch stays with its commits.
    fn remove_worktree(&self, id: &str, path: &str) -> Result<(), String> {
        let Ok(dir) = std::fs::canonicalize(&self.worktrees_dir) else {
            return Ok(());
        };
        let Some(first) = Path::new(path)
            .strip_prefix(&dir)
            .ok()
            .and_then(|rel| rel.components().next())
        else {
            return Ok(());
        };
        let root = dir.join(first);
        // Gone already: git drops its record of the worktree by itself.
        if !root.exists() {
            return Ok(());
        }
        let root = root.to_string_lossy();
        let running = self
            .store
            .threads()
            .into_iter()
            .any(|t| t.project_id.as_deref() == Some(id) && self.runner.is_running(&t.id));
        if running {
            return Err("Stop this worktree's running tasks before removing it".into());
        }
        if !workspace::run_git(&root, &["status", "--porcelain"])?
            .trim()
            .is_empty()
        {
            return Err(
                "This worktree has changes that aren't committed. Commit or discard them before removing it"
                    .into(),
            );
        }
        let common = workspace::run_git(
            &root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let git_dir = format!("--git-dir={}", common.trim());
        workspace::run_git(
            &dir.to_string_lossy(),
            &[&git_dir, "worktree", "remove", &root],
        )?;
        Ok(())
    }

    pub fn remove_project(&self, id: &str) -> Result<(), String> {
        let path = self.project_root(id)?;
        self.remove_worktree(id, &path)?;
        self.store.update_settings(|s| {
            s.projects.retain(|p| p.id != id);
            if s.active_project_id.as_deref() == Some(id) {
                s.active_project_id = s.projects.first().map(|p| p.id.clone());
            }
        })?;
        self.store.delete_project_threads(id)
    }

    /// Files of a project that a mention can refer to.
    pub fn list_files(&self, project_id: &str) -> Result<Vec<String>, String> {
        Ok(workspace::list_files(&self.project_root(project_id)?)?)
    }

    /// Ids of the projects and folders that were collapsed in the sidebar.
    pub fn collapsed_sections(&self) -> Vec<String> {
        self.store
            .state(COLLAPSED_KEY)
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn set_collapsed_sections(&self, ids: &[String]) -> Result<(), String> {
        let json = serde_json::to_string(ids).map_err(|e| e.to_string())?;
        self.store.set_state(COLLAPSED_KEY, &json)
    }

    pub fn list_folders(&self) -> Vec<Folder> {
        self.store.folders()
    }

    pub(crate) fn folder(&self, id: &str) -> Result<Folder, String> {
        self.store
            .folders()
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| format!("unknown folder {id}"))
    }

    pub fn create_folder(&self, project_id: &str, name: &str) -> Result<Folder, String> {
        self.project_root(project_id)?;
        let folder = Folder {
            id: new_id(),
            project_id: project_id.into(),
            name: clean_name(name)?,
        };
        self.store.create_folder(folder.clone())?;
        Ok(folder)
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> Result<(), String> {
        let name = clean_name(name)?;
        self.folder(id)?;
        self.store.rename_folder(id, &name)
    }

    /// Its tasks move to the project's top level.
    pub fn remove_folder(&self, id: &str) -> Result<(), String> {
        self.folder(id)?;
        self.store.delete_folder(id)
    }
}
