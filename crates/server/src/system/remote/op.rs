//! Questions Patchwork asks its helper on an SSH host about the folders there.
//!
//! Each [`Request`] goes to the helper as JSON on its stdin, and the [`Reply`] comes back on its
//! stdout, so nothing variable passes through the host's shell, whichever one it is: `sh`, fish,
//! `cmd` or PowerShell.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::command::Spec;

/// The argument that answers the request on stdin.
pub const OP_ARG: &str = "--remote-op";

/// A question for the helper. Paths are the host's own; a relative one is under its home folder.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub(super) enum Request {
    /// The helper's version.
    Ping,
    /// The folder `path` with its links resolved.
    CanonicalDir { path: String },
    /// The folder `path`, made with its parents when it is not there, with its links resolved.
    MakeDirs { path: String },
    /// A new or empty folder `name` in `parent`, made, with its links resolved.
    CreateDir { parent: String, name: String },
    /// `rel` under `dir` with its links resolved; refused outside `dir`.
    Resolve { dir: String, rel: String },
    /// Up to `max` bytes and one more of the file `rel` under `dir`, as base64.
    Read { dir: String, rel: String, max: u64 },
    /// Deletes the file `rel` under `dir`.
    Remove { dir: String, rel: String },
    /// Each of `rels` under `dir`: its size as a file inside `dir`, 0 for anything else that is
    /// there, and nothing when it is gone.
    Sizes { dir: String, rels: Vec<String> },
    /// The number of lines in each of `rels` under `dir`; 0 for a binary file.
    LineCounts { dir: String, rels: Vec<String> },
    /// The sub-folders of `path`, or of the home folder.
    Browse { path: Option<String> },
    /// Keeps `spec` in a private file for a later `--remote-exec`, and names it.
    Stage { spec: Spec },
    /// What is in `dir`: which of `names` are files and which are folders, which of `programs`
    /// the user's shell finds, and up to `max` bytes of each of `read`.
    Survey {
        dir: String,
        names: Vec<String>,
        programs: Vec<String>,
        read: Vec<String>,
        max: u64,
    },
    /// The modified time and size of each file git sees changed or new in `dir`, and of
    /// `.git/HEAD` and `.git/index`, with the commit HEAD names under `.git/HEAD@`.
    Poll { dir: String },
    /// The tree `git add -A` would give `dir`, written without touching its index.
    Snapshot { dir: String },
}

/// The helper's answer to a [`Request`].
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Reply {
    Done(serde_json::Value),
    /// The path resolves outside the folder it was given under.
    Outside,
    Failed(String),
}

/// What [`Request::Survey`] found.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Survey {
    pub files: Vec<String>,
    pub dirs: Vec<String>,
    pub programs: Vec<String>,
    /// The text of each file asked for, in order; None when it is not there.
    pub texts: Vec<Option<String>>,
}

/// Asks the helper on `host`, which is copied there first if it isn't yet.
///
/// # Errors
///
/// Returns [`Error::OutsideProject`] for a path outside its folder, and an error with the
/// helper's reason when it could not answer or the host cannot be reached.
pub(super) fn call<T: DeserializeOwned>(host: &str, request: &Request) -> Result<T, Error> {
    let body = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    let out = super::upload::run_helper(host, &[OP_ARG], Some(&body))?;
    let reply: Reply = serde_json::from_slice(&out.stdout).map_err(|_unreadable| {
        let said = String::from_utf8_lossy(&out.stderr);
        let reason = said
            .lines()
            .map(str::trim)
            .rfind(|l| !l.is_empty() && !l.starts_with("**"))
            .unwrap_or("no answer");
        format!("Patchwork's helper on {host} failed: {reason}")
    })?;
    match reply {
        Reply::Done(value) => serde_json::from_value(value).map_err(|e| e.to_string().into()),
        Reply::Outside => Err(Error::OutsideProject),
        Reply::Failed(reason) => Err(reason.into()),
    }
}

/// [`Request::Poll`] for `dir` on `host`.
///
/// # Errors
///
/// Returns an error when the host cannot be reached.
pub fn poll(host: &str, dir: &str) -> Result<BTreeMap<String, String>, Error> {
    call(host, &Request::Poll { dir: dir.into() })
}

/// [`Request::Survey`] for `dir` on `host`.
///
/// # Errors
///
/// Returns an error when the host cannot be reached.
pub fn survey(
    host: &str,
    dir: &str,
    names: &[&str],
    programs: &[&str],
    read: &[&str],
    max: u64,
) -> Result<Survey, Error> {
    let owned = |list: &[&str]| list.iter().map(|s| (*s).to_string()).collect();
    call(
        host,
        &Request::Survey {
            dir: dir.into(),
            names: owned(names),
            programs: owned(programs),
            read: owned(read),
            max,
        },
    )
}

/// [`Request::Snapshot`] for `dir` on `host`: the tree id.
///
/// # Errors
///
/// Returns an error when the host cannot be reached or git fails there.
pub fn snapshot(host: &str, dir: &str) -> Result<String, Error> {
    call(host, &Request::Snapshot { dir: dir.into() })
}

/// The folder `path` on `host`, made when it is not there, with its links resolved. A relative
/// path is under the home folder there.
///
/// # Errors
///
/// Returns an error when the host cannot be reached or the folder cannot be made.
pub fn make_dirs(host: &str, path: &str) -> Result<String, Error> {
    call(host, &Request::MakeDirs { path: path.into() })
}

/// The sub-folders of `path` on `host`, or of its home folder.
///
/// # Errors
///
/// Returns an error when the host cannot be reached or the folder cannot be read.
pub fn browse(host: &str, path: Option<&str>) -> Result<crate::system::browse::DirListing, Error> {
    call(
        host,
        &Request::Browse {
            path: path.map(str::to_string),
        },
    )
}
