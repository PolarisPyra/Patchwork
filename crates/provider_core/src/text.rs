//! Short text and tool-input summary helpers shared across provider parsers.

// Rust guideline compliant 2026-10-09

use serde_json::Value;

/// Collapses whitespace in `s` and truncates it to 120 characters.
#[must_use]
pub fn one_line(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}

/// `text` trimmed and cut to `limit` characters, marked with an ellipsis when cut.
#[must_use]
pub fn clip(text: &str, limit: usize) -> String {
    let text = text.trim();
    match text.char_indices().nth(limit) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

/// Strips `root` and any leading `./` from `path` for concise display.
#[must_use]
pub fn relative(root: &str, path: &str) -> String {
    let root = root.trim_end_matches('/');
    let path = path
        .strip_prefix(root)
        .and_then(|p| p.strip_prefix('/'))
        .unwrap_or(path);
    path.strip_prefix("./").unwrap_or(path).to_string()
}

/// One-line summary of a tool call's input: the first useful field that is present.
#[must_use]
pub fn summarize(input: &Value, root: &str) -> String {
    const KEYS: [&str; 7] = [
        "command",
        "skill",
        "file_path",
        "pattern",
        "url",
        "description",
        "query",
    ];
    KEYS.iter()
        .find_map(|key| {
            input[*key].as_str().filter(|s| !s.is_empty()).map(|s| {
                if *key == "file_path" {
                    relative(root, s)
                } else {
                    s.to_string()
                }
            })
        })
        .map(|s| one_line(&s))
        .unwrap_or_default()
}
