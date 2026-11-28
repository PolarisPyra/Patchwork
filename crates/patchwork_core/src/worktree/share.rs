//! The tasks that share one worktree: a task and its forks, and whether any of them is working.

// Rust guideline compliant 2026-10-09

use crate::{Core, Error};
use super::state::{Worktree, brief_key, key};

impl Core {
    /// The tasks whose records name the worktree at `top`, by id: a task and the forks that
    /// share its worktree.
    pub(super) fn sharers(&self, top: &str) -> Vec<String> {
        self.store
            .state_keys("worktree:")
            .into_iter()
            .filter_map(|record| {
                let stored = self.store.state(&record)?;
                let stored: Worktree = serde_json::from_str(&stored).ok()?;
                let id = record.strip_prefix("worktree:")?;
                (stored.top == top).then(|| id.to_owned())
            })
            .collect()
    }

    /// The task the sidebar marks the worktree at `top` on: of the tasks still there that share
    /// it, the one made first, among those not archived while there are any.
    pub(super) fn mark_owner(&self, top: &str) -> Option<String> {
        self.sharers(top)
            .into_iter()
            .filter_map(|id| self.store.chat(&id))
            .min_by_key(|t| (t.archived_at.is_some(), t.created_at))
            .map(|t| t.id)
    }

    /// Gives fork `to` the worktree of task `from`, so it works in the same folder, on the same
    /// branch. Nothing happens when `from` has no worktree.
    fn share_worktree(&self, from: &str, to: &str) -> Result<(), Error> {
        let Some(stored) = self.store.state(&key(from)) else {
            return Ok(());
        };
        self.store.set_state(&key(to), &stored)
    }

    /// Gives fork `fork_id` of task `source_id`, in project `project_id`, the worktree it works
    /// in: the source's, shared, or with `new`, one of its own from the source's. `new` None
    /// follows the project's setting. A worktree of its own that can't be made leaves it sharing.
    /// Its first message then says where it works.
    pub(crate) fn give_fork_worktree(
        &self,
        source_id: &str,
        fork_id: &str,
        project_id: Option<&str>,
        new: Option<bool>,
    ) -> Result<(), Error> {
        let Some(source) = self.worktree_record(source_id) else {
            return Ok(());
        };
        let new = new.unwrap_or_else(|| {
            project_id.is_some() && self.store.settings().worktrees.fork_into_new
        });
        let own = new
            && !source.parked
            && self.owns(&source)
            && match self.fork_worktree(fork_id, &source) {
                Ok(_) => true,
                Err(e) => {
                    log::warn!(error:% = e; "could not make a fork its own worktree, so it shares its source's: {{error}}");
                    false
                }
            };
        if !own {
            self.share_worktree(source_id, fork_id)?;
        }
        self.store.set_state(&brief_key(fork_id), "1")
    }

    /// Whether task `chat_id` or one of its subagents, which work in its worktree, is running,
    /// or still has a background command or task it started running.
    /// A fork sharing the worktree counts as the task.
    pub(in crate::worktree) fn busy(&self, chat_id: &str) -> bool {
        self.worktree_chats(chat_id).iter().any(|t| {
            self.runner.is_running(&t.id)
                || self.runner.is_starting(&t.id)
                || t.messages
                    .iter()
                    .any(|m| m.background.iter().any(|task| task.status.is_live()))
        })
    }

    /// Whether an agent is up in task `chat_id`'s worktree, replying or idle: the task's, a
    /// sharing fork's, or one of their subagents'. A message to an idle agent goes on in the
    /// folder it started in, so the folder stays while it is up.
    pub(super) fn agent_up(&self, chat_id: &str) -> bool {
        self.worktree_chats(chat_id)
            .iter()
            .any(|t| self.runner.has_agent(&t.id))
    }

    /// The tasks working in task `chat_id`'s worktree: it, the forks sharing the worktree, and
    /// their subagents.
    fn worktree_chats(&self, chat_id: &str) -> Vec<crate::chats::Chat> {
        let sharing = self.sharing(chat_id);
        self.store
            .chats()
            .into_iter()
            .filter(|t| {
                sharing.contains(&t.id)
                    || t.parent
                        .as_ref()
                        .is_some_and(|p| sharing.contains(&p.chat_id))
            })
            .collect()
    }

    /// The tasks working in task `chat_id`'s worktree, it among them: it and the forks sharing
    /// the worktree. Only it when it has none.
    pub(super) fn sharing(&self, chat_id: &str) -> Vec<String> {
        let mut ids = self
            .worktree_record(chat_id)
            .map(|w| self.sharers(&w.top))
            .unwrap_or_default();
        if !ids.iter().any(|id| id == chat_id) {
            ids.push(chat_id.to_owned());
        }
        ids
    }

    /// Whether a task other than `chat_id`, still there, shares the worktree at `top`.
    pub(super) fn shared_with_others(&self, top: &str, chat_id: &str) -> bool {
        self.sharers(top)
            .iter()
            .any(|id| id != chat_id && self.store.chat(id).is_some())
    }
}
