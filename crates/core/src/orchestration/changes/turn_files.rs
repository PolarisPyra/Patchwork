//! The files a turn changed, and each one's diff, read from the trees kept at its start and end.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::fmt::Write as _;
use serde::Serialize;
use super::checkpoint::{files_on_disk, git, run};
use crate::threads::TurnTrees;

/// Where a file a turn changed stands now.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnFileState {
    /// Still differs from the last commit.
    Uncommitted,
    /// Matches the last commit.
    Committed,
    /// Back to how it was before the turn.
    Reverted,
    /// Deleted by the turn, or since.
    Deleted,
    /// The turn left it as it was.
    Unchanged,
}

/// A file a turn changed: lines added and removed by the turn, and where it stands now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnFile {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    pub state: TurnFileState,
}

/// Fields of `git diff-tree -z` output for the turn's changes to `paths`.
fn turn_diff_fields(
    root: &str,
    turn: &TurnTrees,
    format: &str,
    paths: &[String],
) -> Result<Vec<String>, Error> {
    let mut args = vec![
        "diff-tree",
        "-r",
        "-z",
        "--relative",
        "--no-renames",
        format,
        &turn.before,
        &turn.after,
        "--",
    ];
    args.extend(paths.iter().map(String::as_str));
    let out = run(root, None, &args, None)?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(String::from)
        .collect())
}

/// What the turn did to each of `paths` (project-relative), and where each file stands now.
///
/// # Errors
///
/// Returns an error when a `git` command that compares the turn's snapshots fails.
pub fn turn_files(root: &str, turn: &TurnTrees, paths: &[String]) -> Result<Vec<TurnFile>, Error> {
    // `A\0path\0` pairs, and `added\tdeleted\tpath\0` lines (`-` for binary files).
    let status: Vec<String> = turn_diff_fields(root, turn, "--name-status", paths)?;
    let status: std::collections::HashMap<&str, &str> = status
        .chunks(2)
        .filter_map(|p| Some((p.get(1)?.as_str(), p[0].as_str())))
        .collect();
    let mut counts = std::collections::HashMap::new();
    for line in turn_diff_fields(root, turn, "--numstat", paths)? {
        let mut parts = line.splitn(3, '\t');
        let (Some(add), Some(del), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        counts.insert(
            path.to_string(),
            (add.parse().unwrap_or(0), del.parse().unwrap_or(0)),
        );
    }
    // The blobs to compare: before the turn, at the last commit, and on disk now.
    let on_disk = files_on_disk(root, paths);
    let present: Vec<&String> = paths
        .iter()
        .zip(&on_disk)
        .filter(|(p, there)| **there && status.get(p.as_str()).is_some_and(|s| *s != "D"))
        .map(|(p, _)| p)
        .collect();
    // Both read paths from the repository's top, which the project may sit below.
    let prefix = git(root, &["rev-parse", "--show-prefix"])?;
    let queries: String = present
        .iter()
        .flat_map(|p| {
            [
                format!("{}:{prefix}{p}\n", turn.before),
                format!("HEAD:{prefix}{p}\n"),
            ]
        })
        .collect();
    let known = run(
        root,
        None,
        &["cat-file", "--batch-check=%(objectname)"],
        Some(queries.as_bytes()),
    )?;
    let known = String::from_utf8_lossy(&known.stdout).into_owned();
    let mut known = known.lines();
    let names: String = present.iter().fold(String::new(), |mut names, p| {
        // Relative to the project, where git runs, so the same names work on an SSH host.
        let _ = writeln!(names, "{p}");
        names
    });
    let now = run(
        root,
        None,
        &["hash-object", "--stdin-paths"],
        Some(names.as_bytes()),
    )?;
    let now = String::from_utf8_lossy(&now.stdout).into_owned();
    let mut blobs = std::collections::HashMap::new();
    for (path, now) in present.iter().zip(now.lines()) {
        let (before, head) = (
            known.next().unwrap_or_default(),
            known.next().unwrap_or_default(),
        );
        blobs.insert(
            path.as_str(),
            (before.to_string(), head.to_string(), now.to_string()),
        );
    }
    Ok(paths
        .iter()
        .map(|path| {
            let (additions, deletions) = counts.get(path).copied().unwrap_or((0, 0));
            let state = match (status.get(path.as_str()), blobs.get(path.as_str())) {
                (None, _) => TurnFileState::Unchanged,
                (_, None) => TurnFileState::Deleted,
                (_, Some((before, _, now))) if before == now => TurnFileState::Reverted,
                (_, Some((_, head, now))) if head == now => TurnFileState::Committed,
                _ => TurnFileState::Uncommitted,
            };
            TurnFile {
                path: path.clone(),
                additions,
                deletions,
                state,
            }
        })
        .collect())
}

/// The turn's diff of one project-relative file, in `git diff` form.
///
/// # Errors
///
/// Returns an error when `git diff` between the turn's snapshots fails.
pub fn turn_file_diff(root: &str, turn: &TurnTrees, path: &str) -> Result<String, Error> {
    git(
        root,
        &[
            "diff",
            "--no-color",
            "--relative",
            &turn.before,
            &turn.after,
            "--",
            path,
        ],
    )
}
