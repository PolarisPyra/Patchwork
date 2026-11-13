//! Project folders and the files in them: creating, resolving, and reading. All functions are
//! blocking.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::git::git;
use crate::projects::Project;
use crate::system::{new_id, remote};

/// Largest file `read_file` returns.
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

pub(crate) fn make_project(path: &str) -> Result<Project, Error> {
    if let Some(at) = remote::split(path) {
        let dir = remote::fs::canonical_dir(at.host, at.path)?;
        let name = dir
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or(at.host)
            .to_string();
        return Ok(Project {
            id: new_id(),
            name,
            path: remote::root(at.host, &dir),
        });
    }
    let canonical =
        crate::system::fs::canonicalize(path).map_err(|e| format!("Cannot open {path}: {e}"))?;
    if !canonical.is_dir() {
        return Err(format!("{path} is not a folder").into());
    }
    let name = canonical
        .file_name()
        .unwrap_or(canonical.as_os_str())
        .to_string_lossy()
        .into_owned();
    Ok(Project {
        id: new_id(),
        name,
        path: canonical.to_string_lossy().into_owned(),
    })
}

pub(crate) fn create_project_dir(parent: &str, name: &str) -> Result<Project, Error> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err("Project name must be a single folder name".into());
    }
    if let Some(at) = remote::split(parent) {
        let dir = remote::fs::create_dir(at.host, at.path, name)?;
        let root = remote::root(at.host, &dir);
        git(&root, &["init", "-b", "main"])?;
        return make_project(&root);
    }
    let parent = crate::system::fs::canonicalize(parent)
        .map_err(|e| format!("Cannot open {parent}: {e}"))?;
    if !parent.is_dir() {
        return Err(format!("{} is not a folder", parent.display()).into());
    }
    let dir = parent.join(name);
    if dir.exists() {
        let empty = dir.is_dir()
            && std::fs::read_dir(&dir)
                .map_err(|e| e.to_string())?
                .next()
                .is_none();
        if !empty {
            return Err(format!("{} already exists and is not empty", dir.display()).into());
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let root = dir.to_string_lossy().into_owned();
    git(&root, &["init", "-b", "main"])?;
    make_project(&root)
}

/// Joins `rel` onto the project root and rejects anything that resolves outside it.
///
/// # Errors
///
/// Returns an error when the root or the joined path does not exist, or the path resolves outside
/// the project.
pub fn resolve_inside(root: &str, rel: &str) -> Result<PathBuf, Error> {
    if let Some(at) = remote::split(root) {
        let path = remote::fs::resolve_inside(at.host, at.path, rel)?;
        return Ok(PathBuf::from(remote::root(at.host, &path)));
    }
    let root = crate::system::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let path =
        crate::system::fs::canonicalize(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    if path.starts_with(&root) {
        Ok(path)
    } else {
        Err(Error::OutsideProject)
    }
}

/// The paths of `rels` that no longer exist in the project. A broken link still exists.
#[must_use]
pub fn missing_files(root: &str, rels: &[String]) -> Vec<String> {
    if let Some(at) = remote::split(root) {
        // A host that cannot be reached says nothing is gone, rather than everything.
        let sizes = remote::fs::sizes(at.host, at.path, rels).unwrap_or_default();
        return rels
            .iter()
            .zip(sizes)
            .filter(|(_, size)| size.is_none())
            .map(|(rel, _)| rel.clone())
            .collect();
    }
    rels.iter()
        .filter(|rel| std::fs::symlink_metadata(Path::new(root).join(rel)).is_err())
        .cloned()
        .collect()
}

/// The text of the project file `rel`.
///
/// # Errors
///
/// Returns an error when the path is outside the project, is not a file, cannot be read, is larger
/// than 2 MB, or is binary.
pub fn read_file(root: &str, rel: &str) -> Result<String, Error> {
    let bytes = if let Some(at) = remote::split(root) {
        remote::fs::read_inside(at.host, at.path, rel, MAX_READ_BYTES)?
    } else {
        let path = resolve_inside(root, rel)?;
        if !path.is_file() {
            return Err(format!("{rel} is not a file").into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .and_then(|f| f.take(MAX_READ_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|e| e.to_string())?;
        bytes
    };
    if bytes.len() as u64 > MAX_READ_BYTES {
        return Err("File is larger than 2 MB".into());
    }
    if bytes.contains(&0) {
        return Err("Binary file".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
