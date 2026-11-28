//! The per-turn state a provider keeps while the turn's reader goes through its output, so the
//! reader itself handles only provider-neutral events.

// Rust guideline compliant 2026-10-09

use std::time::Duration;

use serde_json::Value;

use super::antigravity::Transcript;
use super::{Parsed, TurnSpec};
use crate::events::ChatEvent;

/// Where a line of output belongs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// To the turn's own reply.
    Own,
    /// To the reply, and also to the subagent chat opened for this call or thread.
    Also(String),
    /// To the subagent chat opened for this call or thread alone.
    Only(String),
}

/// One turn's view of a provider's output, around its line parser.
///
/// Every method has a default that does nothing, which is all most providers need.
pub trait LineReader: Send {
    /// Fills in what a line leaves out, from the lines before it.
    fn prepare(&mut self, _value: &mut Value) {}

    /// Whether the line starts a reply the idle agent was woken for.
    fn starts_reply(&self, _value: &Value) -> bool {
        false
    }

    /// Where the line belongs, given the turn's own session.
    fn route(&self, _value: &Value, _session_id: Option<&str>) -> Route {
        Route::Own
    }

    /// The events a subagent's line shows in its own chat, under `message_id`. `root` is the
    /// turn's folder as the chat has it, with any SSH host.
    fn child_events(
        &self,
        _value: &Value,
        _spec: &TurnSpec,
        _root: &str,
        _message_id: &str,
    ) -> Vec<ChatEvent> {
        Vec::new()
    }

    /// Whether the line is work the idle agent started by itself.
    fn starts_own_work(&self, _value: &Value) -> bool {
        false
    }

    /// How often the reader should look at work the agent does out of its stream.
    fn poll_every(&self) -> Option<Duration> {
        None
    }

    /// What that work did since the last look.
    fn tick(&mut self, _session_id: &str, _message_id: &str) -> Vec<Parsed> {
        Vec::new()
    }

    /// Adds what the line means beyond what the parser made of it.
    fn after_parse(
        &mut self,
        _value: &Value,
        _session_id: &str,
        _message_id: &str,
        _parsed: &mut Vec<Parsed>,
    ) {
    }

    /// Whether work out of the stream still holds the turn open.
    fn is_holding(&self) -> bool {
        false
    }

    /// The transcript a call's chat follows from its start, when there is one.
    fn transcript(&self, _tool_id: &str) -> Option<Transcript> {
        None
    }

    /// Ends the work still open once the agent has exited.
    fn finish_open(&mut self, _message_id: &str, _failed: bool) -> Vec<Parsed> {
        Vec::new()
    }

    /// Whether a request's usage is the agent's report of its context, which Patchwork
    /// compacts by.
    fn reports_context(&self, _request_id: &str) -> bool {
        false
    }
}

/// The reader of a provider that needs no state of its own.
#[derive(Debug, Default)]
pub(crate) struct Plain;

impl LineReader for Plain {}
