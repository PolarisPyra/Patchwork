//! Paths on the user's machine: the home folder, resolving a path the user or an agent typed, and
//! writing a private file.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::path::{Component, Path, PathBuf};

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

/// `path` with the links in it followed, as far as it exists. A path a link leads out of its
/// folder is then seen where it really is.
#[must_use]
pub fn real(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut existing = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(existing) {
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
pub(crate) fn write_private(path: &Path, text: &str) -> Result<(), Error> {
    use std::io::Write;
    let dir = path.parent().ok_or("No config folder")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp).map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| (e.to_string()).into())
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
