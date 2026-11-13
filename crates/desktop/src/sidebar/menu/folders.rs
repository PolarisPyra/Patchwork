//! The project buttons that make folders and sort tasks into them by provider, and back.

// Rust guideline compliant 2026-10-09

use super::types::SortUndo;
use super::types::Target;
use std::sync::Arc;
use gpui::{Window, prelude::*};
use patchwork_server::provider::AgentKind;
use patchwork_server::threads::Thread;
use crate::ui::{AGENT_KINDS, agent_label};
use crate::workspace::Workspace;

impl Workspace {
    /// Makes a folder called "New folder" in the project, opens the project, and names it.
    pub fn new_folder(&mut self, project_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let (server, project) = (Arc::clone(&self.server), project_id.to_string());
        let made = {
            let project = project.clone();
            cx.background_spawn(async move { server.create_folder(&project, "New folder") })
        };
        cx.spawn_in(window, async move |this, cx| {
            let made = made.await;
            let _ = this.update_in(cx, |this, window, cx| match made {
                Ok(folder) => {
                    if this.collapsed.remove(&project) {
                        this.save_collapsed(cx);
                    }
                    this.reload(cx);
                    this.start_rename(Target::Folder(folder.id), window, cx);
                }
                Err(e) => log::warn!(error:% = e; "could not make a folder: {{error}}"),
            });
        })
        .detach();
    }

    /// Moves the project's tasks into a folder per provider, made as needed, and remembers where
    /// they were so it can be undone.
    pub fn sort_by_agent(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let tasks: Vec<Arc<Thread>> = self
            .threads
            .iter()
            .filter(|t| {
                t.project_id.as_deref() == Some(project_id)
                    && t.archived_at.is_none()
                    && t.parent.is_none()
            })
            .cloned()
            .collect();
        // Which tasks go to which folder, by its id or by the name of one still to be made. It is
        // worked out here, and the moves are made off the UI thread.
        let mut moves: Vec<(Vec<String>, Result<String, AgentKind>)> = Vec::new();
        for agent in AGENT_KINDS {
            let ids: Vec<String> = tasks
                .iter()
                .filter(|t| t.agent == agent)
                .map(|t| t.id.clone())
                .collect();
            if ids.is_empty() {
                continue;
            }
            let folder = self
                .folders
                .iter()
                .find(|f| f.project_id == project_id && f.sorted_by == Some(agent))
                .map(|f| f.id.clone())
                .ok_or(agent);
            moves.push((ids, folder));
        }
        let project = project_id.to_string();
        let sort = move |server: &patchwork_server::Server| {
            // Every provider is tried, so one that fails does not leave the rest unsorted.
            let mut done = Ok(());
            for (ids, folder) in moves {
                let sorted = match folder {
                    Ok(id) => Ok(id),
                    Err(agent) => server
                        .create_sort_folder(&project, agent_label(agent), agent)
                        .map(|f| f.id),
                }
                .and_then(|folder| server.reorder_threads(&ids, Some(&folder)));
                if let Err(e) = sorted {
                    done = Err(e);
                }
            }
            done
        };
        // A task sorted before keeps the place it had then, not the provider folder it is in now.
        let kept = self.sort_undo.remove(project_id).unwrap_or_default();
        let before: SortUndo = kept
            .iter()
            .cloned()
            .chain(
                tasks
                    .iter()
                    .filter(|t| !kept.iter().any(|(id, _)| id == &t.id))
                    .map(|t| (t.id.clone(), t.folder_id.clone())),
            )
            .collect();
        self.sort_undo.insert(project_id.to_string(), before);
        self.change("sort the tasks", sort, cx);
    }

    /// Puts the tasks in provider folders back where they were before sorting, or at the top of
    /// the project.
    pub fn undo_sort_by_agent(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let agent_folders: Vec<String> = self
            .folders
            .iter()
            .filter(|f| f.project_id == project_id && f.sorted_by.is_some())
            .map(|f| f.id.clone())
            .collect();
        let before = self.sort_undo.remove(project_id).unwrap_or_default();
        let destination = |t: &Thread| -> Option<String> {
            let folder = t.folder_id.clone()?;
            if !agent_folders.contains(&folder) {
                return Some(folder);
            }
            before
                .iter()
                .find(|(id, _)| id == &t.id)
                .and_then(|(_, prior)| prior.clone())
                .filter(|prior| {
                    !agent_folders.contains(prior) && self.folders.iter().any(|f| &f.id == prior)
                })
        };
        let mut lists: Vec<(Option<String>, Vec<&Thread>)> = Vec::new();
        for t in &self.threads {
            if t.project_id.as_deref() != Some(project_id)
                || t.archived_at.is_some()
                || t.parent.is_some()
            {
                continue;
            }
            let to = destination(t);
            match lists.iter_mut().find(|(f, _)| *f == to) {
                Some((_, list)) => list.push(t),
                None => lists.push((to, vec![t])),
            }
        }
        let rank = |id: &str| {
            before
                .iter()
                .position(|(b, _)| b == id)
                .unwrap_or(before.len())
        };
        let mut moves: Vec<(Vec<String>, Option<String>)> = Vec::new();
        for (folder, mut list) in lists {
            if list.iter().all(|t| t.folder_id == folder) {
                continue;
            }
            list.sort_by_key(|t| rank(&t.id));
            moves.push((list.iter().map(|t| t.id.clone()).collect(), folder));
        }
        let undo = move |server: &patchwork_server::Server| {
            // Every step is tried, so one that fails does not leave the rest undone.
            let mut done = Ok(());
            for (ids, folder) in moves {
                if let Err(e) = server.reorder_threads(&ids, folder.as_deref()) {
                    done = Err(e);
                }
            }
            for id in agent_folders {
                if let Err(e) = server.remove_folder(&id) {
                    done = Err(e);
                }
            }
            done
        };
        self.change("undo the sort", undo, cx);
    }
}
