//! agy's swarms: the workers a coordinator adds, read from the files agy keeps, and how the
//! swarm's tool calls are shown.
//!
//! agy sends nothing about a swarm over ACP. Its roster is `members.json` in the coordinator's
//! `swarm` folder, and each worker's steps are in its own conversation's transcript, so both are
//! reread on a timer while a swarm runs. A worker's stand is the one agy's own board reports
//! (`board.StateOf`): killed, or waiting when its run has stopped, else working. A worker with
//! no transcript yet falls back to `wake_mode` the way agy does, and a post that has rung a
//! member leaves it waiting until its run resumes, as agy has it. agy's dashboard puts a
//! member's newest step failing, and a member working with a transcript that has not moved,
//! beside that stand, so the same two reports are kept here: a worker on quota or an error, or
//! one that has gone quiet, is stuck rather than working, and stops holding the swarm open. agy
//! names a used-up quota apart from any other failure (`QUOTA_RE`), and so does this.

// Rust guideline compliant 2026-11-24

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::chats::{BackgroundTask, SubAgent, TaskStatus};
use crate::provider::{Parsed, TaskEvent, ToolStart, one_line, tool_finished, tool_started};
use crate::system::now_ms;

use super::children::{Transcript, conversation_dir, is_conversation_id, transcript_path};
use super::errors::is_quota;

/// Prefix of the ids of the calls that stand for swarm workers (`swarm:<conversationId>`).
pub const SWARM_TASK_PREFIX: &str = "swarm:";

/// How often a running swarm's roster and its workers' transcripts are reread.
pub const SWARM_POLL: Duration = Duration::from_secs(2);

/// The background task listed while workers are at work. It keeps the agent's input open, so a
/// reply that would otherwise close it does not end the swarm under them, and keeps an idle
/// coordinator from being retired or a quiet one from being taken for stalled. Its description
/// names the workers it waits on.
const HOLD_TASK: &str = "swarm:hold";

/// What the hold is called when it names no worker.
const HOLD_TITLE: &str = "Swarm workers";

/// A working worker whose transcript has not moved for this long is stuck, as agy's board
/// reports it, and no longer holds the swarm open.
const STUCK: Duration = Duration::from_secs(10 * 60);

/// How long a failed step must stay a transcript's newest before its worker is stuck on it. The
/// agent normally carries on after a tool step fails, so only one it stopped on counts; a model
/// error counts at once. This is agy's `STEP_ERROR_SETTLE_MS`.
const ERROR_SETTLE: Duration = Duration::from_secs(60);

/// How many roster reads in a row may fail to find the file while the hold is open before the
/// swarm is taken to be over. The roster is only gone once the swarm is.
const ROSTER_GONE_READS: u8 = 5;

/// The most of a transcript's end read to decide whether its worker's turn has ended. A single
/// step longer than this reads as mid-turn until the next step is written.
const MAX_TAIL_BYTES: u64 = 256 * 1024;

/// The largest `members.json` read (4 MiB).
const MAX_MEMBERS_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// Upper bound on batch swarm worker launches parsed from untrusted tool input.
const MAX_SWARM_ADD_LAUNCHES: usize = 64;

/// One reply's view of agy's swarm.
#[derive(Debug, Default)]
pub struct Swarm {
    /// The coordinator called a swarm tool, so its roster is reread.
    running: bool,
    /// The hold task is open.
    held: bool,
    /// What the hold is described as, as last published.
    hold_says: String,
    /// Roster reads in a row that found nothing while the hold was open.
    misses: u8,
    /// Workers by conversation id.
    workers: HashMap<String, Worker>,
}

/// A worker on the roster.
#[derive(Debug)]
struct Worker {
    /// Set once the swarm kills it, which ends its chat.
    killed: Arc<AtomicBool>,
    /// A monitor or narrator, whose periodic pulses do not hold the swarm open.
    housekeeping: bool,
    /// What the call standing for it reads as, and what its card says it is.
    card: Option<Card>,
    /// Its transcript as last read.
    read: Option<Tail>,
}

/// The call standing for a worker on its coordinator's reply.
#[derive(Debug)]
struct Card {
    /// The subagent the call shows it as, titled by its description.
    subagent: SubAgent,
    /// How the call reads: the stand it was last published as.
    stand: Stand,
}

/// How a worker stands, as agy's board reports it (`board.StateOf`) with the failure and quiet
/// reports its dashboard puts beside it. A stand is not final: a stuck worker woken by a post
/// works again, and the reason it was stuck is part of the stand, so a different reason reads as
/// a change.
#[derive(Debug, Clone, PartialEq)]
enum Stand {
    /// Its run has not stopped.
    Working,
    /// Its run has stopped: it is parked until a post or the person wakes it.
    Waiting,
    /// Not working, and why: its newest step failed, or its transcript went quiet.
    Stuck(String),
    /// Stopped for good: the swarm killed it, it left the roster, or the run ended under it.
    Killed,
}

/// A worker's transcript as last read, and what it said.
#[derive(Debug)]
struct Tail {
    /// The transcript's length and change time when it was read.
    len: u64,
    changed: SystemTime,
    /// Its newest step ends a turn.
    ended: bool,
    /// Its newest step failed.
    failed: Option<Failed>,
}

/// A failure a worker's newest step reported.
#[derive(Debug)]
struct Failed {
    /// What the step said went wrong.
    text: String,
    /// A model error is the end of the run; a failed tool step only counts once it stays the
    /// newest step for `ERROR_SETTLE`.
    at_once: bool,
    /// Its text names a used-up quota, which needs a wait rather than a look.
    quota: bool,
}

impl Failed {
    /// What to tell the person, naming a used-up quota apart from any other failure as agy does.
    fn why(&self) -> String {
        if self.quota {
            format!("Out of quota: {}", self.text)
        } else {
            format!("Failed: {}", self.text)
        }
    }
}

impl Swarm {
    /// Notes a call of tool `name`; a swarm tool means the swarm's files are worth reading.
    pub fn note_tool(&mut self, name: &str) {
        if matches!(name, "swarm" | "post_message" | "read_board" | "swarm_wait") {
            self.running = true;
        }
    }

    /// How long to wait for output before rereading the swarm, while one runs.
    #[must_use]
    pub fn poll_every(&self) -> Option<Duration> {
        self.running.then_some(SWARM_POLL)
    }

    /// Whether workers are at work, so the agent's input stays open.
    #[must_use]
    pub fn is_holding(&self) -> bool {
        self.held
    }

    /// The transcript the chat of the worker call `tool_id` stands for follows.
    #[must_use]
    pub fn transcript(&self, tool_id: &str) -> Option<Transcript> {
        let id = tool_id.strip_prefix(SWARM_TASK_PREFIX)?;
        let worker = self.workers.get(id)?;
        Transcript::worker(id, Arc::clone(&worker.killed))
    }

    /// Rereads coordinator `session_id`'s roster: a call under `message_id` for each worker
    /// added, that call shown as its worker now stands, and the hold opened, renamed or closed.
    pub fn poll(&mut self, session_id: &str, message_id: &str) -> Vec<Parsed> {
        if !self.running {
            return Vec::new();
        }
        let Some(roster) = read_roster(session_id) else {
            return self.roster_gone();
        };
        self.misses = 0;
        let mut out = Vec::new();
        let mut working: Vec<&str> = Vec::new();
        let mut named: Vec<&str> = Vec::new();
        for (name, member) in roster["members"].as_object().into_iter().flatten() {
            let Some(id) = member["conversation_id"]
                .as_str()
                .filter(|id| is_conversation_id(id))
            else {
                continue;
            };
            let role = role_of(name, member);
            if role == "coordinator" {
                continue;
            }
            named.push(id);
            let killed = member["killed"].as_bool() == Some(true);
            let worker = match self.workers.entry(id.into()) {
                Entry::Occupied(entry) => entry.into_mut(),
                // One killed before it was seen has nothing to show.
                Entry::Vacant(_) if killed => continue,
                Entry::Vacant(entry) => entry.insert(Worker {
                    killed: Arc::new(AtomicBool::new(false)),
                    housekeeping: matches!(role.as_str(), "monitor" | "narrator"),
                    card: None,
                    read: None,
                }),
            };
            let stand = if killed {
                worker.killed.store(true, Ordering::SeqCst);
                Stand::Killed
            } else {
                worker.stand(id, member["wake_mode"].as_str().unwrap_or_default())
            };
            if stand == Stand::Working && !worker.housekeeping {
                working.push(name);
            }
            out.extend(worker.show(id, name, &role, member, stand, message_id));
        }
        // A worker the roster no longer names has gone, and its card must not read running for
        // good: its chat still follows the transcript it wrote.
        for (id, worker) in &mut self.workers {
            if !named.contains(&id.as_str()) {
                out.extend(worker.left(id, message_id));
            }
        }
        out.extend(self.hold(&working));
        out
    }

    /// Ends every worker's call still reading running, because the agent's process is gone and no
    /// more of their steps will arrive. `failed` when it went with them mid-turn; a stop by the
    /// person is not a failure.
    pub fn finish_open(&mut self, message_id: &str, failed: bool) -> Vec<Parsed> {
        let output = if failed {
            "Stopped with the agent that ran it"
        } else {
            "Stopped"
        };
        let mut out = Vec::new();
        for (id, worker) in &mut self.workers {
            let Some(card) = &mut worker.card else {
                continue;
            };
            if card.stand != Stand::Working {
                continue;
            }
            card.stand = Stand::Killed;
            let tool_id = format!("{SWARM_TASK_PREFIX}{id}");
            out.push(tool_finished(message_id, &tool_id, failed, output));
        }
        self.held = false;
        out
    }

    /// Opens, renames or closes the hold, so it stays open exactly while workers are at work.
    fn hold(&mut self, working: &[&str]) -> Vec<Parsed> {
        if working.is_empty() {
            if !self.held {
                return Vec::new();
            }
            self.held = false;
            self.hold_says.clear();
            return vec![Parsed::Task(TaskEvent::Ended {
                task_id: HOLD_TASK.into(),
                status: TaskStatus::Done,
                summary: None,
                output: None,
            })];
        }
        let says = waits_on(working);
        if !self.held {
            self.held = true;
            self.hold_says.clone_from(&says);
            return vec![Parsed::Task(hold_started(&says))];
        }
        if says == self.hold_says {
            return Vec::new();
        }
        self.hold_says.clone_from(&says);
        vec![Parsed::Task(TaskEvent::Updated {
            task_id: HOLD_TASK.into(),
            status: None,
            description: Some(says),
            backgrounded: false,
        })]
    }

    /// What to do when a running swarm's roster cannot be read: a read that caught agy writing
    /// it is worth another poll, but a roster that stays gone is a swarm that is over, so the
    /// hold must not outlive it.
    fn roster_gone(&mut self) -> Vec<Parsed> {
        if !self.held {
            return Vec::new();
        }
        self.misses += 1;
        if self.misses < ROSTER_GONE_READS {
            return Vec::new();
        }
        log::warn!(
            missing = self.misses;
            "a swarm's roster went away while its workers held the reply open: {{missing}} reads in a row"
        );
        self.running = false;
        self.hold(&[])
    }
}

impl Worker {
    /// Ends its card because the swarm's roster no longer names it, and returns nothing more.
    fn left(&mut self, id: &str, message_id: &str) -> Vec<Parsed> {
        let Some(card) = &mut self.card else {
            return Vec::new();
        };
        if card.stand == Stand::Killed {
            return Vec::new();
        }
        card.stand = Stand::Killed;
        let tool_id = format!("{SWARM_TASK_PREFIX}{id}");
        vec![tool_finished(message_id, &tool_id, false, "Left the swarm")]
    }

    /// How it stands now, read from its transcript as agy's board reads it.
    fn stand(&mut self, id: &str, wake_mode: &str) -> Stand {
        let Some((len, changed, path)) = transcript_path(id).and_then(|path| {
            let meta = std::fs::metadata(&path).ok()?;
            Some((
                meta.len(),
                meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                path,
            ))
        }) else {
            // No transcript yet: the roster alone says, exactly as agy's `statusOf` falls back to
            // it. A member parked on its board reads "mentions" or "anything"; one being started
            // reads anything else, and is working until it writes its first step.
            return if matches!(wake_mode, "mentions" | "anything") {
                Stand::Waiting
            } else {
                Stand::Working
            };
        };
        if self
            .read
            .as_ref()
            .is_none_or(|read| read.len != len || read.changed != changed)
        {
            self.read = Some(Tail::of(&path, len, changed));
        }
        let Some(read) = self.read.as_ref() else {
            return Stand::Waiting;
        };
        // A post that rings a member leaves it waiting until its run resumes, which is when its
        // transcript moves on: `wake_mode` says a post arrived, never that the member got to work.
        let working = !read.ended;
        if let Some(failed) = &read.failed
            && (failed.at_once || changed.elapsed().is_ok_and(|newest| newest >= ERROR_SETTLE))
        {
            return Stand::Stuck(failed.why());
        }
        if working && changed.elapsed().is_ok_and(|quiet| quiet > STUCK) {
            return Stand::Stuck("No steps for 10 minutes".into());
        }
        if working {
            Stand::Working
        } else {
            Stand::Waiting
        }
    }

    /// What to publish so the call standing for `name` reads as `stand`, if it reads otherwise.
    fn show(
        &mut self,
        id: &str,
        name: &str,
        role: &str,
        member: &Value,
        stand: Stand,
        message_id: &str,
    ) -> Vec<Parsed> {
        // agy re-derives a member's issue on every snapshot, so a worker failing a second way
        // reads differently: the stand carries the reason, and so the reason is part of the test.
        if self.card.as_ref().is_some_and(|card| card.stand == stand) {
            return Vec::new();
        }
        // Built from the roster each time the card changes: a worker is added once and then only
        // parks and wakes, but one renamed or re-modelled while it waits reads right too.
        let subagent = SubAgent {
            kind: if role.is_empty() { "worker" } else { role }.into(),
            description: member_title(name, role),
            model: member["model"]
                .as_str()
                .filter(|m| !m.is_empty())
                .map(String::from),
            prompt: None,
        };
        let says = subagent.description.clone();
        self.card = Some(Card {
            subagent,
            stand: stand.clone(),
        });
        let tool_id = format!("{SWARM_TASK_PREFIX}{id}");
        match stand {
            // A worker that was woken got back to work: its call reads running again, keeping
            // the started time the reply already shows for it.
            Stand::Working => vec![tool_started(
                message_id,
                ToolStart {
                    id: &tool_id,
                    name: "Agent",
                    summary: says,
                    subagent: self.card.as_ref().map(|card| card.subagent.clone()),
                    parent: None,
                },
            )],
            Stand::Waiting => vec![tool_finished(
                message_id,
                &tool_id,
                false,
                "Waiting for a post or the person",
            )],
            Stand::Stuck(why) => vec![tool_finished(message_id, &tool_id, true, &why)],
            Stand::Killed => vec![tool_finished(
                message_id,
                &tool_id,
                false,
                "Stopped by the swarm",
            )],
        }
    }
}

impl Tail {
    /// Reads the end of a transcript for what its newest step says about its worker.
    fn of(path: &Path, len: u64, changed: SystemTime) -> Self {
        let tail = tail(path).unwrap_or_default();
        Self {
            len,
            changed,
            ended: turn_ended(&tail),
            failed: newest_failure(&tail),
        }
    }
}

/// What the hold is described as while `working` workers are at work, as agy's board says which
/// member it waits on.
fn waits_on(working: &[&str]) -> String {
    const NAMED: usize = 3;
    let handles: Vec<String> = working
        .iter()
        .take(NAMED)
        .map(|name| format!("@{}", one_line(name.trim_start_matches('@'))))
        .collect();
    match working.len() {
        0 => HOLD_TITLE.into(),
        1 => format!("Waiting on {}", handles[0]),
        n if n <= NAMED => format!("Waiting on {}", handles.join(", ")),
        n => format!("Waiting on {}, and {} more", handles.join(", "), n - NAMED),
    }
}

fn hold_started(description: &str) -> TaskEvent {
    TaskEvent::Started {
        task: BackgroundTask {
            tool_id: HOLD_TASK.into(),
            task_id: Some(HOLD_TASK.into()),
            run_id: None,
            kind: "agent".into(),
            description: description.into(),
            status: TaskStatus::Running,
            started_at: now_ms(),
            summary: None,
            output: None,
            cut_off: false,
        },
        task_id: HOLD_TASK.into(),
        top_level: true,
        backgrounded: true,
    }
}

/// A member's role, or the one its name gives a monitor or narrator on older rosters.
fn role_of(name: &str, member: &Value) -> String {
    let role = member["role"].as_str().map(str::trim).unwrap_or_default();
    if !role.is_empty() && role != "worker" {
        return role.to_ascii_lowercase();
    }
    let base = name.rsplit_once('-').map_or(name, |(base, n)| {
        if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() {
            base
        } else {
            name
        }
    });
    match base {
        "monitor" | "narrator" => base.into(),
        _ => role.into(),
    }
}

fn member_title(name: &str, role: &str) -> String {
    let handle = format!("@{}", one_line(name.trim_start_matches('@')));
    if role.is_empty() || role == "worker" {
        handle
    } else {
        format!("{handle} ({role})")
    }
}

/// The swarm roster of coordinator `session_id`.
fn read_roster(session_id: &str) -> Option<Value> {
    let path = conversation_dir(session_id)?.join(".system_generated/swarm/members.json");
    let meta = std::fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_MEMBERS_FILE_BYTES {
        return None;
    }
    serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()
}

/// The complete lines at the end of a transcript, up to `MAX_TAIL_BYTES`, plus a torn last line.
fn tail(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(MAX_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_TAIL_BYTES).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // A read from the middle starts partway through a line.
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    Some(text.to_owned())
}

/// Whether a transcript's newest step ends a turn, as agy's swarm board decides it: an answer
/// that called nothing, or the results of calls that included `swarm_wait` once its own result
/// is `DONE`. Anything newer, such as a system message, user input, a model error being retried,
/// or a torn last line, means the worker is mid-turn.
fn turn_ended(tail: &str) -> bool {
    // Whether each result after the newest answer is `DONE`, newest first.
    let mut results = Vec::new();
    for line in tail.lines().rev().filter(|line| !line.trim().is_empty()) {
        let Ok(step) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        match step["type"].as_str() {
            Some("GENERIC") => {
                results.push(step["status"] == "DONE");
                continue;
            }
            Some("PLANNER_RESPONSE") => {}
            _ => return false,
        }
        let calls = step["tool_calls"].as_array().map_or(&[][..], Vec::as_slice);
        if calls.is_empty() {
            return results.is_empty();
        }
        results.reverse();
        let wait = calls.iter().position(|call| call["name"] == "swarm_wait");
        return wait.is_some_and(|wait| results.len() >= calls.len() && results[wait]);
    }
    false
}

/// The newest step of a transcript that failed, and what to say about it, as agy's `stepFailure`
/// reads it: a model error ended the worker's run where it stands, and a failed tool step only
/// counts as such once it has stayed the newest step for `ERROR_SETTLE`, since the agent normally
/// carries on after one.
/// The failure a transcript's newest step reports, if its newest step failed.
fn newest_failure(tail: &str) -> Option<Failed> {
    let step = tail
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .find_map(|line| serde_json::from_str::<Value>(line).ok())?;
    let (fallback, at_once) = match step["type"].as_str() {
        Some("ERROR_MESSAGE") => ("Model error", true),
        Some("GENERIC") if step["status"] == "ERROR" => ("Step failed", false),
        _ => return None,
    };
    let raw = step["error"]
        .as_str()
        .filter(|error| !error.trim().is_empty());
    Some(Failed {
        // The whole error is read for the quota test, which agy runs on it before any shortening:
        // a message that names the quota late is still a quota.
        quota: is_quota(raw.unwrap_or(fallback)),
        text: raw.map_or_else(|| fallback.into(), one_line),
        at_once,
    })
}

/// Formats `swarm`, `post_message`, `read_board`, and `swarm_wait` tool calls cleanly.
pub(super) fn format_swarm_tool(name: &str, args: &Value) -> Option<(&'static str, String)> {
    let clean_handle = |key: &str| {
        args[key]
            .as_str()
            .map(|s| s.trim().trim_start_matches('@'))
            .filter(|s| !s.is_empty())
    };
    let clean_channel = |key: &str| {
        args[key]
            .as_str()
            .map(|s| s.trim().trim_start_matches('#'))
            .filter(|s| !s.is_empty())
    };

    match name {
        "swarm" => {
            let action = args["action"]
                .as_str()
                .map(str::trim)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let handle = clean_handle("name");
            let summary = match action.as_str() {
                "add" => {
                    let tasks = args["tasks"].as_array().map_or(0, Vec::len);
                    let n = args["n"]
                        .as_u64()
                        .and_then(|v| usize::try_from(v).ok())
                        .unwrap_or(0);
                    let count = tasks.max(n).min(MAX_SWARM_ADD_LAUNCHES);
                    match (count > 1, handle) {
                        (true, Some(h)) => format!("Add {count} swarm workers (@{h}-1..{count})"),
                        (true, None) => format!("Add {count} swarm workers"),
                        (false, Some(h)) => format!("Add swarm worker @{h}"),
                        (false, None) => "Add swarm worker".into(),
                    }
                }
                "kill" => match handle {
                    Some(h) => format!("Stop swarm worker @{h}"),
                    None => "Stop swarm worker".into(),
                },
                "status" => "Check swarm status".into(),
                "models" => "List swarm models".into(),
                "model" => {
                    let model = args["model"].as_str().map(str::trim).unwrap_or_default();
                    match (handle, !model.is_empty()) {
                        (Some(h), true) => format!("Switch @{h} model to {model}"),
                        (Some(h), false) => format!("Switch swarm model for @{h}"),
                        (None, true) => format!("Switch swarm model to {model}"),
                        (None, false) => "Switch swarm model".into(),
                    }
                }
                "title" => match args["title"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    Some(title) => format!("Set swarm title \"{}\"", one_line(title)),
                    None => "Set swarm title".into(),
                },
                "channel" => {
                    let op = args["op"].as_str().map(str::trim).unwrap_or_default();
                    match (op.is_empty(), clean_channel("channel")) {
                        (false, Some(ch)) => format!("Swarm channel {op} #{ch}"),
                        (false, None) => format!("Swarm channel {op}"),
                        (true, _) => "Manage swarm channels".into(),
                    }
                }
                "join" => match args["swarm"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    Some(s) => format!("Join swarm {s}"),
                    None => "Join swarm".into(),
                },
                _ => "Run swarm".into(),
            };
            Some(("Swarm", summary))
        }
        "post_message" => {
            let summary = match clean_channel("channel") {
                Some(ch) if ch != "main" => format!("Post to #{ch}"),
                _ => "Post to swarm board".into(),
            };
            Some(("Swarm", summary))
        }
        "read_board" => {
            let summary = if args["status"].as_bool() == Some(true) {
                "Read swarm status".into()
            } else if let Some(ch) = clean_channel("channel")
                && ch != "main"
            {
                format!("Read #{ch}")
            } else {
                "Read swarm board".into()
            };
            Some(("Swarm", summary))
        }
        "swarm_wait" => Some(("Swarm", "Wait for swarm".into())),
        _ => None,
    }
}

/// Builds a `SubAgent` descriptor for classic `invoke_subagent` calls when present.
pub(super) fn invoke_subagent_descriptor(args: &Value) -> Option<SubAgent> {
    if let Some(role) = args["Role"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let prompt = args["Prompt"].as_str().map(String::from);
        let model = args["Model"]
            .as_str()
            .filter(|m| !m.is_empty() && *m != "inherit")
            .map(String::from);
        return Some(SubAgent {
            kind: args["TypeName"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("subagent")
                .into(),
            description: role.into(),
            model,
            prompt,
        });
    }
    let first = args["Subagents"].as_array()?.first()?;
    let role = first["Role"].as_str()?.trim();
    if role.is_empty() {
        return None;
    }
    Some(SubAgent {
        kind: "subagent".into(),
        description: role.into(),
        model: None,
        prompt: first["Prompt"].as_str().map(String::from),
    })
}
