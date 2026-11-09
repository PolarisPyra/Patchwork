//! Projects, the folders that group their threads, and what the sidebar remembers about them.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::system::new_id;
use crate::threads::title::clean_name;
use crate::Server;
use crate::{watch, workspace};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// A folder inside a project. Its threads are those whose `folder_id` names it.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown or its folder cannot be watched.
    pub fn watch_project(
        self: &Arc<Self>,
        project_id: &str,
        on_change: impl Fn(crate::workspace::ProjectChanged) + Send + 'static,
    ) -> Result<(), Error> {
        let mut current = self
            .project_watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current.as_ref().is_some_and(|(id, _)| id == project_id) {
            return Ok(());
        }
        let root = self.project_root(project_id)?;
        let folder = PathBuf::from(&root);
        // The watcher lives in the server, so it holds the server weakly.
        let server = Arc::downgrade(self);
        let id = project_id.to_string();
        let watcher = watch::ProjectWatcher::start(&folder, move |burst| {
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

    /// The folder of the project `project_id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown.
    pub fn project_root(&self, project_id: &str) -> Result<String, Error> {
        self.store
            .settings()
            .projects
            .into_iter()
            .find(|p| p.id == project_id)
            .map(|p| p.path)
            .ok_or_else(|| format!("unknown project {project_id}").into())
    }

    /// Adds `project` unless its path is already registered; returns the stored one.
    pub(crate) fn register_project(
        &self,
        project: Project,
        activate: bool,
    ) -> Result<Project, Error> {
        self.store.update_settings(|s| {
            let p = if let Some(existing) = s.projects.iter().find(|p| p.path == project.path) {
                existing.clone()
            } else {
                s.projects.push(project.clone());
                project
            };
            if activate {
                s.active_project_id = Some(p.id.clone());
            }
            p
        })
    }

    /// Adds the folder at `path` as a project.
    ///
    /// # Errors
    ///
    /// Returns an error when `path` cannot be opened or is not a folder, or when the settings
    /// cannot be saved.
    pub fn add_project(&self, path: &str) -> Result<Project, Error> {
        self.register_project(workspace::make_project(path)?, true)
    }

    /// Makes the folder `name` in `parent` and adds it as a project.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is not a single folder name, when the folder cannot be created
    /// in `parent`, or when the settings cannot be saved.
    pub fn create_project(&self, parent: &str, name: &str) -> Result<Project, Error> {
        self.register_project(workspace::create_project_dir(parent, name)?, true)
    }

    /// Renames the project `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is empty or invalid, when the project is unknown, or when the
    /// settings cannot be saved.
    pub fn rename_project(&self, id: &str, name: &str) -> Result<(), Error> {
        let name = clean_name(name)?;
        self.store.update_settings(|s| {
            let project = s.projects.iter_mut().find(|p| p.id == id);
            project.ok_or_else(|| format!("unknown project {id}"))?.name = name;
            Ok::<_, Error>(())
        })?
    }

    /// Points a project at another folder, such as after it was moved or renamed. Agents keep
    /// their sessions by folder, so the project's tasks start new ones, sent their transcript.
    ///
    /// # Errors
    ///
    /// Returns an error when `path` is not a folder, when one of the project's tasks is running,
    /// when the project is unknown or another project already uses the folder, or when the settings
    /// or a chat cannot be saved.
    pub fn relocate_project(&self, id: &str, path: &str) -> Result<(), Error> {
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
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when it is not in a git repository or the
    /// repository has no commit, or when `git` or the file system fails to create the worktree.
    pub fn create_worktree(&self, project_id: &str) -> Result<Project, Error> {
        let source = self
            .store
            .settings()
            .projects
            .into_iter()
            .find(|p| p.id == project_id)
            .ok_or_else(|| format!("unknown project {project_id}"))?;
        let top = workspace::run_git(&source.path, &["rev-parse", "--show-toplevel"])
            .map_err(|_discarded| format!("{} isn't a git repository", source.name))?;
        let top = top.trim();
        if workspace::run_git(top, &["rev-parse", "--verify", "-q", "HEAD"]).is_err() {
            return Err("Make a first commit before starting a task in a worktree".into());
        }
        // A project in a folder of its repository opens the same folder of the worktree.
        let prefix = workspace::run_git(&source.path, &["rev-parse", "--show-prefix"])?;
        let short = &new_id()[..8];
        let folder = Path::new(top)
            .file_name()
            .map_or_else(|| "worktree".into(), |n| n.to_string_lossy().into_owned());
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
    fn remove_worktree(&self, id: &str, path: &str) -> Result<(), Error> {
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

    /// Removes the project, with its worktree if Patchwork made one.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when its worktree cannot be removed, or when
    /// the database cannot save the removal.
    pub fn remove_project(&self, id: &str) -> Result<(), Error> {
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
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown or its files cannot be listed.
    pub fn list_files(&self, project_id: &str) -> Result<Vec<String>, Error> {
        workspace::list_files(&self.project_root(project_id)?)
    }

    /// Ids of the projects and folders that were collapsed in the sidebar.
    pub fn collapsed_sections(&self) -> Vec<String> {
        self.store
            .state(COLLAPSED_KEY)
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    /// Saves which projects and folders the sidebar has collapsed.
    ///
    /// # Errors
    ///
    /// Returns an error when the list cannot be serialized or saved.
    pub fn set_collapsed_sections(&self, ids: &[String]) -> Result<(), Error> {
        let json = serde_json::to_string(ids).map_err(|e| e.to_string())?;
        self.store.set_state(COLLAPSED_KEY, &json)
    }

    pub fn list_folders(&self) -> Vec<Folder> {
        self.store.folders()
    }

    pub(crate) fn folder(&self, id: &str) -> Result<Folder, Error> {
        self.store
            .folders()
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| format!("unknown folder {id}").into())
    }

    /// Makes a folder named `name` in the project.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when `name` is empty or invalid, or when the
    /// database cannot save the folder.
    pub fn create_folder(&self, project_id: &str, name: &str) -> Result<Folder, Error> {
        self.project_root(project_id)?;
        let folder = Folder {
            id: new_id(),
            project_id: project_id.into(),
            name: clean_name(name)?,
        };
        self.store.create_folder(folder.clone())?;
        Ok(folder)
    }

    /// Renames the folder `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is empty or invalid, when the folder is unknown, or when the
    /// database cannot save the new name.
    pub fn rename_folder(&self, id: &str, name: &str) -> Result<(), Error> {
        let name = clean_name(name)?;
        self.folder(id)?;
        self.store.rename_folder(id, &name)
    }

    /// Its tasks move to the project's top level.
    ///
    /// # Errors
    ///
    /// Returns an error when the folder is unknown or the database cannot delete it.
    pub fn remove_folder(&self, id: &str) -> Result<(), Error> {
        self.folder(id)?;
        self.store.delete_folder(id)
    }
}
