//! Project folders and the files in them: creating, resolving, and reading. All functions are
//! blocking.

use crate::Error;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::git::git;
use crate::types::{Project, new_id};

/// Largest file `read_file` returns.
const MAX_READ_BYTES: u64 = 2 * 1024 * 1024;

pub fn make_project(path: &str) -> Result<Project, Error> {
    let canonical = std::fs::canonicalize(path).map_err(|e| format!("Cannot open {path}: {e}"))?;
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

pub fn create_project_dir(parent: &str, name: &str) -> Result<Project, Error> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err("Project name must be a single folder name".into());
    }
    let parent = std::fs::canonicalize(parent).map_err(|e| format!("Cannot open {parent}: {e}"))?;
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
pub fn resolve_inside(root: &str, rel: &str) -> Result<PathBuf, Error> {
    let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let path = std::fs::canonicalize(root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    if path.starts_with(&root) {
        Ok(path)
    } else {
        Err(Error::OutsideProject)
    }
}

/// The paths of `rels` that no longer exist in the project. A broken link still exists.
pub fn missing_files(root: &str, rels: &[String]) -> Vec<String> {
    rels.iter()
        .filter(|rel| std::fs::symlink_metadata(Path::new(root).join(rel)).is_err())
        .cloned()
        .collect()
}

pub fn read_file(root: &str, rel: &str) -> Result<String, Error> {
    let path = resolve_inside(root, rel)?;
    if !path.is_file() {
        return Err(format!("{rel} is not a file").into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .and_then(|f| f.take(MAX_READ_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_READ_BYTES {
        return Err("File is larger than 2 MB".into());
    }
    if bytes.contains(&0) {
        return Err("Binary file".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
