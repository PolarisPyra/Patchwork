//! The sidebar's folders and the order of the chats in them, as the database keeps them.

// Rust guideline compliant 2026-10-09

use crate::Error;
use rusqlite::params;
use super::store::{Store, enc, err};
use crate::projects::Folder;
use crate::chats::Chat;

impl Store {
    pub fn folders(&self) -> Vec<Folder> {
        self.lock().folders.clone()
    }

    /// Stores a new folder.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot insert the folder.
    pub fn create_folder(&self, folder: Folder) -> Result<(), Error> {
        let mut inner = self.lock();
        inner
            .conn
            .execute(
                "INSERT INTO folders (id, project_id, name, sorted_by, position)
                 VALUES (?1, ?2, ?3, ?4,
                         (SELECT COALESCE(MAX(position), -1) + 1 FROM folders WHERE project_id = ?2))",
                params![
                    folder.id,
                    folder.project_id,
                    folder.name,
                    folder.sorted_by.as_ref().map(enc)
                ],
            )
            .map_err(err)?;
        inner.folders.push(folder);
        Ok(())
    }

    /// Renames the folder `id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot update the folder.
    pub fn rename_folder(&self, id: &str, name: &str) -> Result<(), Error> {
        let mut inner = self.lock();
        inner
            .conn
            .execute(
                "UPDATE folders SET name = ?1 WHERE id = ?2",
                params![name, id],
            )
            .map_err(err)?;
        if let Some(folder) = inner.folders.iter_mut().find(|f| f.id == id) {
            folder.name = name.into();
        }
        Ok(())
    }

    /// Removes a folder. Its tasks move to the project's top level.
    pub(crate) fn delete_folder(&self, id: &str) -> Result<(), Error> {
        let mut inner = self.lock();
        let tx = inner.conn.transaction().map_err(err)?;
        // Back to the default order at the top level: newest first, as for a new task.
        tx.execute(
            "UPDATE chats SET folder_id = NULL, position = -created_at WHERE folder_id = ?1",
            [id],
        )
        .map_err(err)?;
        tx.execute("DELETE FROM folders WHERE id = ?1", [id])
            .map_err(err)?;
        tx.commit().map_err(err)?;
        inner.folders.retain(|f| f.id != id);
        for t in &mut inner.chats {
            if t.folder_id.as_deref() == Some(id) {
                t.folder_id = None;
                t.position = -(t.created_at as i64);
            }
        }
        Ok(())
    }

    /// Puts `ids` in `folder_id` (None: the top level) in that order. The ids are the
    /// whole destination list, so a chat that came from elsewhere joins it here.
    /// Archived, pinned and favorite chats are not in `ids`; they keep their slots in the
    /// list, so restoring, unpinning or unfavoriting one puts it back where it was.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot save the new order.
    pub fn reorder_chats(&self, ids: &[String], folder_id: Option<&str>) -> Result<(), Error> {
        let mut inner = self.lock();
        // Listed elsewhere in the sidebar, so not among the rows that were dragged.
        let held = |t: &Chat| t.archived_at.is_some() || t.pinned || t.favorite;
        let ids = {
            let project_id = ids
                .first()
                .and_then(|id| inner.chats.iter().find(|t| t.id == *id))
                .map(|t| t.project_id.clone());
            let mut list: Vec<&Chat> = inner
                .chats
                .iter()
                .filter(|t| {
                    Some(&t.project_id) == project_id.as_ref()
                        && t.folder_id.as_deref() == folder_id
                        && (held(t) || ids.contains(&t.id))
                })
                .map(|t| &t.chat)
                .collect();
            list.sort_by_key(|t| t.position);
            let mut moved = ids.iter();
            let mut order: Vec<String> = list
                .iter()
                .filter_map(|t| {
                    if held(t) && !ids.contains(&t.id) {
                        Some(t.id.clone())
                    } else {
                        moved.next().cloned()
                    }
                })
                .collect();
            order.extend(moved.cloned());
            order
        };
        let tx = inner.conn.transaction().map_err(err)?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute(
                "UPDATE chats SET position = ?1, folder_id = ?2 WHERE id = ?3",
                params![i as i64, folder_id, id],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)?;
        for t in &mut inner.chats {
            if let Some(i) = ids.iter().position(|id| *id == t.id) {
                t.position = i as i64;
                t.folder_id = folder_id.map(String::from);
            }
        }
        Ok(())
    }
}
