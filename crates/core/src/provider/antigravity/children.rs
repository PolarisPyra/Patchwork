//! The work of agy's sub agents and swarm workers, read from the transcripts agy keeps.
//!
//! The call that starts them returns at once, and they work on alone. Their steps are not sent
//! over ACP, so they are read from the transcript agy writes for each conversation.

// Rust guideline compliant 2026-11-23

use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::events::AgentEvent;
use crate::provider::{Parsed, ToolStart, text_delta, tool_finished, tool_started};

use super::cli::home;
use super::tool_fields::tool_summary;

/// The most sub agents one call is followed for.
const MAX_CHILDREN: usize = 16;

/// The most of a transcript read at once, so a burst of steps cannot stall the reader.
const MAX_READ_BYTES: u64 = 1024 * 1024;

/// The agy profiles a conversation's folder may be in.
const PROFILES: [&str; 3] = ["antigravity-cli", "antigravity-acp", "antigravity"];

/// Where a conversation's transcript is, under its folder.
const TRANSCRIPT: &str = ".system_generated/logs/transcript.jsonl";

/// The transcripts of the sub agents a finished `invoke_subagent` call's `output` names.
#[must_use]
pub fn child_transcripts(output: &str) -> Vec<Transcript> {
    const KEY: &str = "\"conversationId\":";
    let Some(home) = home() else {
        return Vec::new();
    };
    output
        .match_indices(KEY)
        .filter_map(|(at, _)| {
            let rest = output[at + KEY.len()..].trim_start().strip_prefix('"')?;
            let id = &rest[..rest.find('"')?];
            if !is_conversation_id(id) {
                return None;
            }
            PROFILES
                .iter()
                .map(|profile| home.join(profile).join("brain").join(id).join(TRANSCRIPT))
                .find(|path| path.is_file())
                .map(|path| Transcript::new(id, path, Ends::Answered))
        })
        .take(MAX_CHILDREN)
        .collect()
}

/// Whether `id` can be a conversation id, so it is safe to put in a path.
pub(super) fn is_conversation_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The folder of conversation `id` in the profile `agy acp` runs with.
pub(super) fn conversation_dir(id: &str) -> Option<PathBuf> {
    if !is_conversation_id(id) {
        return None;
    }
    Some(home()?.join("antigravity-cli").join("brain").join(id))
}

/// The transcript of conversation `id`, in the profile `agy acp` runs with.
pub(super) fn transcript_path(id: &str) -> Option<PathBuf> {
    Some(conversation_dir(id)?.join(TRANSCRIPT))
}

/// When a transcript has nothing more to follow.
#[derive(Debug)]
enum Ends {
    /// A sub agent stops once it answers with nothing left running.
    Answered,
    /// A swarm worker answers, parks, and is woken again, until the swarm kills it.
    Killed(Arc<AtomicBool>),
}

/// One sub agent's or swarm worker's transcript, read as it grows.
#[derive(Debug)]
pub struct Transcript {
    id: String,
    path: PathBuf,
    ends: Ends,
    offset: u64,
    /// The end of the last read, up to a line not yet complete.
    partial: Vec<u8>,
    /// Calls it started whose results have not been written yet, oldest first.
    open: VecDeque<String>,
    /// Its last step was an answer that called nothing, so it has stopped.
    answered: bool,
    /// Its newest step ends a turn: an answer, or the `swarm_wait` of a batch whose own result
    /// is `DONE`. agy's board reads the same, so this is its `turnEnded`.
    ended: bool,
    /// The `swarm_wait` among the calls of the reply in flight (its index), and the results of
    /// that batch read so far.
    waiting: Option<(usize, usize)>,
}

impl Transcript {
    fn new(id: &str, path: PathBuf, ends: Ends) -> Self {
        Self {
            id: id.into(),
            path,
            ends,
            offset: 0,
            partial: Vec::new(),
            open: VecDeque::new(),
            answered: false,
            ended: false,
            waiting: None,
        }
    }

    /// A swarm worker's transcript, followed until `killed` is set.
    pub(super) fn worker(id: &str, killed: Arc<AtomicBool>) -> Option<Self> {
        Some(Self::new(id, transcript_path(id)?, Ends::Killed(killed)))
    }

    /// Whether it has nothing more to follow.
    #[must_use]
    pub fn is_done(&self) -> bool {
        match &self.ends {
            Ends::Answered => self.answered && self.open.is_empty(),
            Ends::Killed(killed) => killed.load(Ordering::SeqCst),
        }
    }

    /// Whether going quiet for a long time means it has stopped. A parked worker is quiet too.
    #[must_use]
    pub fn ends_when_quiet(&self) -> bool {
        matches!(self.ends, Ends::Answered)
    }

    /// Whether its newest step ends a turn, so it stopped by itself rather than being cut off
    /// partway through one.
    #[must_use]
    pub fn has_ended(&self) -> bool {
        self.ended
    }

    /// The events for the steps written since the last read, under `message_id`.
    pub fn read_new(&mut self, message_id: &str, root: &str) -> Vec<AgentEvent> {
        let Ok(mut file) = File::open(&self.path) else {
            return Vec::new();
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut bytes = Vec::new();
        let Ok(read) = file.take(MAX_READ_BYTES).read_to_end(&mut bytes) else {
            return Vec::new();
        };
        self.offset += read as u64;
        self.partial.extend_from_slice(&bytes);
        let Some(end) = self.partial.iter().rposition(|b| *b == b'\n') else {
            return Vec::new();
        };
        let complete: Vec<u8> = self.partial.drain(..=end).collect();
        let mut events = Vec::new();
        for line in complete.split(|b| *b == b'\n') {
            if let Ok(step) = serde_json::from_slice::<Value>(line) {
                self.step(&step, message_id, root, &mut events);
            }
        }
        events
    }

    fn step(&mut self, step: &Value, message_id: &str, root: &str, events: &mut Vec<AgentEvent>) {
        let mut parsed = Vec::new();
        match step["type"].as_str() {
            Some("PLANNER_RESPONSE") => {
                if let Some(text) = step["content"].as_str().filter(|t| !t.trim().is_empty()) {
                    parsed.push(text_delta(message_id, text));
                }
                let calls = step["tool_calls"].as_array().map_or(&[][..], Vec::as_slice);
                let index = step["step_index"].as_u64().unwrap_or_default();
                for (n, call) in calls.iter().enumerate() {
                    let id = format!("{}:{index}:{n}", self.id);
                    let name = call["name"].as_str().unwrap_or("tool");
                    let args = arguments(&call["args"]);
                    let summary = tool_summary(&Value::Null, &args, root);
                    parsed.push(tool_started(message_id, ToolStart::new(&id, name, summary)));
                    self.open.push_back(id);
                }
                self.answered = calls.is_empty();
                self.ended = self.answered;
                // A batch's results follow in call order, so the wait's own result is the one at
                // its index; the turn ends only once that one is DONE.
                self.waiting = calls
                    .iter()
                    .position(|call| call["name"] == "swarm_wait")
                    .map(|at| (at, 0));
            }
            Some("GENERIC") => {
                if let Some(id) = self.open.pop_front() {
                    let failed = step["status"] == "ERROR";
                    let output = step["error"]
                        .as_str()
                        .or_else(|| step["content"].as_str())
                        .unwrap_or_default();
                    parsed.push(tool_finished(message_id, &id, failed, output));
                    if let Some((wait, results)) = &mut self.waiting {
                        if *results == *wait && step["status"] == "DONE" {
                            self.ended = true;
                        }
                        *results += 1;
                    }
                }
            }
            // Its instructions, which its chat opens with, and messages sent to it.
            _ => {}
        }
        events.extend(parsed.into_iter().filter_map(|p| match p {
            Parsed::Event(event) => Some(event),
            _ => None,
        }));
    }
}

/// The call's arguments, which agy writes with each value as JSON text.
fn arguments(args: &Value) -> Value {
    let Some(map) = args.as_object() else {
        return args.clone();
    };
    map.iter()
        .map(|(key, value)| {
            let decoded = value
                .as_str()
                .and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or_else(|| value.clone());
            (key.clone(), decoded)
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}
