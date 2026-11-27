//! Copying the files a command names to an SSH host.
//!
//! An agent on a host runs with the same launch files it would get here (settings, skills,
//! hooks, the relay file they read), so the files a command names under Patchwork's own
//! folders are copied to a mirror on the host, and the command names the copies.

// Rust guideline compliant 2026-10-09

use crate::Locked;
use crate::Error;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::sync::{LazyLock, Mutex, OnceLock};

use super::command::send;
use super::upload::{Installed, base, helper};

/// The most bytes of files copied for one command, so a folder named by mistake cannot fill
/// the host.
const MOST: u64 = 256 * 1024 * 1024;

/// A hash of what each file copied to a host held, by host and local path, so unchanged files
/// are not sent again.
static SENT: LazyLock<Mutex<HashMap<(String, String), u64>>> = LazyLock::new(Mutex::default);

/// Forgets the files sent to `host`, so they are sent again.
pub(super) fn forget(host: &str) {
    SENT.locked().retain(|(sent_to, _), _| sent_to != host);
}

/// Patchwork's folders whose files a command may name, and whether a folder named under each
/// is copied whole. The data folder holds the database, so only its single files go.
fn roots() -> &'static [(String, bool)] {
    static ROOTS: OnceLock<Vec<(String, bool)>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        let dirs = crate::dirs();
        [
            (&dirs.config, true),
            (&dirs.cache, true),
            (&dirs.state, true),
            (&dirs.data, false),
        ]
        .into_iter()
        .filter(|(p, _)| p.is_absolute())
        .map(|(p, whole)| (p.to_string_lossy().into_owned(), whole))
        .collect()
    })
}

/// Whether `c` ends a path written inside an argument, a JSON value or a TOML string.
fn ends_path(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | ',' | ';' | ')' | ']' | '}' | '>' | '`' | '\0'
        )
}

/// A local path as it is named under the mirror folder: `/a/b` stays, and `C:\a\b` becomes
/// `/C/a/b`.
fn mirror_name(path: &str) -> String {
    let path = path.replace('\\', "/");
    if super::location::has_drive(&path) {
        format!("/{}{}", &path[..1], &path[2..])
    } else {
        path
    }
}

/// The files a command names under Patchwork's folders, gathered for one copy to the host.
pub(super) struct Mirror {
    host: String,
    installed: Installed,
    /// The folder the files go in on the host; a local path `/a/b` becomes `<folder>/a/b`.
    folder: String,
    exe: Option<String>,
    /// What to send: name under the mirror, contents as the host gets them, and its [`Kind`].
    files: Vec<(String, Vec<u8>, Kind)>,
    seen: HashSet<String>,
    size: u64,
}

impl Mirror {
    /// A mirror of files for `host`, with the helper there already.
    pub(super) fn new(host: &str) -> Result<Self, Error> {
        Ok(Self {
            host: host.to_string(),
            installed: helper(host)?,
            folder: format!("{}/mirror", base(host)?),
            exe: std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
            files: Vec::new(),
            seen: HashSet::new(),
            size: 0,
        })
    }

    /// The helper on the host.
    pub(super) fn helper(&self) -> &Installed {
        &self.installed
    }

    /// `text` with Patchwork's binary and files named as on the host, and the files it names
    /// gathered to send.
    pub(super) fn rewrite(&mut self, text: &str) -> Result<String, Error> {
        let mut text = text.to_string();
        if let Some(exe) = &self.exe {
            text = text.replace(exe.as_str(), &self.installed.path);
            // A Windows path written in JSON doubles its backslashes.
            if exe.contains('\\') {
                text = text.replace(&exe.replace('\\', r"\\"), &self.installed.path);
            }
        }
        for (root, whole) in roots() {
            text = self.rewrite_root(&text, root, *whole, false)?;
            if root.contains('\\') {
                text = self.rewrite_root(&text, &root.replace('\\', r"\\"), *whole, true)?;
            }
        }
        Ok(text)
    }

    /// `text` with the paths under the local folder `root` named as their mirrors; `escaped`
    /// when `root` is written with doubled backslashes.
    fn rewrite_root(
        &mut self,
        text: &str,
        root: &str,
        whole: bool,
        escaped: bool,
    ) -> Result<String, Error> {
        if !text.contains(root) {
            return Ok(text.to_string());
        }
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find(root) {
            let tail = &rest[at..];
            // Looked for after the folder itself, whose own name may hold a space.
            let len = tail[root.len()..]
                .find(ends_path)
                .map_or(tail.len(), |end| root.len() + end);
            let path = &tail[..len];
            // A longer name that only starts like the folder, such as `patchwork-old`, is
            // something else.
            let after = &path[root.len()..];
            let inside = after.is_empty() || after.starts_with(['/', '\\']);
            out.push_str(&rest[..at]);
            if inside {
                let local = if escaped {
                    path.replace(r"\\", "\\")
                } else {
                    path.to_string()
                };
                self.gather(Path::new(&local), whole)?;
                out.push_str(&self.folder);
                out.push_str(&mirror_name(&local));
            } else {
                out.push_str(path);
            }
            rest = &tail[len..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Adds `path` to what is sent: a file, or a folder's files when `whole`.
    fn gather(&mut self, path: &Path, whole: bool) -> Result<(), Error> {
        let key = path.to_string_lossy().into_owned();
        if !self.seen.insert(key.clone()) {
            return Ok(());
        }
        let Ok(meta) = std::fs::metadata(path) else {
            // Named but not there yet, such as a file the command writes; its folder is made.
            if let Some(parent) = path.parent() {
                let parent = mirror_name(&parent.to_string_lossy());
                self.files.push((parent, Vec::new(), Kind::Folder));
            }
            return Ok(());
        };
        if meta.is_dir() {
            if !whole {
                // A folder of the data folder, such as where conversations are kept, is made
                // empty: what is in it stays on this machine.
                self.files
                    .push((mirror_name(&key), Vec::new(), Kind::Folder));
                return Ok(());
            }
            let entries = std::fs::read_dir(path).map_err(|e| format!("Cannot read {key}: {e}"))?;
            for entry in entries.flatten() {
                self.gather(&entry.path(), true)?;
            }
            return Ok(());
        }
        self.size += meta.len();
        if self.size > MOST {
            return Err(format!(
                "{key} and the files before it are too large to copy to {}",
                self.host
            )
            .into());
        }
        let bytes = std::fs::read(path).map_err(|e| format!("Cannot read {key}: {e}"))?;
        // A launch file may itself name Patchwork's binary or other files, such as a settings
        // file whose hooks run Patchwork.
        let bytes = match String::from_utf8(bytes) {
            Ok(text) => self.rewrite(&text)?.into_bytes(),
            Err(e) => e.into_bytes(),
        };
        let kind = if runs(&meta) {
            Kind::Program
        } else {
            Kind::File
        };
        self.files.push((mirror_name(&key), bytes, kind));
        Ok(())
    }

    /// Sends the gathered files that changed since they were last sent to the host.
    pub(super) fn send(self) -> Result<(), Error> {
        let mut stream = Vec::new();
        let mut sent = Vec::new();
        {
            let guard = SENT.locked();
            for (path, bytes, kind) in &self.files {
                let mut hasher = DefaultHasher::new();
                (bytes, *kind as u8).hash(&mut hasher);
                let hash = hasher.finish();
                let key = (self.host.clone(), path.clone());
                if guard.get(&key) == Some(&hash) {
                    continue;
                }
                frame(&mut stream, path, bytes, *kind);
                sent.push((key, hash));
            }
        }
        if sent.is_empty() {
            return Ok(());
        }
        let line = self.installed.line(&[UNPACK_ARG]);
        let out = send(&self.host, &line, Some(&stream))?;
        if !out.status.success() {
            return Err(format!(
                "Could not copy Patchwork's files to {}: {}",
                self.host,
                String::from_utf8_lossy(&out.stderr).trim()
            )
            .into());
        }
        SENT.locked().extend(sent);
        Ok(())
    }
}
/// `text` with the paths it names under Patchwork's folders as they are on `host`.
///
/// Patchwork's binary and those files are copied there first. This is for what a command reads
/// on stdin, which [`super::supervised`] does not see.
///
/// # Errors
///
/// Returns an error when the host cannot be reached or the files cannot be copied to it.
pub fn mirrored(host: &str, text: &str) -> Result<String, Error> {
    let mut mirror = Mirror::new(host)?;
    let text = mirror.rewrite(text)?;
    mirror.send()?;
    Ok(text)
}

/// Whether the file `meta` describes is a program.
fn runs(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

/// Appends one entry to the stream the helper unpacks: the path's length and the path, its
/// [`Kind`], the contents' length and the contents.
fn frame(stream: &mut Vec<u8>, path: &str, bytes: &[u8], kind: Kind) {
    stream.extend_from_slice(&(path.len() as u64).to_be_bytes());
    stream.extend_from_slice(path.as_bytes());
    stream.push(kind as u8);
    stream.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    stream.extend_from_slice(bytes);
}

/// The argument that unpacks the files on stdin into the helper's mirror folder.
pub const UNPACK_ARG: &str = "--remote-unpack";

/// What an entry in the stream the helper unpacks is, as the byte after its path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    File = 0,
    Program = 1,
    Folder = 2,
}
