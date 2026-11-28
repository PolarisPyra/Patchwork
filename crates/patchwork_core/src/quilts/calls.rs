//! A chat's calls of the quilt tool, as its transcript holds them: telling them from other
//! calls, and finding the quilt and version each one saved.

// Rust guideline compliant 2026-10-09

pub use patchwork_model::quilts::is_quilt_call;
use super::types::Quilt;
use crate::chats::{EditLines, ToolCall};

/// The tool's name as Claude calls it on Patchwork's own server.
pub(crate) const CLAUDE_NAME: &str = "mcp__patchwork__quilt";

/// What the tool tells its agent after saving version `n` of the quilt titled `title`,
/// named `key` in its chat. [`saved_by`] reads it back.
pub(super) fn saved(title: &str, key: &str, n: u32) -> String {
    format!("Saved version {n} of \"{title}\" (id {key}). The person sees it on the Quilts page.")
}

/// The quilt and version that the quilt-tool call `tool` saved.
///
/// Read from what the tool answered, among the quilts of chat `chat_id`. An answer from
/// before it named the quilt's id is matched by its title.
#[must_use]
pub fn saved_by<'a>(
    quilts: &'a [Quilt],
    chat_id: &str,
    tool: &ToolCall,
) -> Option<(&'a Quilt, u32)> {
    let rest = tool.output.as_deref()?.strip_prefix("Saved version ")?;
    let (n, rest) = rest.split_once(" of \"")?;
    let n = n.parse().ok()?;
    let ours = || quilts.iter().filter(|a| a.chat_id == chat_id);
    let found = if let Some((_, after)) = rest.split_once("\" (id ") {
        let (key, _) = after.split_once(')')?;
        ours().find(|a| a.key == key)?
    } else {
        let (title, _) = rest.rsplit_once("\". ")?;
        ours().find(|a| a.title == title)?
    };
    Some((found, n))
}

/// Gives the quilt-tool call `tool` the type, title and line count its whole input `args` holds,
/// for a card shown once the call has begun. `args` may be the input's JSON text.
pub(crate) fn fill_in(tool: &mut ToolCall, args: &serde_json::Value) {
    let parsed;
    let args = match args.as_str() {
        Some(text) => {
            parsed = serde_json::from_str(text).unwrap_or_default();
            &parsed
        }
        None => args,
    };
    if let Some(kind) = args["type"].as_str().filter(|k| !k.trim().is_empty()) {
        tool.kind = Some(kind.trim().to_owned());
    }
    if let Some(title) = args["title"].as_str().filter(|t| !t.trim().is_empty()) {
        tool.title = Some(title.trim().to_owned());
    }
    let body = args["content"]
        .as_str()
        .or_else(|| args["new_str"].as_str());
    if let Some(body) = body {
        tool.lines = Some(EditLines {
            added: u32::try_from(body.lines().count()).unwrap_or(u32::MAX),
            removed: 0,
        });
    }
}
