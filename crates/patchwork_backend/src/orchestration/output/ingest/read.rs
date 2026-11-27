//! The loop that reads the agent's stdout for one turn and hands each line on.
//!
//! What sets one agent's output apart is kept by its provider's line reader and capabilities, so
//! nothing here asks which agent the turn runs.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::process::{Child, ChildStdout};
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

use serde_json::Value;

use super::compaction::Compaction;
use super::idle::go_idle;
use super::unprompted::{Lines, Next, Unprompted};
use crate::chats::{Goal, MessageStatus};
use crate::events::ChatEvent;
use crate::logging::{AGENT_LOG, Data, Source};
use crate::orchestration::output::stream::{Unsaved, publish};
use crate::orchestration::output::usage::Request;
use crate::orchestration::tasks::background::Background;
use crate::orchestration::tasks::subagents::SubagentChat;
use crate::orchestration::turn::start::Turn;
use crate::orchestration::turn::stuck::Repeats;
use crate::provider::{Capabilities, LineReader, ProviderKind, Route, Steer};
use crate::settings::LogLevel;
use crate::system::{Locked, now_ms};

/// Reads the agent's stdout until it closes, then records the final status.
pub(in crate::orchestration) fn ingest(
    turn: Turn,
    child: Child,
    stdout: ChildStdout,
    stderr_tail: JoinHandle<String>,
) {
    let lines = Lines::read(stdout, &turn.wake);
    let mut reading = Reading::new(turn, child.id());
    while reading.next(&lines) {}
    reading.end(child, stderr_tail);
}

/// What kind of output a line is, as its `type` (or JSON-RPC `method`) and any `subtype`.
fn line_kind(value: &Value) -> String {
    let kind = value["type"]
        .as_str()
        .or_else(|| value["method"].as_str())
        .unwrap_or("untyped");
    match value["subtype"].as_str() {
        Some(subtype) => format!("{kind}/{subtype}"),
        None => kind.into(),
    }
}

/// Publishes a subagent's events to its own thread, with its replies a paragraph apart.
fn publish_child(turn: &Turn, child: &mut SubagentChat, events: Vec<ChatEvent>) {
    for mut event in events {
        if let ChatEvent::TextDelta { text, .. } = &mut event {
            if child.has_text {
                text.insert_str(0, "\n\n");
            }
            child.has_text = true;
        }
        publish(&turn.core, &child.chat_id, event);
    }
}

/// Completes the replies a steer moved the turn on from, and counts them in `completed`, which is
/// how many of `segments` are done. Left streaming, the reply before the steer keeps its own
/// "Working" line while the answer to the steer has another, so the chat shows two turns running
/// at once. The end of the turn skips the replies counted here.
pub(super) fn close_steered_segment(turn: &Turn, segments: &[String], completed: &mut usize) {
    for id in &segments[*completed..] {
        turn.publish(ChatEvent::TurnCompleted {
            message_id: id.clone(),
            status: MessageStatus::Complete,
            error: None,
        });
    }
    *completed = segments.len();
}

/// Whether the agent has listed its slash commands yet, with the line waiting until it has.
pub(super) enum Commands {
    Unlisted(Option<String>),
    Listed,
}

/// One turn's reading of its agent's output: everything kept from one line to the next.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate fact the reading keeps between lines"
)]
pub(super) struct Reading {
    pub(super) turn: Turn,
    pub(super) pid: u32,
    /// The provider's own state, around its parser.
    pub(super) reader: Box<dyn LineReader>,
    pub(super) caps: Capabilities,
    pub(super) goal: Option<Goal>,
    pub(super) has_text: bool,
    /// Whether the last event was text, which a streaming agent's next chunk continues.
    pub(super) last_was_text: bool,
    /// The reply opened for a steer has nothing in it yet, so a steer right after it shares it.
    pub(super) steer_reply_empty: bool,
    /// Tool id → the message it was started in, which a steer may since have moved on from.
    pub(super) tool_owner: HashMap<String, String>,
    /// The name and summary each restated call was last shown with.
    pub(super) tool_shown: HashMap<String, (String, String)>,
    /// The chats of the subagents still running, by the call that started each.
    pub(super) children: HashMap<String, SubagentChat>,
    /// The tasks the agent started, and which of them are listed under the background.
    pub(super) background: Background,
    /// Every message of the turn: the first reply, then one per steering message taken.
    pub(super) segments: Vec<String>,
    /// How many of `segments` were completed when the agent went idle.
    pub(super) completed: usize,
    pub(super) stream_error: Option<String>,
    pub(super) repeats: Repeats,
    /// The agent could not find the session the turn resumed, so the turn runs again without it.
    pub(super) session_lost: bool,
    /// The agent is replaying the session the turn loads, until it answers the load.
    pub(super) replaying: bool,
    /// The agent reported its turn finished.
    pub(super) ended: bool,
    /// Text streamed to the frontend but not yet stored; saving every delta stalls the reader,
    /// so it is stored before the next event and at the end.
    pub(super) unsaved: Unsaved,
    /// Every model request the turn has made; each reply's usage is summed from these.
    pub(super) requests: Vec<Request>,
    /// The model a request runs on when the agent does not name it, looked up at the first one.
    #[expect(
        clippy::option_option,
        reason = "not yet looked up differs from looked up and found nothing"
    )]
    pub(super) fallback_model: Option<Option<String>>,
    /// Where this turn's requests start in `requests`; the agent may answer several turns.
    pub(super) turn_requests: usize,
    pub(super) commands: Commands,
    /// Patchwork's compaction of a reply that fills the window, and the calls running, which it
    /// waits for.
    pub(super) compaction: Compaction,
    pub(super) open_calls: HashSet<String>,
    /// Whether a call of the current line ended.
    pub(super) call_ended: bool,
    /// Kinds of output the parser had no use for, logged once each per turn, so a CLI that
    /// changes how it reports something shows up in the agent log.
    pub(super) logged_kinds: HashSet<String>,
    pub(super) unprompted: Unprompted,
}

impl Reading {
    fn new(turn: Turn, pid: u32) -> Self {
        let provider = turn.agent.provider();
        let goal = turn.core.store.chat(&turn.chat_id).and_then(|t| t.goal);
        Self {
            segments: vec![turn.message_id.clone()],
            reader: provider.reader(),
            caps: provider.capabilities(),
            turn,
            pid,
            goal,
            has_text: false,
            last_was_text: false,
            steer_reply_empty: false,
            tool_owner: HashMap::new(),
            tool_shown: HashMap::new(),
            children: HashMap::new(),
            background: Background::default(),
            completed: 0,
            stream_error: None,
            repeats: Repeats::default(),
            session_lost: false,
            replaying: false,
            ended: false,
            unsaved: Unsaved::default(),
            requests: Vec::new(),
            fallback_model: None,
            turn_requests: 0,
            commands: Commands::Unlisted(None),
            compaction: Compaction::default(),
            open_calls: HashSet::new(),
            call_ended: false,
            logged_kinds: HashSet::new(),
            unprompted: Unprompted::No,
        }
    }

    /// The agent log's source for this turn's lines.
    pub(super) fn source(&self) -> Source {
        let turn = &self.turn;
        Source::agent(turn.agent, self.pid, &turn.chat_id, Some(&turn.message_id))
    }

    /// Completes the replies not yet completed and leaves the agent idle.
    pub(super) fn idle(&mut self, stopped: bool, error: Option<&str>) {
        go_idle(
            &self.turn,
            &self.segments,
            &mut self.completed,
            &mut self.unsaved,
            stopped,
            error,
        );
    }

    /// Starts the next reply of the turn, with no text in it yet.
    pub(super) fn start_segment(&mut self) {
        self.turn.next_segment(&mut self.unsaved);
        self.segments.push(self.turn.message_id.clone());
        (self.has_text, self.last_was_text) = (false, false);
    }

    /// Handles what comes next from the agent; false once its output has closed.
    fn next(&mut self, lines: &Lines) -> bool {
        self.turn
            .calls_open
            .store(!self.open_calls.is_empty(), Ordering::Relaxed);
        // A tick rereads work the agent does out of its stream, and says nothing about the agent.
        let mut tick = false;
        let line = match lines.next(&self.unprompted, self.reader.poll_every()) {
            Next::Line(line) => Some(line),
            // The idle agent was sent a message, and its reply starts here; the agent may say
            // nothing for a while, as Antigravity does all through `/compact`.
            Next::Woken => None,
            Next::Tick => {
                tick = true;
                None
            }
            Next::Closed => return false,
            Next::Quiet => {
                self.went_quiet();
                return true;
            }
        };
        let Some(mut value) = self.value(line.as_deref()) else {
            return true;
        };
        self.reader.prepare(&mut value);
        // Claude's `tool_progress` only says a call is still running, which a stuck one is too.
        if value["type"] != "tool_progress" && !tick {
            self.turn.last_output.store(now_ms(), Ordering::Relaxed);
        }
        self.take_steers(&value, tick);
        if line.is_none() && !tick {
            return true;
        }
        // A subagent's lines also go to its own chat, or to it alone.
        let route = self.reader.route(&value, self.turn.session_id.as_deref());
        if let Route::Also(id) | Route::Only(id) = &route
            && let Some(child) = self.children.get_mut(id)
        {
            let events = self.reader.child_events(
                &value,
                &self.turn.spec(),
                &self.turn.root,
                &child.message_id,
            );
            publish_child(&self.turn, child, events);
        }
        if matches!(route, Route::Only(_)) {
            return true;
        }
        self.own_work(&value);
        // The conversation a loaded session replays was shown before; only the answer counts.
        if self.replaying && value.get("id").is_none() && value["method"] == "session/update" {
            return true;
        }
        let mut parsed = if tick {
            self.reader.tick(
                self.turn.session_id.as_deref().unwrap_or_default(),
                &self.turn.message_id,
            )
        } else {
            crate::provider::parse_line(
                self.turn.agent,
                &value,
                &self.turn.spec(),
                &self.turn.message_id,
            )
        };
        self.reader.after_parse(
            &value,
            self.turn.session_id.as_deref().unwrap_or_default(),
            &self.turn.message_id,
            &mut parsed,
        );
        // A subagent's lines went to its own chat above.
        let routed = matches!(route, Route::Also(_));
        if parsed.is_empty() && !routed && !tick && AGENT_LOG.logs(LogLevel::Debug) {
            let kind = line_kind(&value);
            if self.logged_kinds.insert(kind.clone()) {
                AGENT_LOG.write(LogLevel::Debug, "ignored", &self.source(), Data::Text(kind));
            }
        }
        self.call_ended = false;
        for item in parsed {
            self.dispatch(item, &value);
        }
        // Between calls is where a reply that filled the window stops to compact.
        if self.call_ended && self.caps.patchwork_compacts {
            let busy = !self.open_calls.is_empty() || self.reader.is_holding();
            self.compaction.stop_if_due(&self.turn, busy);
        }
        true
    }

    /// The line as JSON, logged; `Null` when there is no line. None for a line that is not JSON.
    fn value(&mut self, line: Option<&str>) -> Option<Value> {
        let Some(line) = line else {
            return Some(Value::Null);
        };
        if AGENT_LOG.logs(LogLevel::Error) {
            AGENT_LOG.line("out", &self.source(), line);
        }
        let value = serde_json::from_str::<Value>(line).ok();
        if value.is_none() && self.logged_kinds.insert("unparsed".into()) {
            AGENT_LOG.write(
                LogLevel::Warning,
                "unparsed",
                &self.source(),
                Data::Outline(line.to_owned()),
            );
        }
        value
    }

    /// The agent stopped working by itself, so the reply it started ends. A message sent meanwhile
    /// that it has yet to answer keeps the reply open for that answer.
    fn went_quiet(&mut self) {
        if self.turn.steers.locked().is_empty() {
            // Before the reply ends, so the messages it sends on find the agent idle.
            self.unprompted.set(Unprompted::No, &self.turn.alone);
            self.idle(false, None);
            self.ended = true;
        }
    }

    /// Moves on to the reply that answers the messages sent while the agent replied or idled.
    ///
    /// A message Claude took after it had already ended its reply (while a background subagent
    /// kept it running) is answered from here on, below that message. So is a reply an idle
    /// Claude starts on its own, such as once a background command ends.
    fn take_steers(&mut self, value: &Value, tick: bool) {
        let resumed =
            self.reader.starts_reply(value) && self.turn.idle.swap(false, Ordering::SeqCst);
        let warm_reply = self.caps.steer != Steer::Inline && !self.turn.steers.locked().is_empty();
        if tick || !(self.ended || self.unprompted.is_working()) || !(resumed || warm_reply) {
            return;
        }
        let mut steers = self.turn.steers.locked();
        // An agent stopped for a steer takes only the message that resumed it now.
        if self.caps.steer == Steer::StopsReply {
            steers.pop_front();
        } else {
            steers.clear();
        }
        drop(steers);
        // The reply the agent started by itself ends where its answer to the message starts.
        if self.unprompted.is_working() {
            close_steered_segment(&self.turn, &self.segments, &mut self.completed);
            self.unprompted.set(Unprompted::No, &self.turn.alone);
        }
        self.start_segment();
        self.ended = false;
    }

    /// Once a command it moved to the background ends, an idle agent may go on working by itself.
    /// That work is a reply of its own, which ends once it goes quiet. Messages sent meanwhile
    /// still go to it as new prompts, which it answers once it is done.
    fn own_work(&mut self, value: &Value) {
        if self.ended && self.turn.idle.load(Ordering::SeqCst) && self.reader.starts_own_work(value)
        {
            self.start_segment();
            self.ended = false;
            self.unprompted
                .set(Unprompted::Working(HashSet::new()), &self.turn.alone);
        }
    }
}
