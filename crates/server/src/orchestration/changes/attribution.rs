//! Which files a reply changed. Edit tools name their files as they go; files changed any other
//! way, such as through the shell, are found by comparing `git status` with how it stood when the
//! reply started. No platform reports which process wrote a file, so a change counts as the
//! reply's only when it happened while one of its calls that could write it ran: a read or a
//! search writes nothing, an edit only the files it names, a shell command (or a background one,
//! until it ends) anything. Edits the user makes meanwhile, and a commit, stay out of the list.
//! When two replies could claim a file, those whose call named it get it. A change no call covers
//! yet waits, since some agents report a call only once it is done.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::orchestration::publish;
use crate::Server;
use crate::orchestration::AgentEvent;
use crate::system::now_ms;
use crate::threads::{MessageStatus, Role, ToolCall};
use crate::workspace::FileChange;
use crate::watch::Change;

/// How long after a tool call returns its writes may still arrive: FSEvents reports late.
const GRACE: Duration = Duration::from_secs(1);

/// Calls that only read, as each agent's parser names them.
const READ_ONLY: [&str; 9] = [
    "Read",
    "NotebookRead",
    "Grep",
    "Glob",
    "LS",
    "WebFetch",
    "WebSearch",
    "TodoWrite",
    "BashOutput",
];
/// Calls that write only the files their summary names, comma-separated as Codex lists them.
const EDITS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

pub(in crate::orchestration) type SharedAttribution = Arc<Mutex<Attribution>>;

pub(in crate::orchestration) fn lock(
    attribution: &Mutex<Attribution>,
) -> MutexGuard<'_, Attribution> {
    attribution.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a call can write.
#[derive(Debug, Clone)]
pub(in crate::orchestration) enum Reach {
    /// These files, and only these; none for a call that only reads.
    Files(Vec<String>),
    /// Anything, as a shell command can; the command, to tell whether it names a file.
    Anything(String),
}

impl Reach {
    pub(in crate::orchestration) fn of(tool: &ToolCall) -> Self {
        let name = tool.name.as_str();
        if READ_ONLY.contains(&name) {
            Self::Files(Vec::new())
        } else if EDITS.contains(&name) && !tool.summary.is_empty() {
            Self::Files(tool.summary.split(", ").map(String::from).collect())
        } else {
            Self::Anything(tool.summary.clone())
        }
    }

    fn can_write(&self, path: &str) -> bool {
        match self {
            Self::Files(files) => files.iter().any(|f| f == path),
            Self::Anything(_) => true,
        }
    }

    /// Whether the call named `path`: an edit of it, or a command that mentions it or its name.
    fn names(&self, path: &str) -> bool {
        match self {
            Self::Files(files) => files.iter().any(|f| f == path),
            Self::Anything(command) => {
                let name = Path::new(path).file_name().map(|n| n.to_string_lossy());
                command.contains(path) || name.is_some_and(|n| command.contains(n.as_ref()))
            }
        }
    }
}

/// How a call's window bears on a change at some time.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Cover {
    No,
    Yes,
    /// By a call that named the file.
    Named,
}

/// A change one reply could claim from a burst.
pub(in crate::orchestration) struct Candidate {
    path: String,
    times: Vec<Instant>,
    cover: Cover,
}

/// What a reply under way has to go on to claim the files it changed.
pub(in crate::orchestration) struct Attribution {
    /// `git status` when the reply started.
    baseline: Vec<FileChange>,
    /// Calls under way, by id, with when each started and what it can write. A background
    /// command counts until it ends.
    running: HashMap<String, (Instant, Reach)>,
    /// When finished calls ran, each with `GRACE` added at its end, and what they could write.
    ran: Vec<(Range<Instant>, Reach)>,
    /// Paths seen changing while none of the reply's known calls that could write them ran,
    /// with when. A call reported later may still cover them.
    pending: HashMap<String, Vec<Instant>>,
    /// Paths another reply's call named, which this reply leaves to it.
    taken: HashSet<String>,
}

impl Attribution {
    pub(in crate::orchestration) fn shared(baseline: Vec<FileChange>) -> SharedAttribution {
        Arc::new(Mutex::new(Self::new(baseline)))
    }

    fn new(baseline: Vec<FileChange>) -> Self {
        Self {
            baseline,
            running: HashMap::new(),
            ran: Vec::new(),
            pending: HashMap::new(),
            taken: HashSet::new(),
        }
    }

    /// `started_at` is when the call started, in ms since the epoch, which for an agent that
    /// reports calls once done is before now. Codex reports a call when it starts and again when
    /// it completes; the first start counts.
    pub(in crate::orchestration) fn tool_started(
        &mut self,
        id: &str,
        started_at: Option<u64>,
        reach: Reach,
    ) {
        let now = Instant::now();
        let start = started_at
            .and_then(|ms| now.checked_sub(Duration::from_millis(now_ms().saturating_sub(ms))))
            .unwrap_or(now);
        self.running.entry(id.to_string()).or_insert((start, reach));
    }

    /// Ends a call's window and returns the waiting paths it covers, which are the reply's.
    pub(in crate::orchestration) fn tool_finished(&mut self, id: &str) -> Vec<String> {
        let Some((start, reach)) = self.running.remove(id) else {
            return Vec::new();
        };
        self.ran.push((start..Instant::now() + GRACE, reach));
        let covered: Vec<String> = self
            .pending
            .iter()
            .filter(|(path, times)| times.iter().any(|&at| self.cover(path, at) > Cover::No))
            .map(|(path, _)| path.clone())
            .collect();
        covered.iter().for_each(|path| {
            self.pending.remove(path);
        });
        covered
    }

    fn cover(&self, path: &str, at: Instant) -> Cover {
        let running = self
            .running
            .values()
            .filter(|(start, _)| *start <= at)
            .map(|(_, r)| r);
        let ran = self
            .ran
            .iter()
            .filter(|(span, _)| span.contains(&at))
            .map(|(_, r)| r);
        running
            .chain(ran)
            .filter(|reach| reach.can_write(path))
            .map(|reach| {
                if reach.names(path) {
                    Cover::Named
                } else {
                    Cover::Yes
                }
            })
            .max()
            .unwrap_or(Cover::No)
    }

    /// The changes in a settled burst this reply could claim: those that differ from the
    /// baseline and are not `listed` yet, with whether one of its calls covers each.
    fn candidates(
        &self,
        changes: &[FileChange],
        burst: &[Change],
        listed: &[String],
    ) -> Vec<Candidate> {
        let mut found = Vec::new();
        for path in changed_paths(&self.baseline, changes) {
            if listed.contains(&path) || self.taken.contains(&path) {
                continue;
            }
            let own: Vec<Instant> = burst
                .iter()
                .filter(|c| covers_path(&c.path, &path))
                .map(|c| c.at)
                .collect();
            // A path with no event of its own, such as a file a commit took out of the list,
            // goes by the burst's other events, such as `.git/index`, unless it already waits.
            let times = if !own.is_empty() {
                own
            } else if self.pending.contains_key(&path) {
                continue;
            } else {
                burst.iter().map(|c| c.at).collect()
            };
            let cover = times
                .iter()
                .map(|&at| self.cover(&path, at))
                .max()
                .unwrap_or(Cover::No);
            found.push(Candidate { path, times, cover });
        }
        found
    }

    /// Records what became of a candidate: claimed, waiting, or left to another reply.
    fn resolve(&mut self, candidate: Candidate, outcome: Outcome) {
        match outcome {
            Outcome::Claimed => {
                self.pending.remove(&candidate.path);
            }
            Outcome::Waits => {
                self.pending
                    .entry(candidate.path)
                    .or_default()
                    .extend(candidate.times);
            }
            Outcome::Taken => {
                self.pending.remove(&candidate.path);
                self.taken.insert(candidate.path);
            }
        }
    }

    /// At the end of the reply: what it changed since the baseline, without paths only seen
    /// changing while none of its calls that could write them ran, or that another reply's call
    /// named. A path the watcher never saw, as in a project that is not the open one, counts.
    /// Starts over from `after` for the next reply.
    pub(in crate::orchestration) fn settle(&mut self, after: Vec<FileChange>) -> Vec<String> {
        let paths = changed_paths(&self.baseline, &after)
            .into_iter()
            .filter(|path| !self.pending.contains_key(path) && !self.taken.contains(path))
            .collect();
        *self = Self::new(after);
        paths
    }
}

#[derive(Clone, Copy)]
enum Outcome {
    Claimed,
    Waits,
    Taken,
}

/// Whether an event at `event_path` stands for a change to `path`: the path itself, or a folder
/// above it, as when a whole folder is removed.
fn covers_path(event_path: &str, path: &str) -> bool {
    path.strip_prefix(event_path)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Paths whose git status or line counts differ between two snapshots, and paths that left the
/// list: a deleted untracked file, or a change undone.
pub(in crate::orchestration) fn changed_paths(
    before: &[FileChange],
    after: &[FileChange],
) -> Vec<String> {
    let key = |c: &FileChange| (c.status.clone(), c.additions, c.deletions);
    let changed = after.iter().filter(|a| {
        before
            .iter()
            .find(|b| b.path == a.path)
            .is_none_or(|b| key(b) != key(a))
    });
    let gone = before
        .iter()
        .filter(|b| !after.iter().any(|a| a.path == b.path));
    changed.chain(gone).map(|c| c.path.clone()).collect()
}

impl Server {
    /// Adds what a settled burst of changes in the project changed to each reply under way
    /// there, so a reply lists its files as they change, deletions made through the shell
    /// included. `changes` is the project's `git status` after the burst. A file two replies
    /// could claim goes to those whose call named it, and to all of them when none did.
    pub(crate) fn note_project_changes(
        &self,
        project_id: &str,
        changes: &[FileChange],
        burst: &[Change],
    ) {
        let mut replies = Vec::new();
        for (thread_id, attribution) in self.runner.attributions() {
            let Some(thread) = self.store.thread(&thread_id) else {
                continue;
            };
            if thread.project_id.as_deref() != Some(project_id) {
                continue;
            }
            let reply = thread
                .messages
                .iter()
                .rev()
                .find(|m| m.role == Role::Assistant && m.status == MessageStatus::Streaming);
            let Some(reply) = reply else { continue };
            let candidates = lock(&attribution).candidates(changes, burst, &reply.files);
            replies.push((thread_id, reply.id.clone(), attribution, candidates));
        }
        let named: HashSet<String> = replies
            .iter()
            .flat_map(|(.., candidates)| candidates)
            .filter(|c| c.cover == Cover::Named)
            .map(|c| c.path.clone())
            .collect();
        for (thread_id, message_id, attribution, candidates) in replies {
            let mut claimed = Vec::new();
            let mut attribution = lock(&attribution);
            for candidate in candidates {
                let outcome = match candidate.cover {
                    Cover::No => Outcome::Waits,
                    Cover::Yes if named.contains(&candidate.path) => Outcome::Taken,
                    Cover::Yes | Cover::Named => Outcome::Claimed,
                };
                if matches!(outcome, Outcome::Claimed) {
                    claimed.push(candidate.path.clone());
                }
                attribution.resolve(candidate, outcome);
            }
            drop(attribution);
            for path in claimed {
                let touched = AgentEvent::FileTouched {
                    message_id: message_id.clone(),
                    path,
                };
                publish(self, &thread_id, touched);
            }
        }
    }
}
