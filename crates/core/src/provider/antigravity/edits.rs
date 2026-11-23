//! The lines an agy edit adds and removes, from the diff it reports or the text it replaces.

// Rust guideline compliant 2026-11-23

use serde_json::Value;

use crate::chats::EditLines;
use crate::provider::changed_lines;

/// The lines `tool`, a `tool_call` or a permission request's `toolCall`, adds and removes.
///
/// agy's `diff` content counts first. Without it, the replacements in its input are counted,
/// and a whole file written counts as added, which holds only when it creates the file. None
/// when the call carries no edit text.
pub(super) fn edit_lines(tool: &Value) -> Option<EditLines> {
    let diffs: Vec<EditLines> = tool["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| entry["type"] == "diff")
        .filter_map(|entry| {
            let new = entry["newText"].as_str()?;
            Some(changed_lines(
                entry["oldText"].as_str().unwrap_or_default(),
                new,
            ))
        })
        .collect();
    if !diffs.is_empty() {
        return Some(sum(diffs));
    }
    let input = if tool["rawInput"].is_object() {
        &tool["rawInput"]
    } else {
        &tool["args"]
    };
    // agy takes its edit arguments in either casing.
    let field = |value: &Value, pascal: &str, snake: &str| -> Option<String> {
        value[pascal]
            .as_str()
            .or_else(|| value[snake].as_str())
            .map(String::from)
    };
    let chunk = |value: &Value| {
        let old = field(value, "TargetContent", "target_content")?;
        let new = field(value, "ReplacementContent", "replacement_content")?;
        Some(changed_lines(&old, &new))
    };
    let chunks = input["ReplacementChunks"]
        .as_array()
        .or_else(|| input["replacement_chunks"].as_array());
    if let Some(chunks) = chunks {
        return chunks
            .iter()
            .map(chunk)
            .collect::<Option<Vec<_>>>()
            .map(sum);
    }
    if let Some(lines) = chunk(input) {
        return Some(lines);
    }
    if input["Overwrite"] == true || input["overwrite"] == true {
        return None;
    }
    field(input, "CodeContent", "code_content").map(|text| EditLines {
        added: u32::try_from(text.lines().count()).unwrap_or(u32::MAX),
        removed: 0,
    })
}

fn sum(all: Vec<EditLines>) -> EditLines {
    all.into_iter().fold(
        EditLines {
            added: 0,
            removed: 0,
        },
        |sum, lines| EditLines {
            added: sum.added.saturating_add(lines.added),
            removed: sum.removed.saturating_add(lines.removed),
        },
    )
}
