//! Adding, opening and removing projects and their folders, and watching the folder Source
//! Control shows.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use crate::Error;
use crate::provider::AgentKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::system::{new_id, ssh};
use crate::chats::title::clean_name;
use crate::Core;
use crate::source_control;

use super::files::{create_project_dir, make_project};
use super::types::{Folder, Project, ProjectChanged, ProjectWatch};
use super::watch;

/// Whether the project folder at `root` is there, on this computer or on its SSH host.
///
/// This blocks on the host for a remote root. It is `None` when the host cannot be reached
/// to ask, so an offline host does not read as a deleted folder.
#[must_use]
pub fn folder_found(root: &str) -> Option<bool> {
    let Some(at) = ssh::split(root) else {
        return Some(Path::new(root).is_dir());
    };
    if ssh::fs::canonical_dir(at.host, at.path).is_ok() {
        return Some(true);
    }
    ssh::home(at.host).ok().map(|_| false)
}

/// `app_state` key for the projects and folders the sidebar has collapsed (JSON array).
const COLLAPSED_KEY: &str = "collapsed_sections";

impl Core {
    /// Watches the folder Source Control shows for the project, which is the open task
    /// `chat_id`'s worktree when it has one, in place of any folder watched before. After each
    /// settled burst it runs one `git status` and passes the result to `on_change`. What changes
    /// there is added to the replies under way in that folder, and while a worktree is shown, a
    /// second watcher does the same for the replies in the project's own folder.
    ///
    /// `seq` orders the calls: one older than the last is ignored, so a slow read cannot move
    /// the watch back to a task that is no longer open.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown or its folder cannot be watched.
    pub fn watch_project(
        self: &Arc<Self>,
        project_id: &str,
        chat_id: Option<&str>,
        seq: u64,
        on_change: impl Fn(ProjectChanged) + Send + 'static,
    ) -> Result<(), Error> {
        let root = self.source_root(project_id, chat_id)?;
        let project_root = self.project_root(project_id)?;
        let mut current = self.project_watch.locked();
        if let Some(watching) = current.as_mut() {
            if watching.seq > seq {
                return Ok(());
            }
            if watching.project_id == project_id && watching.folder == root {
                watching.seq = seq;
                return Ok(());
            }
        }
        let in_project = root == project_root;
        // The watchers live in the core, so they hold it weakly.
        let core = Arc::downgrade(self);
        let id = project_id.to_string();
        let folder = root.clone();
        let watcher = start_watcher(&root, move |burst: Vec<watch::Change>| {
            let Some(core) = core.upgrade() else {
                return;
            };
            let changes = source_control::git_changes(&folder).ok();
            if let Some(changes) = changes.as_ref() {
                core.note_project_changes(&id, &folder, changes, &burst);
            }
            on_change(ProjectChanged {
                project_id: id.clone(),
                root: folder.clone(),
                paths: burst.into_iter().map(|c| c.path).collect(),
                branch: source_control::git_branch(&folder),
                changes,
            });
        })?;
        let credit = if in_project {
            None
        } else {
            let core = Arc::downgrade(self);
            let id = project_id.to_string();
            let folder = project_root.clone();
            start_watcher(&project_root, move |burst: Vec<watch::Change>| {
                let Some(core) = core.upgrade() else {
                    return;
                };
                if let Ok(changes) = source_control::git_changes(&folder) {
                    core.note_project_changes(&id, &folder, &changes, &burst);
                }
            })
            .inspect_err(|e| {
                log::warn!(error:% = e; "could not watch the project for replies: {{error}}");
            })
            .ok()
        };
        *current = Some(ProjectWatch {
            seq,
            project_id: project_id.to_string(),
            folder: root,
            _watcher: watcher,
            _credit: credit,
        });
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
        self.update_settings(|s| {
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
        self.register_project(make_project(path)?, true)
    }

    /// Makes the folder `name` in `parent` and adds it as a project.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is not a single folder name, when the folder cannot be created
    /// in `parent`, or when the settings cannot be saved.
    pub fn create_project(&self, parent: &str, name: &str) -> Result<Project, Error> {
        self.register_project(create_project_dir(parent, name)?, true)
    }

    /// Renames the project `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is empty or invalid, when the project is unknown, or when the
    /// settings cannot be saved.
    pub fn rename_project(&self, id: &str, name: &str) -> Result<(), Error> {
        let name = clean_name(name)?;
        self.update_settings(|s| {
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
        let path = make_project(path)?.path;
        let chats: Vec<String> = self
            .store
            .chats()
            .into_iter()
            .filter(|t| t.project_id.as_deref() == Some(id))
            .map(|t| t.id)
            .collect();
        if chats.iter().any(|t| self.runner.is_running(t)) {
            return Err("Stop this project's running tasks before changing its folder".into());
        }
        let moved = self.update_settings(|s| {
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
            let mut watch = self.project_watch.locked();
            if watch.as_ref().is_some_and(|w| w.project_id == id) {
                *watch = None;
            }
            drop(watch);
            for chat_id in &chats {
                // An idle agent still runs in the old folder, and may still be at work there.
                self.runner.replace_idle(chat_id);
                self.store.update_chat(chat_id, |t| {
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
        let top = source_control::run_git(&source.path, &["rev-parse", "--show-toplevel"])
            .map_err(|_discarded| format!("{} isn't a git repository", source.name))?;
        let top = ssh::same_host(&source.path, top.trim());
        if source_control::run_git(&top, &["rev-parse", "--verify", "-q", "HEAD"]).is_err() {
            return Err("Make a first commit before starting a task in a worktree".into());
        }
        // A project in a folder of its repository opens the same folder of the worktree.
        let prefix = source_control::run_git(&source.path, &["rev-parse", "--show-prefix"])?;
        let short = &new_id()[..8];
        let folder = Path::new(ssh::here(&top))
            .file_name()
            .map_or_else(|| "worktree".into(), |n| n.to_string_lossy().into_owned());
        let dir = Self::worktrees_for(&top)?;
        let root = ssh::join(&dir, &format!("{folder}-{short}"));
        let branch = format!("patchwork/{short}");
        source_control::run_git(
            &top,
            &["worktree", "add", "-b", &branch, ssh::here(&root), "HEAD"],
        )?;
        let mut project = make_project(&ssh::join(&root, prefix.trim()))?;
        project.name = format!("{} · {short}", source.name);
        self.register_project(project, true)
    }

    /// The folder of the worktree `create_worktree` made for the project at `path`, when it is
    /// one and may be removed: refused while a task runs in it or it has changes that aren't
    /// committed. None for any other project.
    fn removable_worktree(&self, id: &str, path: &str) -> Result<Option<String>, Error> {
        let root = if let Some(at) = ssh::split(path) {
            // Only asked of the host when the path could be one of its worktrees, so removing a
            // project on a host that is down still works.
            if !at.path.contains("/patchwork/worktrees/") {
                return Ok(None);
            }
            let Ok(dir) = Self::worktrees_for(path) else {
                return Ok(None);
            };
            let Some(first) = path
                .strip_prefix(&format!("{dir}/"))
                .and_then(|rel| rel.split('/').next())
            else {
                return Ok(None);
            };
            let root = ssh::join(&dir, first);
            // Gone already: git drops its record of the worktree by itself.
            if ssh::fs::canonical_dir(at.host, ssh::here(&root)).is_err() {
                return Ok(None);
            }
            root
        } else {
            let Some(root) = self.worktree_top_of(Path::new(path)) else {
                return Ok(None);
            };
            // Gone already: git drops its record of the worktree by itself.
            if !root.exists() {
                return Ok(None);
            }
            root.to_string_lossy().into_owned()
        };
        let running = self
            .store
            .chats()
            .into_iter()
            .any(|t| t.project_id.as_deref() == Some(id) && self.runner.is_running(&t.id));
        if running {
            return Err("Stop this worktree's running tasks before removing it".into());
        }
        if !source_control::run_git(&root, &["status", "--porcelain"])?
            .trim()
            .is_empty()
        {
            return Err(
                "This worktree has changes that aren't committed. Commit or discard them before removing it"
                    .into(),
            );
        }
        let ignored = crate::worktree::ignored_only_here(&root)?;
        if !ignored.is_empty() {
            let shown: Vec<&str> = ignored.iter().take(5).map(String::as_str).collect();
            let more = ignored.len().saturating_sub(shown.len());
            let more = if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            };
            return Err(format!(
                "This worktree has files git ignores that the project doesn't have: {}{more}. Move them out before removing it",
                shown.join(", ")
            )
            .into());
        }
        Ok(Some(root))
    }

    /// Removes the worktree at `root`. Its branch stays with its commits.
    fn remove_worktree(root: &str) -> Result<(), Error> {
        // Run from the folder it is in, which stays when it goes.
        let dir = match ssh::split(root) {
            Some(_) => Self::worktrees_for(root)?,
            None => Path::new(root)
                .parent()
                .ok_or("the worktree has no folder above it")?
                .to_string_lossy()
                .into_owned(),
        };
        let common = source_control::run_git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let git_dir = format!("--git-dir={}", common.trim());
        source_control::run_git(&dir, &[&git_dir, "worktree", "remove", ssh::here(root)])?;
        Ok(())
    }

    /// The folder `create_worktree` puts worktrees of the repository at `root` in, made if it
    /// isn't there: the repository's own `.patchwork` folder here, and Patchwork's folder under
    /// the home folder of an SSH host.
    fn worktrees_for(root: &str) -> Result<String, Error> {
        let Some(at) = ssh::split(root) else {
            let dir = crate::worktree::make_home(root)?;
            return Ok(dir.to_string_lossy().into_owned());
        };
        let dir = ssh::op::make_dirs(at.host, ".local/share/patchwork/worktrees")?;
        Ok(ssh::root(at.host, &dir))
    }

    /// Removes the project, with its worktree if Patchwork made one.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when its worktree cannot be removed, or when
    /// the database cannot save the removal.
    pub fn remove_project(&self, id: &str) -> Result<(), Error> {
        let path = self.project_root(id)?;
        let worktree = self.removable_worktree(id, &path)?;
        // Each chat goes as one deleted by itself does: its agent stopped, and what it kept
        // outside the database removed, while the project's folder is still there to find it in.
        let chats: Vec<String> = self.store.with_chats(|chats| {
            chats
                .iter()
                .filter(|t| t.project_id.as_deref() == Some(id))
                .map(|t| t.id.clone())
                .collect()
        });
        for chat in &chats {
            self.forget_chat(chat);
            self.keep_reply_costs(chat, 0);
        }
        if let Some(root) = worktree {
            Self::remove_worktree(&root)?;
        }
        // The chats before the project: a failure between the two leaves an empty project, not
        // chats that belong to none.
        self.store.delete_project_chats(id)?;
        self.update_settings(|s| {
            s.projects.retain(|p| p.id != id);
            if s.active_project_id.as_deref() == Some(id) {
                s.active_project_id = s.projects.first().map(|p| p.id.clone());
            }
        })?;
        self.store.clear_state(&crate::chats::project_key(id))
    }

    /// Files of a project that a mention can refer to.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown or its files cannot be listed.
    pub fn list_files(&self, project_id: &str) -> Result<Vec<String>, Error> {
        source_control::list_files(&self.project_root(project_id)?)
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
        self.add_folder(project_id, clean_name(name)?, None)
    }

    /// Creates the folder `name` that sorting tasks by provider puts `agent`'s tasks in.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is empty or invalid, the project is unknown, or the database
    /// cannot save the folder.
    pub fn create_sort_folder(
        &self,
        project_id: &str,
        name: &str,
        agent: AgentKind,
    ) -> Result<Folder, Error> {
        self.add_folder(project_id, clean_name(name)?, Some(agent))
    }

    fn add_folder(
        &self,
        project_id: &str,
        name: String,
        sorted_by: Option<AgentKind>,
    ) -> Result<Folder, Error> {
        self.project_root(project_id)?;
        let folder = Folder {
            id: new_id(),
            project_id: project_id.into(),
            name,
            sorted_by,
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

/// Watches `folder`, polling it when it is on an SSH host.
fn start_watcher(
    folder: &str,
    on_burst: impl FnMut(Vec<watch::Change>) + Send + 'static,
) -> Result<watch::ProjectWatcher, Error> {
    if ssh::is_remote(folder) {
        watch::ProjectWatcher::poll(folder, on_burst)
    } else {
        watch::ProjectWatcher::start(&PathBuf::from(folder), on_burst)
    }
}
