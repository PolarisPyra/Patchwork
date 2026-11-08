//! One change of a file's diff at a time: staging it, taking it back out of the index, or
//! discarding it from the file, and which of a file's changes are staged.

use std::io::Write;
use std::process::Stdio;

use super::git::{accept_file, git, has_head, is_untracked, revert_file};
use crate::Error;
use crate::threads::TurnTrees;
use crate::workspace::HunkAction;

/// A file's part of a diff: its header lines, and each change from its `@@` line on.
#[derive(Default)]
struct Section {
    header: String,
    hunks: Vec<String>,
}

fn sections(diff: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for line in diff.split_inclusive('\n') {
        if line.starts_with("diff --git ") || out.is_empty() {
            out.push(Section::default());
        }
        let Some(section) = out.last_mut() else {
            continue;
        };
        if line.starts_with("@@") {
            section.hunks.push(line.to_string());
        } else if let Some(hunk) = section.hunks.last_mut() {
            hunk.push_str(line);
        } else {
            section.header.push_str(line);
        }
    }
    out
}

/// Each change of `diff` as a patch of its own, with its file's header.
fn patches(diff: &str) -> Vec<(String, String)> {
    sections(diff)
        .into_iter()
        .flat_map(|section| {
            let header = section.header;
            section.hunks.into_iter().map(move |hunk| {
                let mut patch = format!("{header}{hunk}");
                if !patch.ends_with('\n') {
                    patch.push('\n');
                }
                (hunk, patch)
            })
        })
        .collect()
}

/// The file's diff as the diff view shows it, the reply's when `turn` is given and else against
/// HEAD, but with paths from the repository's top, where a patch of it applies.
fn file_diff(root: &str, rel: &str, turn: Option<&TurnTrees>) -> Result<String, Error> {
    let diff = |args: &[&str]| {
        let mut all = vec!["diff", "--no-color", "--no-ext-diff"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--", rel]);
        git(root, &all)
    };
    match turn {
        Some(turn) => diff(&[&turn.before, &turn.after]),
        None if has_head(root) => diff(&["HEAD"]),
        // No commits yet: the staged part, then the rest, as the diff view gets it.
        None => Ok(diff(&["--cached"])? + &diff(&[])?),
    }
}

/// Runs `git apply <args>` on `patch` in `top`, the repository's top folder.
fn apply(top: &str, args: &[&str], patch: &str) -> Result<(), Error> {
    let mut child = crate::system::host_command("git")
        .arg("apply")
        .args(args)
        .current_dir(top)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Git(format!("Failed to run git: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(patch.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(Error::Git(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

fn toplevel(root: &str) -> Result<String, Error> {
    Ok(git(root, &["rev-parse", "--show-toplevel"])?
        .trim()
        .to_string())
}

/// Stages, unstages, or discards the change of the file's diff whose text, from its `@@` line
/// on, is `hunk`. Refused when the file changed so that the change is no longer in its diff.
pub fn apply_hunk(
    root: &str,
    rel: &str,
    turn: Option<&TurnTrees>,
    hunk: &str,
    action: HunkAction,
) -> Result<(), Error> {
    // A new file's diff is one change, and git has no index entry to apply it against.
    if turn.is_none() && is_untracked(root, rel) {
        return match action {
            HunkAction::Stage => accept_file(root, rel),
            HunkAction::Discard => revert_file(root, rel),
            HunkAction::Unstage => Err(Error::Message("This change isn't staged".into())),
        };
    }
    let wanted = hunk.trim_end_matches('\n');
    let patch = patches(&file_diff(root, rel, turn)?)
        .into_iter()
        .find(|(hunk, _)| hunk.trim_end_matches('\n') == wanted)
        .map(|(_, patch)| patch)
        .ok_or_else(|| Error::Message("The file changed since. Look at its diff again.".into()))?;
    let top = toplevel(root)?;
    let refused = |what: &'static str| move |_| Error::Message(what.to_string());
    match action {
        HunkAction::Stage => apply(&top, &["--cached"], &patch).map_err(refused(
            "The staged copy of the file no longer fits this change",
        )),
        HunkAction::Unstage => apply(&top, &["--cached", "--reverse"], &patch)
            .map_err(refused("This change isn't staged")),
        HunkAction::Discard => {
            apply(&top, &["--reverse"], &patch).map_err(refused(
                "The file was edited around this change since, so it can't be taken out cleanly",
            ))?;
            // A staged copy goes too, so the change is gone rather than hidden behind the index.
            // A reply's change is left there, since the index may hold it as committed.
            if turn.is_none() {
                let _ = apply(&top, &["--cached", "--reverse"], &patch);
            }
            Ok(())
        }
    }
}

/// Which changes of the file's diff are staged, in its order. Empty when the file has nothing
/// staged, which also leaves out a reply's changes that were committed since.
pub fn staged_hunks(root: &str, rel: &str, turn: Option<&TurnTrees>) -> Result<Vec<bool>, Error> {
    if is_untracked(root, rel) || git(root, &["diff", "--cached", "--quiet", "--", rel]).is_ok() {
        return Ok(Vec::new());
    }
    let top = toplevel(root)?;
    Ok(patches(&file_diff(root, rel, turn)?)
        .iter()
        .map(|(_, patch)| apply(&top, &["--cached", "--reverse", "--check"], patch).is_ok())
        .collect())
}
