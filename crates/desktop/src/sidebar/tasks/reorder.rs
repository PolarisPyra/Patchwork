//! Dragging tasks to reorder them in the tree: a task dropped on another goes before it, in that
//! task's folder or at its project's top level, and one dropped on a folder goes last in it.
//! Pinned and favorite tasks keep their own places and neither move nor take a drop.

// Rust guideline compliant 2026-10-09

use gpui::Context;
use patchwork_core::chats::Chat;

use crate::workspace::Workspace;

/// Whether `c` sits in its project's reorderable list: listed, unpinned, and not a favorite, a
/// subagent's chat or an agent's run.
fn movable(c: &Chat) -> bool {
    c.project_id.is_some()
        && c.archived_at.is_none()
        && c.parent.is_none()
        && c.agent_id.is_none()
        && !c.pinned
        && !c.favorite
}

impl Workspace {
    /// Whether task `id` may be dropped among `onto`'s rows: both are movable tasks of the same
    /// project, and they are not the same task.
    pub(crate) fn can_drop_task(&self, id: &str, onto: &Chat) -> bool {
        id != onto.id
            && movable(onto)
            && self
                .chats
                .iter()
                .find(|c| c.id == id)
                .is_some_and(|c| movable(c) && c.project_id == onto.project_id)
    }

    /// Moves task `id` to just before task `onto`, into `onto`'s folder.
    pub(crate) fn drop_task_before(&mut self, id: &str, onto: &str, cx: &mut Context<Self>) {
        self.task_drop_before = None;
        let Some(target) = self.chats.iter().find(|c| c.id == onto).cloned() else {
            return;
        };
        if !self.can_drop_task(id, &target) {
            return;
        }
        let folder = target.folder_id.clone();
        let mut ids = self.listed_in(target.project_id.as_deref(), folder.as_deref(), id);
        let at = ids.iter().position(|c| c == onto).unwrap_or(ids.len());
        ids.insert(at, id.to_owned());
        self.save_task_order(&ids, folder.as_deref(), cx);
    }

    /// Moves task `id` to the end of folder `folder` of project `project`.
    pub(crate) fn drop_task_in_folder(
        &mut self,
        id: &str,
        project: &str,
        folder: &str,
        cx: &mut Context<Self>,
    ) {
        self.task_drop_before = None;
        let fits = self
            .chats
            .iter()
            .find(|c| c.id == id)
            .is_some_and(|c| movable(c) && c.project_id.as_deref() == Some(project));
        if !fits {
            return;
        }
        let mut ids = self.listed_in(Some(project), Some(folder), id);
        ids.push(id.to_owned());
        self.save_task_order(&ids, Some(folder), cx);
    }

    /// The movable tasks of `project` in `folder`, in their order, leaving out `moving`.
    fn listed_in(&self, project: Option<&str>, folder: Option<&str>, moving: &str) -> Vec<String> {
        self.chats
            .iter()
            .filter(|c| {
                movable(c)
                    && c.id != moving
                    && c.project_id.as_deref() == project
                    && c.folder_id.as_deref() == folder
            })
            .map(|c| c.id.clone())
            .collect()
    }

    /// Saves `ids` as the order of `folder`, or of the top level when None, and redraws the tree.
    fn save_task_order(&mut self, ids: &[String], folder: Option<&str>, cx: &mut Context<Self>) {
        if let Err(e) = self.core.reorder_chats(ids, folder) {
            log::warn!(error:% = e; "could not reorder the tasks: {{error}}");
            self.error = Some(e.to_string());
        }
        self.reload(cx);
    }
}
