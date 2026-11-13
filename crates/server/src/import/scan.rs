//! Finding the conversations other apps saved, and importing the ones asked for.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::Path;

use crate::import::ilog;
use crate::import::{ImportSource, ImportScan};
use crate::provider::AgentKind;
use crate::{import, workspace};
use crate::Server;

impl Server {
    /// Sessions `source` saved on disk that can be imported: ones whose folder
    /// still exists, that Patchwork did not run as a chat, and that are not already
    /// threads: an agent session in `known`, or an imported transcript in `imported`.
    /// Returns (found, importable).
    fn importable(
        &self,
        source: ImportSource,
        known: &[(AgentKind, String)],
        imported: &[String],
    ) -> (u32, u32, Vec<import::Session>) {
        let default_agent = self
            .store
            .settings()
            .agents
            .first()
            .copied()
            .unwrap_or(AgentKind::Claude);
        let all = import::sessions(source, default_agent);
        let subagents = all.iter().filter(|s| s.subagent).count() as u32;
        // Patchwork's own chats are not the user's imports, so they are not counted.
        let all: Vec<_> = all
            .into_iter()
            .filter(|s| {
                if s.subagent {
                    ilog!("{source:?}: skipped {}: sub-agent run", s.session_id);
                    return false;
                }
                if Path::new(&s.cwd).starts_with(&self.chats_dir) {
                    ilog!("{source:?}: skipped {}: a Patchwork chat", s.session_id);
                    return false;
                }
                true
            })
            .collect();
        let found = all.len() as u32;
        let new: Vec<_> = all
            .into_iter()
            .filter(|s| {
                let already = if s.imported {
                    imported.contains(&s.session_id)
                } else {
                    known
                        .iter()
                        .any(|(a, id)| *a == s.agent && *id == s.session_id)
                };
                if already {
                    ilog!("{source:?}: skipped {}: already in Patchwork", s.session_id);
                }
                !already
            })
            .collect();
        ilog!(
            "{source:?}: {found} found, {} new, {subagents} sub-agent runs",
            new.len()
        );
        (found, subagents, new)
    }

    pub fn scan_imports(&self, source: ImportSource) -> ImportScan {
        ilog!("{source:?}: scan started");
        let (found, subagents, new) = self.importable(
            source,
            &self.store.session_keys(),
            &self.store.imported_sources(),
        );
        ImportScan {
            kind: source,
            found,
            new: new.len() as u32,
            subagents,
        }
    }

    /// Imports new sessions as threads in the project for their folder, adding
    /// the project if needed. A conversation found by more than one source (a
    /// T3 Code thread is also in the agent's own files) is imported once.
    /// Returns how many threads were added. A session whose folder cannot be added as a project
    /// is left out.
    ///
    /// # Errors
    ///
    /// Returns an error when the imported threads cannot be saved.
    pub fn import_threads(&self, sources: &[ImportSource]) -> Result<u32, Error> {
        ilog!("import started: {sources:?}");
        let mut known = self.store.session_keys();
        let mut imported = self.store.imported_sources();
        let mut threads = Vec::new();
        for &source in sources {
            for session in self.importable(source, &known, &imported).2 {
                if session.imported {
                    if imported.contains(&session.session_id) {
                        ilog!(
                            "{source:?}: skipped {}: imported from another source",
                            session.session_id
                        );
                        continue;
                    }
                    imported.push(session.session_id.clone());
                } else {
                    if known
                        .iter()
                        .any(|(a, id)| *a == session.agent && *id == session.session_id)
                    {
                        ilog!(
                            "{source:?}: skipped {}: imported from another source",
                            session.session_id
                        );
                        continue;
                    }
                    known.push((session.agent, session.session_id.clone()));
                }
                let project_id = if import::is_chat_folder(Path::new(&session.cwd)) {
                    ilog!(
                        "{source:?}: importing {} as a chat ({})",
                        session.session_id,
                        session.cwd
                    );
                    None
                } else {
                    ilog!(
                        "{source:?}: importing {} into project {}",
                        session.session_id,
                        session.cwd
                    );
                    // A folder that cannot be a project costs its own sessions, not the rest.
                    match workspace::make_project(&session.cwd)
                        .and_then(|p| self.register_project(p, false))
                    {
                        Ok(project) => Some(project.id),
                        Err(e) => {
                            ilog!("{source:?}: couldn't add project {}: {e}", session.cwd);
                            continue;
                        }
                    }
                };
                threads.push(import::to_thread(session, project_id));
            }
        }
        let count = threads.len() as u32;
        self.store
            .insert_threads(threads)
            .inspect_err(|e| ilog!("import failed saving threads: {e}"))?;
        ilog!("import finished: {count} threads added");
        Ok(count)
    }
}
