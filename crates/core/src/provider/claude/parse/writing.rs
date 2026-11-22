//! A quilt call while Claude writes its input, so its card can show the quilt's title
//! and how many lines are written before the call is whole.
//!
//! Claude streams a call's input as pieces of JSON that name the call only by its place in the
//! message, so the calls being written are kept here between lines.

// Rust guideline compliant 2026-10-09

use std::sync::{Mutex, PoisonError};

use crate::quilts::Written;
use crate::events::AgentEvent;
use crate::provider::Parsed;
use crate::chats::{EditLines, ToolCall};

/// A quilt call being written.
struct Writing {
    message_id: String,
    /// Its place among the message's content blocks.
    index: u64,
    tool: ToolCall,
    /// Its input as written so far.
    input: String,
    /// What its card shows of it so far.
    written: Written,
}

/// The quilt calls being written, across every turn.
static WRITING: Mutex<Vec<Writing>> = Mutex::new(Vec::new());

/// Starts following the call `tool`, the `index`th block of message `message_id`.
pub(super) fn started(message_id: &str, index: u64, tool: &ToolCall) {
    let mut writing = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
    writing.retain(|w| w.message_id != message_id || w.index != index);
    writing.push(Writing {
        message_id: message_id.to_owned(),
        index,
        tool: tool.clone(),
        input: String::new(),
        written: Written::default(),
    });
}

/// Adds the piece `json` to the input of the `index`th block of message `message_id`, and
/// returns the call again when its card has something new to show.
pub(super) fn grew(message_id: &str, index: u64, json: &str) -> Option<Parsed> {
    let mut writing = WRITING.lock().unwrap_or_else(PoisonError::into_inner);
    let call = writing
        .iter_mut()
        .find(|w| w.message_id == message_id && w.index == index)?;
    call.input.push_str(json);
    if !call.written.read(&call.input) {
        return None;
    }
    call.tool.title.clone_from(&call.written.title);
    call.tool.kind.clone_from(&call.written.kind);
    if let Some(command) = &call.written.command {
        call.tool.summary.clone_from(command);
    }
    if call.written.lines > 0 {
        call.tool.lines = Some(EditLines {
            added: call.written.lines,
            removed: 0,
        });
    }
    Some(Parsed::Event(AgentEvent::ToolStarted {
        message_id: message_id.to_owned(),
        tool: call.tool.clone(),
    }))
}

/// Stops following the `index`th block of message `message_id`, once it is whole.
pub(super) fn stopped(message_id: &str, index: u64) {
    WRITING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|w| w.message_id != message_id || w.index != index);
}
