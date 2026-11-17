//! Paths on the user's machine: the home folder, resolving a path the user or an agent typed, and
//! writing a private file.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// `path` resolved against the root, with `.` and `..` folded away.
#[must_use]
pub fn resolve(root: &Path, path: &str) -> PathBuf {
    let path = path.trim();
    let expanded = match path.strip_prefix("~/") {
        Some(rest) => home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    };
    let joined = if expanded.is_absolute() {
        expanded
    } else {
        root.join(expanded)
    };
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                // The system follows a link before it steps up from it, so the step is taken
                // from where the link leads: `link/..` is the folder above the link's target,
                // not the folder the link is in.
                if out.is_symlink()
                    && let Ok(target) = std::fs::canonicalize(&out)
                {
                    out = target;
                }
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// `path` made absolute, with its links followed, written as the user writes it.
///
/// On Windows the system gives a resolved path the `\\?\` prefix, as in `\\?\C:\Users`, which
/// Patchwork never shows or keeps; the path comes back as `C:\Users`, and a network one as
/// `\\server\share`. Elsewhere this is [`std::fs::canonicalize`].
///
/// # Errors
///
/// Returns an error when `path` does not exist or cannot be read.
pub fn canonicalize(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    let resolved = std::fs::canonicalize(path)?;
    Ok(without_verbatim(&resolved).map_or(resolved, PathBuf::from))
}

/// `path` without the `\\?\` prefix Windows gives a resolved path, or `None` when it has none.
#[must_use]
pub fn without_verbatim(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        return Some(format!(r"\\{share}"));
    }
    text.strip_prefix(r"\\?\")
        .filter(|rest| rest.as_bytes().get(1) == Some(&b':'))
        .map(str::to_string)
}

/// `path` with the links in it followed, as far as it exists. A path a link leads out of its
/// folder is then seen where it really is.
#[must_use]
pub fn real(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut existing = path;
    loop {
        if let Ok(real) = canonicalize(existing) {
            return rest.iter().rev().fold(real, |path, part| path.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Whether `path`, resolved against the root, is in the root's folder once links are followed.
#[must_use]
pub fn is_inside(root: &Path, path: &str) -> bool {
    real(&resolve(root, path)).starts_with(real(root))
}

/// Whether `name` can be a file's name as it is: not hidden, and without a path separator or a
/// character Windows refuses.
#[must_use]
pub fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
}

/// Writes `text` through a temporary file, readable only by the user.
///
/// On Windows an antivirus or the search indexer opens a file the moment it is written, and
/// the move over the old file is then refused for a moment. The move is tried a few times, and a
/// file that still cannot be replaced is written where it is instead.
///
/// # Errors
///
/// Returns an error when the file cannot be written.
pub fn write_private(path: &Path, text: &str) -> Result<(), Error> {
    /// Counts the temporary files of this process, so two writers never share one.
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().ok_or("No config folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(
        ".{}-{}.tmp",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp = PathBuf::from(tmp);
    let moved = write_new(&tmp, text).and_then(|()| replace(&tmp, path));
    let Err(error) = moved else { return Ok(()) };
    let _ = std::fs::remove_file(&tmp);
    // Only a refusal: after a full disk, writing in place would cut the old file short.
    if error.kind() == ErrorKind::PermissionDenied && write_new(path, text).is_ok() {
        return Ok(());
    }
    Err(format!("Could not write {}: {error}", path.display()).into())
}

/// Writes `text` to `path` and waits for it to reach the disk. A new file is the user's alone.
fn write_new(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// Moves `tmp` over `path`, trying again for a third of a second while Windows refuses it.
fn replace(tmp: &Path, path: &Path) -> std::io::Result<()> {
    let tries = if cfg!(windows) { 6 } else { 1 };
    let mut wait = Duration::from_millis(10);
    for _ in 1..tries {
        match std::fs::rename(tmp, path) {
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                std::thread::sleep(wait);
                wait *= 2;
            }
            done => return done,
        }
    }
    std::fs::rename(tmp, path)
}

/// The JSON file at `path`, or the default when there is none. A file that does not parse is kept
/// as `<name>.bad` before the default is used, so the next save cannot lose what it held.
#[must_use]
pub fn read_json_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(text) = std::fs::read_to_string(path) else {
        return T::default();
    };
    match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(e) => {
            let mut bad = path.as_os_str().to_owned();
            bad.push(".bad");
            let kept = std::fs::copy(path, &bad).is_ok();
            log::warn!(
                path:% = path.display(),
                error:% = e,
                kept = if kept {
                    "; a copy was kept beside it as .bad"
                } else {
                    ""
                };
                "{{path}} is not valid JSON ({{error}}){{kept}}"
            );
            T::default()
        }
    }
}
