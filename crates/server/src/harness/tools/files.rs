//! The file tools: reading, writing and editing files, listing folders, and finding files and text.

use std::path::{Path, PathBuf};
use super::shown;

/// Lines `read_file` returns when no limit is given.
pub(super) const READ_LINES: usize = 2000;

const MAX_LINE: usize = 2000;

const MAX_MATCHES: usize = 200;

pub(super) fn read_file(path: &Path, offset: usize, limit: usize) -> Result<String, String> {
    if path.is_dir() {
        return Err(format!("{} is a directory; use list_dir", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return Err(format!("{} is a binary file", path.display()));
    }
    let text = String::from_utf8_lossy(&bytes);
    if text.is_empty() {
        return Ok("(empty file)".into());
    }
    let total = text.lines().count();
    let mut out = String::new();
    // Whole lines up to the longest result, so a long read ends on a line to read on from.
    let mut shown_end = offset - 1;
    for (i, line) in text.lines().enumerate().skip(offset - 1).take(limit) {
        let line: String = line.chars().take(MAX_LINE).collect();
        let numbered = format!("{:>6}\t{line}\n", i + 1);
        // Room is left for the note that follows.
        if out.len() + numbered.len() > super::MAX_RESULT - 100 && !out.is_empty() {
            break;
        }
        out.push_str(&numbered);
        shown_end = i + 1;
    }
    if shown_end < total {
        out.push_str(&format!(
            "\n({total} lines in all; read on with offset {})",
            shown_end + 1
        ));
    }
    Ok(out)
}

pub(super) fn write_file(path: &Path, content: &str, root: &Path) -> Result<String, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let existed = path.exists();
    std::fs::write(path, content).map_err(|e| format!("{}: {e}", path.display()))?;
    let verb = if existed { "Wrote" } else { "Created" };
    Ok(format!(
        "{verb} {} ({} lines)",
        shown(path, root),
        content.lines().count()
    ))
}

pub(super) fn edit_file(
    path: &Path,
    old: &str,
    new: &str,
    all: bool,
    root: &Path,
) -> Result<String, String> {
    if old == new {
        return Err("old_string and new_string are the same".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if old.is_empty() {
        if !text.is_empty() {
            return Err(
                "old_string is empty, but the file is not; give the text to replace".into(),
            );
        }
        std::fs::write(path, new).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(format!("Edited {}", shown(path, root)));
    }
    // A file with Windows line endings still matches text the model wrote with plain ones, and
    // keeps its line endings.
    let (old, new) = if !old.contains('\r') && text.contains("\r\n") && !text.contains(old) {
        (old.replace('\n', "\r\n"), new.replace('\n', "\r\n"))
    } else {
        (old.to_string(), new.to_string())
    };
    let (old, new) = (old.as_str(), new.as_str());
    let count = text.matches(old).count();
    let updated = match count {
        0 => {
            return Err(format!(
                "old_string was not found in {}. Read the file again and match it exactly",
                shown(path, root)
            ));
        }
        1 => text.replacen(old, new, 1),
        _ if all => text.replace(old, new),
        n => {
            return Err(format!(
                "old_string appears {n} times in {}; add surrounding lines to make it unique, or set replace_all",
                shown(path, root)
            ));
        }
    };
    std::fs::write(path, updated).map_err(|e| format!("{}: {e}", path.display()))?;
    let times = if count > 1 {
        format!(" ({count} places)")
    } else {
        String::new()
    };
    Ok(format!("Edited {}{times}", shown(path, root)))
}

pub(super) fn list_dir(path: &Path, root: &Path) -> Result<String, String> {
    let mut entries: Vec<String> = std::fs::read_dir(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .flatten()
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                format!("{name}/")
            } else {
                name
            }
        })
        .collect();
    entries.sort();
    let total = entries.len();
    entries.truncate(500);
    let mut out = format!("{}:\n{}", shown(path, root), entries.join("\n"));
    if total > entries.len() {
        out.push_str(&format!("\n({total} entries in all)"));
    }
    Ok(out)
}

/// Files under `dir`, honoring .gitignore and skipping `.git`.
fn walk(dir: &Path) -> impl Iterator<Item = PathBuf> {
    ignore::WalkBuilder::new(dir)
        .hidden(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
}

pub(super) fn glob(pattern: &str, dir: &Path, root: &Path) -> Result<String, String> {
    let matcher = globset::GlobBuilder::new(pattern)
        .literal_separator(pattern.contains('/'))
        .build()
        .map_err(|e| e.to_string())?
        .compile_matcher();
    let mut found: Vec<(PathBuf, std::time::SystemTime)> = walk(dir)
        .filter(|p| {
            let rel = p.strip_prefix(dir).unwrap_or(p);
            matcher.is_match(rel)
                || (!pattern.contains('/') && p.file_name().is_some_and(|n| matcher.is_match(n)))
        })
        .map(|p| {
            let modified = p
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (p, modified)
        })
        .collect();
    if found.is_empty() {
        return Ok("No files found".into());
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.1));
    let total = found.len();
    let mut out: Vec<String> = found
        .iter()
        .take(MAX_MATCHES)
        .map(|(p, _)| shown(p, root))
        .collect();
    if total > MAX_MATCHES {
        out.push(format!("({total} files in all; narrow the pattern)"));
    }
    Ok(out.join("\n"))
}

pub(super) fn grep(
    pattern: &str,
    path: &Path,
    filter: Option<&str>,
    nocase: bool,
    root: &Path,
) -> Result<String, String> {
    let regex = regex::RegexBuilder::new(pattern)
        .case_insensitive(nocase)
        .build()
        .map_err(|e| e.to_string())?;
    let filter = filter
        .map(|g| globset::Glob::new(g).map(|g| g.compile_matcher()))
        .transpose()
        .map_err(|e| e.to_string())?;
    let files: Box<dyn Iterator<Item = PathBuf>> = if path.is_file() {
        Box::new(std::iter::once(path.to_path_buf()))
    } else {
        Box::new(walk(path))
    };
    let mut out = Vec::new();
    let mut more = false;
    'files: for file in files {
        if let Some(filter) = &filter {
            let rel = file.strip_prefix(path).unwrap_or(&file);
            if !filter.is_match(rel) && !file.file_name().is_some_and(|n| filter.is_match(n)) {
                continue;
            }
        }
        let Ok(bytes) = std::fs::read(&file) else {
            continue;
        };
        if bytes[..bytes.len().min(8192)].contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (i, line) in text.lines().enumerate() {
            if regex.is_match(line) {
                // One match past the limit is enough to know there are more.
                if out.len() == MAX_MATCHES {
                    more = true;
                    break 'files;
                }
                let line: String = line.trim().chars().take(300).collect();
                out.push(format!("{}:{}: {line}", shown(&file, root), i + 1));
            }
        }
    }
    if out.is_empty() {
        return Ok("No matches".into());
    }
    if more {
        out.push(format!(
            "(more than {MAX_MATCHES} matches; narrow the search)"
        ));
    }
    Ok(out.join("\n"))
}
