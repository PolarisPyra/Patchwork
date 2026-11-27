//! Making a task's worktree on its first turn, giving a fork its own, and bringing back a
//! parked one.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use crate::system::{new_id, ssh};
use crate::chats::{Role, Chat};
use crate::source_control::{self};
use crate::{Core, Error};
use super::condition::{Condition, repaired_condition};
use super::home::{is_home, make_home};
use super::include::copy_included;
use super::git::{
    NOT_OURS, canonical, checked_out_at, git, identity, in_worktree, main_top, off_branch,
};
use super::state::{Worktree, brief_key, failed_key, key};

/// The commit a new worktree of the repository at `top` starts from: its branch's upstream when
/// that is ahead of HEAD alone, as it is after a fetch the branch hasn't pulled, else HEAD. It is
/// the upstream's commit rather than its name, so the new branch doesn't track it.
fn start_point(top: &str) -> String {
    git(top, &["rev-parse", "--verify", "-q", "@{upstream}"])
        .map(|sha| sha.trim().to_owned())
        .ok()
        .filter(|sha| git(top, &["merge-base", "--is-ancestor", "HEAD", sha]).is_ok())
        .unwrap_or_else(|| "HEAD".to_owned())
}

/// Takes back what a failed `worktree add` of folder `dir` on `branch` left in the repository at
/// `top`: the folder, git's record of it, and the branch. Each step goes ahead when the one
/// before fails, since the add may have stopped anywhere.
fn undo_add(top: &str, dir: &str, branch: &str) {
    let _ = git(top, &["worktree", "remove", "--force", "--", dir]);
    if Path::new(dir).exists()
        && let Err(e) = std::fs::remove_dir_all(dir)
    {
        log::warn!(error:% = e; "could not remove a task worktree that failed to be made: {{error}}");
    }
    let _ = git(top, &["worktree", "prune"]);
    let _ = git(top, &["branch", "-D", "--", branch]);
}

/// The Cargo build folder of the worktree whose top folder is `top`: one per worktree, by its
/// folder's name, so builds in worktrees side by side don't wait on each other's lock.
pub(super) fn build_dir(top: &Path) -> Option<PathBuf> {
    let name = top.file_name()?;
    Some(
        crate::system::dirs()
            .cache
            .join("worktree-builds")
            .join(name),
    )
}

impl Core {
    /// The folder a turn of `chat`, in the project at `path`, runs in. A subagent works where
    /// the task that started it does. A task's first turn makes its worktree when its project
    /// is a git repository on this computer, unless the project edits in place, the turn only
    /// plans, or the task is a chat app's own chat; if making it fails, that turn runs in
    /// `path` and the task keeps why.
    ///
    /// # Errors
    ///
    /// Returns an error when the task has a worktree that can't be used or made again, so the
    /// turn doesn't start rather than edit the project's own folder.
    pub(crate) fn turn_root(
        &self,
        chat: &Chat,
        project_id: &str,
        path: &str,
    ) -> Result<String, Error> {
        let owner = chat
            .parent
            .as_ref()
            .map_or(chat.id.as_str(), |p| p.chat_id.as_str());
        // A task that has a worktree never works in the project's folder instead: its work is on
        // its branch, and its agent would carry on editing the user's own checkout.
        if let Some(record) = self.worktree_record(owner) {
            let root = self.ready_worktree(owner, &record)?;
            if self.store.state(&failed_key(owner)).is_some()
                && let Err(e) = self.store.clear_state(&failed_key(owner))
            {
                log::warn!(error:% = e; "could not forget why a task had no worktree: {{error}}");
            }
            return Ok(root);
        }
        let first_turn =
            chat.session_id.is_none() && !chat.messages.iter().any(|m| m.role == Role::Assistant);
        if chat.parent.is_some()
            || !first_turn
            || ssh::is_remote(path)
            || self.edits_in_place(project_id)
            || self.is_worktree_project(path)
            || self.is_dispatcher(&chat.id)
        {
            return Ok(path.to_owned());
        }
        match self.add_worktree(&chat.id, path) {
            Ok(Some(worktree)) => {
                // An earlier first turn's failure no longer says where the task works.
                if self.store.state(&failed_key(&chat.id)).is_some()
                    && let Err(e) = self.store.clear_state(&failed_key(&chat.id))
                {
                    log::warn!(error:% = e; "could not forget why a task had no worktree: {{error}}");
                }
                Ok(worktree.root)
            }
            Ok(None) => Ok(path.to_owned()),
            Err(e) => {
                log::warn!(error:% = e; "could not make the task's worktree, so it works in the project: {{error}}");
                // Kept, so the task shows it edits the project's own folder.
                if let Err(e) = self.store.set_state(&failed_key(&chat.id), &e.to_string()) {
                    log::warn!(error:% = e; "could not keep why a task has no worktree: {{error}}");
                }
                Ok(path.to_owned())
            }
        }
    }

    /// The folder a turn of task `owner`, whose worktree is `record`, works in. A worktree whose
    /// folder is gone is made again from its branch, whether a cleanup rule, someone outside
    /// Patchwork, or git removed it. One whose link to its repository broke, and that `git
    /// worktree repair` can't link again, fails, and so does one Patchwork didn't make.
    fn ready_worktree(&self, owner: &str, record: &Worktree) -> Result<String, Error> {
        let repo = self.project_top(owner)?;
        let made_again = |why: &str| {
            log::warn!(branch:% = record.branch, why:%; "{{why}} the worktree on {{branch}}; making it again from its branch");
            self.unpark_worktree(owner, record).map(|w| w.root)
        };
        match repaired_condition(record, &repo) {
            Condition::Present if self.owns(record) => Ok(record.root.clone()),
            Condition::Present => Err(NOT_OURS.into()),
            Condition::Parked => self.unpark_worktree(owner, record).map(|w| w.root),
            Condition::FolderDeleted => made_again("someone deleted the folder of"),
            Condition::Unregistered => made_again("git removed"),
            Condition::Broken => Err(format!(
                "This task's worktree at {} is no longer linked to its repository. Move what you \
                 need out of it and delete the folder, and the next turn makes it again",
                record.top
            )
            .into()),
        }
    }

    /// `text` with where task `chat_id` works before it, when it works in its own worktree and
    /// `fresh`, the message starting its agent's session, or it is a fork's first message. An
    /// agent that switches branches there leaves the work where Merge doesn't find it.
    pub(crate) fn worktree_brief(&self, chat_id: &str, text: String, fresh: bool) -> String {
        let forked = self.store.state(&brief_key(chat_id)).is_some();
        if forked && let Err(e) = self.store.clear_state(&brief_key(chat_id)) {
            log::warn!(error:% = e; "could not forget a fork's worktree note: {{error}}");
        }
        let Some(worktree) = self.chat_worktree(chat_id).filter(|_| fresh || forked) else {
            return text;
        };
        format!(
            "<worktree>\nYou work in a git worktree of this project, {}, on the branch {}. The \
             user merges that branch into the project from Patchwork. Stay on it: don't check out, \
             switch to or create another branch here, and don't rebase it onto another. Commit \
             here; to push the work to a branch of its own, run \
             `git push -u origin HEAD:<name>`, so Patchwork finds the pushed branch and its pull \
             request.\n\
             </worktree>\n\n{text}",
            worktree.root, worktree.branch
        )
    }

    /// Makes task `chat_id`'s worktree of the repository `path` is in, on a new branch from its
    /// HEAD, and stores it. None when `path` isn't in a repository with a commit.
    fn add_worktree(&self, chat_id: &str, path: &str) -> Result<Option<Worktree>, Error> {
        let Ok(top) = git(path, &["rev-parse", "--show-toplevel"]) else {
            return Ok(None);
        };
        let top = top.trim();
        if git(top, &["rev-parse", "--verify", "-q", "HEAD"]).is_err() {
            return Ok(None);
        }
        let prefix = git(path, &["rev-parse", "--show-prefix"])?;
        let start = start_point(top);
        self.make_worktree(
            chat_id,
            top,
            prefix.trim(),
            &start,
            source_control::git_branch(top),
        )
        .map(Some)
    }

    /// Makes fork `chat_id` a worktree of its own from `source`, its source task's: a new
    /// branch from its HEAD, with what it hadn't committed, merging back into the same branch.
    pub(super) fn fork_worktree(
        &self,
        chat_id: &str,
        source: &Worktree,
    ) -> Result<Worktree, Error> {
        let top = main_top(&source.top).ok_or("The source task's repository is gone")?;
        let start = in_worktree(source, &["rev-parse", "HEAD"])?;
        // What the source hasn't committed, as a commit no branch holds; empty when it has none.
        let mut args = identity(&source.top);
        args.extend(["stash", "create"]);
        let stash = in_worktree(source, &args)?;
        let prefix = Path::new(&source.root)
            .strip_prefix(&source.top)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let worktree =
            self.make_worktree(chat_id, &top, &prefix, start.trim(), source.base.clone())?;
        if !stash.trim().is_empty()
            && let Err(e) = in_worktree(&worktree, &["stash", "apply", "-q", stash.trim()])
        {
            log::warn!(error:% = e; "could not bring a task's uncommitted changes into its fork's worktree: {{error}}");
        }
        Ok(worktree)
    }

    /// Makes task `chat_id`'s worktree of the repository at `top` on a new branch from
    /// `start`, working in its folder `prefix`, and stores it. `base` is the branch merging goes
    /// back into.
    fn make_worktree(
        &self,
        chat_id: &str,
        top: &str,
        prefix: &str,
        start: &str,
        base: Option<String>,
    ) -> Result<Worktree, Error> {
        let short = &new_id()[..8];
        let name: String = Path::new(top)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .collect();
        let name = name.trim_start_matches(['-', '.']);
        let name = if name.is_empty() { "worktree" } else { name };
        let dir = make_home(top)?.join(format!("{name}-{short}"));
        let dir = dir.to_string_lossy().into_owned();
        let branch = format!("patchwork/{short}");
        let worktree = Worktree {
            root: Path::new(&dir)
                .join(prefix)
                .to_string_lossy()
                .trim_end_matches('/')
                .to_owned(),
            top: dir.clone(),
            branch: branch.clone(),
            base,
            parked: false,
        };
        let made = git(top, &["worktree", "add", "-b", &branch, "--", &dir, start])
            .and_then(|_| serde_json::to_string(&worktree).map_err(|e| e.to_string().into()))
            .and_then(|stored| self.store.set_state(&key(chat_id), &stored));
        if let Err(e) = made {
            undo_add(top, &dir, &branch);
            return Err(e);
        }
        log::info!(branch:% = worktree.branch; "made a worktree for a task on {{branch}}");
        self.prepare_worktree(top, &worktree);
        Ok(worktree)
    }

    /// Gives `worktree`, just checked out from the repository at `top`, what git doesn't: the
    /// ignored files `.worktreeinclude` names, its submodules, and a start on its build.
    fn prepare_worktree(&self, top: &str, worktree: &Worktree) {
        copy_included(top, &worktree.top);
        let mut submodules = source_control::unattended_ssh(top);
        submodules.extend(["submodule", "update", "--init", "--recursive"]);
        if Path::new(&worktree.top).join(".gitmodules").is_file()
            && let Err(e) = git(&worktree.top, &submodules)
        {
            log::warn!(error:% = e; "could not check out a task worktree's submodules: {{error}}");
        }
        self.seed_build(top, &worktree.top);
    }

    /// Makes task `chat_id`'s worktree again, at its folder and on its branch, after a cleanup
    /// rule removed its folder. Every task sharing it gets it back.
    fn unpark_worktree(&self, chat_id: &str, parked: &Worktree) -> Result<Worktree, Error> {
        let at = Path::new(&parked.top);
        if !at
            .parent()
            .is_some_and(|dir| is_home(dir) || self.is_old_home(dir))
            || !parked.branch.starts_with("patchwork/")
            || parked.branch.contains("..")
            || at.exists()
        {
            return Err("This task's worktree is not one Patchwork can make again".into());
        }
        let top = self.project_top(chat_id)?;
        if let Err(e) = git(&top, &["worktree", "prune"]) {
            log::warn!(error:% = e; "could not prune a project's worktrees: {{error}}");
        }
        // Git refuses a second checkout of a branch; say where it is rather than pass that on.
        if let Ok(Some(elsewhere)) = checked_out_at(&top, &parked.branch) {
            return Err(format!(
                "{} is checked out in {elsewhere}; switch that worktree to another branch first",
                parked.branch
            )
            .into());
        }
        let full = format!("refs/heads/{}", parked.branch);
        let made = if git(&top, &["rev-parse", "--verify", "-q", &full]).is_ok() {
            git(
                &top,
                &["worktree", "add", "--", &parked.top, &parked.branch],
            )
        } else {
            log::warn!(branch:% = parked.branch; "the branch {{branch}} is gone; starting it again from the project's HEAD");
            let start = start_point(&top);
            git(
                &top,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &parked.branch,
                    "--",
                    &parked.top,
                    &start,
                ],
            )
        };
        if let Err(e) = made {
            if at.exists() {
                let _ = std::fs::remove_dir_all(at);
            }
            let _ = git(&top, &["worktree", "prune"]);
            return Err(e);
        }
        let worktree = Worktree {
            parked: false,
            ..parked.clone()
        };
        self.prepare_worktree(&top, &worktree);
        let stored = serde_json::to_string(&worktree).map_err(|e| e.to_string())?;
        for id in self.sharers(&parked.top) {
            self.store.set_state(&key(&id), &stored)?;
        }
        if self.store.state(&failed_key(chat_id)).is_some() {
            self.store.clear_state(&failed_key(chat_id))?;
        }
        log::info!(branch:% = worktree.branch; "made a task's worktree on {{branch}} again");
        Ok(worktree)
    }

    /// Starts the build folder of the Rust worktree at `worktree_top` as a copy of the project's
    /// at `top`, made in the background, only where the file system shares the copy's blocks
    /// with the original: a new task then doesn't build from cold, and the copy takes no room
    /// until one of them changes. Elsewhere the worktree builds from cold.
    fn seed_build(&self, top: &str, worktree_top: &str) {
        let Some((_, target)) = self.worktree_env(worktree_top).into_iter().next() else {
            return;
        };
        let (from, target) = (Path::new(top).join("target"), PathBuf::from(target));
        if !from.is_dir() || target.exists() {
            return;
        }
        std::thread::spawn(move || {
            let mut seeding = target.clone().into_os_string();
            seeding.push(".seeding");
            let seeding = PathBuf::from(seeding);
            if let Some(parent) = target.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                log::warn!(error:% = e; "could not make the worktree builds folder: {{error}}");
                return;
            }
            let copied = crate::system::host_command("cp")
                .args(["-a", "--reflink=always", "--"])
                .arg(&from)
                .arg(&seeding)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            // A build started meanwhile keeps its own folder.
            if !copied || std::fs::rename(&seeding, &target).is_err() {
                let _ = std::fs::remove_dir_all(&seeding);
            }
        });
    }

    /// Checks, before a commit, merge, reset or removal, that `worktree` is what its record says:
    /// a worktree of task `chat_id`'s project, on its own branch. A record or a repository
    /// changed since then fails here instead of acting on another folder or branch.
    pub(super) fn verify(&self, chat_id: &str, worktree: &Worktree) -> Result<String, Error> {
        let top = self.verify_repository(chat_id, worktree)?;
        if let Some(on) = off_branch(worktree) {
            return Err(format!(
                "This task's worktree is on {on}, not its branch {}. Switch it back first",
                worktree.branch
            )
            .into());
        }
        Ok(top)
    }

    /// The part of [`Self::verify`] that holds on any branch: `worktree` is a worktree of task
    /// `chat_id`'s project, at the folder its record names. Returns the project's top folder.
    pub(super) fn verify_repository(
        &self,
        chat_id: &str,
        worktree: &Worktree,
    ) -> Result<String, Error> {
        let top = self.project_top(chat_id)?;
        let theirs = git(
            &top,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let ours = in_worktree(
            worktree,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let shown = in_worktree(worktree, &["rev-parse", "--show-toplevel"])?;
        if canonical(&theirs)? != canonical(&ours)?
            || canonical(&shown)? != canonical(&worktree.top)?
        {
            return Err("This task's worktree is not the one Patchwork made".into());
        }
        Ok(top)
    }
}
