//! A chat's calls of the quilt tool, as its transcript holds them: telling them from other
//! calls, and finding the quilt and version each one saved.

// Rust guideline compliant 2026-10-09

use super::tool::TOOL;
use super::types::Quilt;
use crate::chats::{EditLines, ToolCall};

/// The tool's name as Claude calls it on Patchwork's own server.
pub(crate) const CLAUDE_NAME: &str = "mcp__patchwork__quilt";

/// What the tool tells its agent after saving version `n` of the quilt titled `title`,
/// named `key` in its chat. [`saved_by`] reads it back.
pub(super) fn saved(title: &str, key: &str, n: u32) -> String {
    format!("Saved version {n} of \"{title}\" (id {key}). The person sees it on the Quilts page.")
}

/// The tool's name before it was called `quilt`, which older chats' calls hold.
const OLD_TOOL: &str = "artifact";

/// Whether `tool` is a call of the quilt tool, by its name now or before: by Claude, by Codex,
/// or through Patchwork's own harness, which names it as the tool is named.
#[must_use]
pub fn is_quilt_call(tool: &ToolCall) -> bool {
    [TOOL, OLD_TOOL].into_iter().any(|name| {
        tool.name == name
            || tool.name == format!("mcp__patchwork__{name}")
            || (tool.name == "MCP" && tool.summary == format!("patchwork.{name}"))
    })
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

/// How many more lines are written before a card being written is told again.
const STEP: u32 = 10;

/// What a card being written shows of a quilt-tool call whose input is still arriving: its
/// title, its type, its command, and how many lines it holds so far.
#[derive(Debug, Clone, Default)]
pub(crate) struct Written {
    pub title: Option<String>,
    pub kind: Option<String>,
    pub command: Option<String>,
    pub lines: u32,
}

impl Written {
    /// Reads the input `json` as written so far, and returns whether the card has something new
    /// to show: the title, type or command once each is whole, or another [`STEP`] lines.
    pub(crate) fn read(&mut self, json: &str) -> bool {
        let title = string_field(json, "title");
        let kind = string_field(json, "type");
        let command = string_field(json, "command");
        let lines = u32::try_from(json.matches("\\n").count()).unwrap_or(u32::MAX);
        let mut news = false;
        if title.is_some() && self.title.is_none() {
            self.title = title;
            news = true;
        }
        if kind.is_some() && self.kind.is_none() {
            self.kind = kind;
            news = true;
        }
        if command.is_some() && self.command.is_none() {
            self.command = command;
            news = true;
        }
        if lines >= self.lines + STEP {
            self.lines = lines;
            news = true;
        }
        news
    }
}

/// The string `key` holds in the partly written JSON object `json`, once all of it is written.
fn string_field(json: &str, key: &str) -> Option<String> {
    let after = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let after = after.trim_start().strip_prefix(':')?.trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let mut escaped = false;
    let end = after.char_indices().skip(1).find_map(|(i, c)| {
        let end = c == '"' && !escaped;
        escaped = c == '\\' && !escaped;
        end.then_some(i)
    })?;
    match serde_json::from_str::<serde_json::Value>(&after[..=end]) {
        Ok(serde_json::Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        _ => None,
    }
}
