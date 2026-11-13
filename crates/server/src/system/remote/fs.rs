//! Files and folders on an SSH host, each question one call to Patchwork's helper there.
//!
//! These mirror what the workspace asks of a local folder through `std::fs`, for a project whose
//! root is remote. Paths are the host's own; the callers split them off the root first.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::HashMap;

use base64::Engine as _;

use super::op::{Request, call};

/// The folder `path` on `host` with its links resolved.
///
/// # Errors
///
/// Returns an error when `path` is not a folder there.
pub fn canonical_dir(host: &str, path: &str) -> Result<String, Error> {
    call(host, &Request::CanonicalDir { path: path.into() })
}

/// `rel` under the folder `dir` on `host`, links resolved, refused outside the folder.
///
/// # Errors
///
/// Returns an error when the path does not exist or resolves outside `dir`.
pub fn resolve_inside(host: &str, dir: &str, rel: &str) -> Result<String, Error> {
    call(
        host,
        &Request::Resolve {
            dir: dir.into(),
            rel: rel.into(),
        },
    )
}

/// Up to `max` bytes of the file `rel` under `dir` on `host`, plus one more when it is longer.
///
/// # Errors
///
/// Returns an error when the path is outside `dir`, is not a file, or cannot be read.
pub fn read_inside(host: &str, dir: &str, rel: &str, max: u64) -> Result<Vec<u8>, Error> {
    let text: String = call(
        host,
        &Request::Read {
            dir: dir.into(),
            rel: rel.into(),
            max,
        },
    )?;
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map_err(|e| e.to_string().into())
}

/// Deletes the file `rel` under `dir` on `host`.
///
/// # Errors
///
/// Returns an error when the path is outside `dir` or cannot be deleted.
pub fn remove_inside(host: &str, dir: &str, rel: &str) -> Result<(), Error> {
    call(
        host,
        &Request::Remove {
            dir: dir.into(),
            rel: rel.into(),
        },
    )
}

/// For each of `rels` under `dir` on `host`: its size when it is a file inside `dir`, `Some(0)`
/// for anything else that is there, and None when it is gone. A broken link is there.
///
/// # Errors
///
/// Returns an error when the host cannot be reached.
pub fn sizes(host: &str, dir: &str, rels: &[String]) -> Result<Vec<Option<u64>>, Error> {
    if rels.is_empty() {
        return Ok(Vec::new());
    }
    call(
        host,
        &Request::Sizes {
            dir: dir.into(),
            rels: rels.to_vec(),
        },
    )
}

/// The number of lines in each of `rels` under `dir` on `host`, as the local count has them: a
/// last line without a newline counts, and a binary file counts 0.
///
/// # Errors
///
/// Returns an error when the host cannot be reached.
pub fn line_counts(host: &str, dir: &str, rels: &[String]) -> Result<HashMap<String, u32>, Error> {
    if rels.is_empty() {
        return Ok(HashMap::new());
    }
    let counts: Vec<u32> = call(
        host,
        &Request::LineCounts {
            dir: dir.into(),
            rels: rels.to_vec(),
        },
    )?;
    Ok(rels.iter().cloned().zip(counts).collect())
}

/// Makes the folder `name` in `parent` on `host`, which must be new or empty, and returns its
/// path with links resolved.
///
/// # Errors
///
/// Returns an error when `parent` is not a folder, or `name` is there and not an empty folder.
pub fn create_dir(host: &str, parent: &str, name: &str) -> Result<String, Error> {
    call(
        host,
        &Request::CreateDir {
            parent: parent.into(),
            name: name.into(),
        },
    )
}
