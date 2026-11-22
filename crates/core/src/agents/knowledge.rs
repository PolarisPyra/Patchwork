//! The files an agent knows: copies kept in its own folder, which its runs are told of and may
//! read but not change.

// Rust guideline compliant 2026-10-09

use std::path::{Component, Path, PathBuf};

use super::types::KnowledgeFile;
use crate::Core;
use crate::Error;
use crate::permissions::Target;

/// The largest file an agent can be given, in bytes.
const FILE_MAX: u64 = 10 * 1024 * 1024;

/// The most files an agent's folder lists; any more are left out.
const FILES_MAX: usize = 200;

/// The folder in `data_dir` that holds one folder per agent.
pub(crate) fn agents_folder(data_dir: &Path) -> PathBuf {
    data_dir.join("agents")
}

/// Whether `id` has the shape of an id Patchwork makes, so it can name a folder safely.
fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// Whether `name` is a plain file name: one part, no folder, not hidden, on one line.
fn safe_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!(parts.next(), Some(Component::Normal(_)))
        && parts.next().is_none()
        && name.len() <= 255
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
}

impl Core {
    /// Agent `id`'s own folder. None for an id Patchwork could not have made.
    pub(crate) fn agent_dir(&self, id: &str) -> Option<PathBuf> {
        safe_id(id).then(|| self.agents_dir.join(id))
    }

    /// Opens where agent `id`'s next run works in the editor: `project_id` when it is one of the
    /// agent's projects on this computer, else the agent's own folder.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent does not exist, its folder cannot be made, or no editor
    /// opens.
    pub fn open_agent_in_editor(&self, id: &str, project_id: Option<&str>) -> Result<(), Error> {
        let agent = self.agent(id).ok_or("The agent no longer exists")?;
        let project = project_id
            .filter(|p| agent.projects.iter().any(|b| b == p))
            .and_then(|p| {
                self.store
                    .settings()
                    .projects
                    .into_iter()
                    .find(|project| project.id == p && project.host().is_none())
            });
        let root = if let Some(project) = project {
            project.path
        } else {
            let dir = self.agent_dir(id).ok_or("The agent no longer exists")?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            dir.to_string_lossy().into_owned()
        };
        crate::system::open_in_editor(&root, "", None)
    }

    /// The folder that holds the files agent `id` knows.
    pub(crate) fn knowledge_dir(&self, id: &str) -> Option<PathBuf> {
        self.agent_dir(id).map(|dir| dir.join("knowledge"))
    }

    /// The folder of files known by the agent behind chat `chat_id`, which its runs and their
    /// subagents read without asking on every provider. None for a chat that is not an agent's run.
    pub(crate) fn run_knowledge_dir(&self, chat_id: &str) -> Option<PathBuf> {
        self.knowledge_dir(&self.run_agent_id(chat_id)?)
    }

    /// The agent behind chat `chat_id`: its own, or for a subagent its owner's. None for a
    /// chat that is not an agent's run.
    pub(crate) fn run_agent_id(&self, chat_id: &str) -> Option<String> {
        let chat = self.store.chat(chat_id)?;
        let owner = match &chat.parent {
            Some(parent) => self.store.chat(&parent.chat_id)?,
            None => chat,
        };
        owner.agent_id
    }

    /// Whether the agent behind chat `chat_id` blocks `target`, which its runs and their
    /// subagents are refused on every provider.
    pub(crate) fn agent_refuses(&self, chat_id: &str, root: &Path, target: &Target) -> bool {
        let Some(agent) = self.run_agent_id(chat_id).and_then(|id| self.agent(&id)) else {
            return false;
        };
        crate::permissions::refuses(&agent.blocked.rules(), root, target)
    }

    /// The files agent `id` knows, by name. Links and folders put there by anything else are left
    /// out.
    #[must_use]
    pub fn knowledge_files(&self, id: &str) -> Vec<KnowledgeFile> {
        let Some(dir) = self.knowledge_dir(id) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut files: Vec<KnowledgeFile> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                // `DirEntry::metadata` does not follow links.
                let meta = entry.metadata().ok()?;
                (meta.is_file() && safe_name(&name)).then_some(KnowledgeFile {
                    name,
                    size: meta.len(),
                })
            })
            .take(FILES_MAX)
            .collect();
        files.sort_by(|a, b| a.name.cmp(&b.name));
        files
    }

    /// Copies the file at `source` into agent `id`'s folder, replacing one of the same name.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent does not exist, `source` is not a plain file or is too
    /// large, its name is not a plain file name, or the copy fails.
    pub fn add_knowledge(&self, id: &str, source: &Path) -> Result<KnowledgeFile, Error> {
        if self.agent(id).is_none() {
            return Err("The agent no longer exists".into());
        }
        let dir = self.knowledge_dir(id).ok_or("The agent no longer exists")?;
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| safe_name(n))
            .ok_or("That file's name can't be used")?
            .to_string();
        // Opened without following a link, then checked and copied through the same handle,
        // so the path can't be swapped for a link or a bigger file between the check and the
        // copy.
        let mut open = std::fs::OpenOptions::new();
        open.read(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::custom_flags(&mut open, libc::O_NOFOLLOW);
        let not_file = || Error::from("Only a file can be added, not a folder or a link");
        let file = match open.open(source) {
            Ok(file) => file,
            Err(e) if e.raw_os_error() == Some(libc::ELOOP) => return Err(not_file()),
            Err(e) => return Err(e.to_string().into()),
        };
        let meta = file.metadata().map_err(|e| e.to_string())?;
        if !meta.is_file() {
            return Err(not_file());
        }
        if meta.len() > FILE_MAX {
            return Err(format!("{name} is larger than {} MB", FILE_MAX / 1024 / 1024).into());
        }
        create_private_dir(&dir)?;
        // Copied under a name of its own, then renamed into place, which replaces a link of the
        // same name rather than writing through it.
        let incoming = dir.join(format!(".incoming-{}", crate::system::new_id()));
        let copied = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&incoming)
            .and_then(|mut out| std::io::copy(&mut std::io::Read::take(file, FILE_MAX), &mut out))
            .and_then(|_| std::fs::rename(&incoming, dir.join(&name)));
        if let Err(e) = copied {
            let _ = std::fs::remove_file(&incoming);
            return Err(format!("Could not add {name}: {e}").into());
        }
        Ok(KnowledgeFile {
            name,
            size: meta.len(),
        })
    }

    /// Removes the file `name` from agent `id`'s folder.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is not a plain file name or the file cannot be removed.
    pub fn remove_knowledge(&self, id: &str, name: &str) -> Result<(), Error> {
        if !safe_name(name) {
            return Err("That file's name can't be used".into());
        }
        let dir = self.knowledge_dir(id).ok_or("The agent no longer exists")?;
        match std::fs::remove_file(dir.join(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("Could not remove {name}: {e}").into())
            }
            _ => Ok(()),
        }
    }
}

/// Makes `dir` and the folders above it, readable only by this user on Unix.
fn create_private_dir(dir: &Path) -> Result<(), Error> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(dir)
        .map_err(|e| format!("Could not make the agent's folder: {e}").into())
}
