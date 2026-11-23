//! Which files a reply changed. Edit tools name their files as they go; files changed any other
//! way, such as through the shell, are found by comparing `git status` with how it stood when the
//! reply started. No platform reports which process wrote a file, so a change counts as the
//! reply's only when it happened while one of its calls that could write it ran: a read or a
//! search writes nothing, an edit only the files it names, a shell command (or a background one,
//! until it ends) anything. Edits the user makes meanwhile, and a commit, stay out of the list.
//! When two replies could claim a file, those whose call named it get it, and a file both could
//! have written with neither naming it goes to neither: one file in two replies' lists is worse
//! than a file in none, since the Changes view still lists it. A change no call covers yet waits,
//! since some agents report a call only once it is done.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::orchestration::publish;
use crate::Core;
use crate::events::AgentEvent;
use crate::system::now_ms;
use crate::chats::{MessageStatus, Role, ToolCall};
use crate::source_control::{self, FileChange};
use crate::projects::watch::Change;

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
    /// When the reply started, in seconds since the epoch, to find the commits it made.
    since: u64,
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
            since: now_ms() / 1000,
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
        for path in &covered {
            self.pending.remove(path);
        }
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

    /// At the end of the reply: what it changed since the baseline, and what the commits made
    /// at `root` while it ran changed, which `git status` no longer shows, without paths only seen
    /// changing while none of its calls that could write them ran, or that another reply's call
    /// named. A path the watcher never saw, as in a project that is not the open one, counts.
    /// Starts over from `after` for the next reply.
    pub(in crate::orchestration) fn settle(
        &mut self,
        root: &str,
        after: Vec<FileChange>,
    ) -> Vec<String> {
        let mut paths = changed_paths(&self.baseline, &after);
        for path in source_control::committed_since(root, self.since) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        let paths = paths
            .into_iter()
            .filter(|path| !self.pending.contains_key(path) && !self.taken.contains(path))
            .collect();
        self.restart(after);
        paths
    }

    /// Starts over from `after`, the files as they are now, for the next reply.
    pub(in crate::orchestration) fn restart(&mut self, after: Vec<FileChange>) {
        *self = Self::new(after);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// What each reply may claim from one burst of changes. A path a reply's call named is that
/// reply's, whoever else could have written it. A path more than one reply could have written,
/// with none of them naming it, is nobody's: which reply wrote it is a coin toss, and a file
/// listed under the wrong reply reads as that reply's work.
fn claims(replies: &[&[Candidate]]) -> Vec<Vec<Outcome>> {
    let mut named: HashSet<&str> = HashSet::new();
    let mut claimants: HashMap<&str, usize> = HashMap::new();
    for candidates in replies {
        for candidate in *candidates {
            match candidate.cover {
                Cover::Named => {
                    named.insert(&candidate.path);
                }
                Cover::Yes => *claimants.entry(&candidate.path).or_default() += 1,
                Cover::No => {}
            }
        }
    }
    replies
        .iter()
        .map(|candidates| {
            candidates
                .iter()
                .map(|candidate| match candidate.cover {
                    Cover::No => Outcome::Waits,
                    Cover::Yes if named.contains(candidate.path.as_str()) => Outcome::Taken,
                    Cover::Yes
                        if claimants.get(candidate.path.as_str()).copied().unwrap_or(0) > 1 =>
                    {
                        Outcome::Taken
                    }
                    Cover::Named | Cover::Yes => Outcome::Claimed,
                })
                .collect()
        })
        .collect()
}

impl Core {
    /// Adds what a settled burst of changes in `folder`, the project's own or a task's worktree,
    /// changed to each reply under way there, so a reply lists its files as they change,
    /// deletions made through the shell included. `changes` is the folder's `git status` after
    /// the burst. A reply working in another folder of the project is left out: its baseline is
    /// that folder's status, so the two would read as changes it made. A file two replies could
    /// claim goes to those whose call named it, and to neither when none did.
    pub(crate) fn note_project_changes(
        &self,
        project_id: &str,
        folder: &str,
        changes: &[FileChange],
        burst: &[Change],
    ) {
        let mut replies = Vec::new();
        for (chat_id, root, attribution) in self.runner.attributions() {
            if root != folder {
                continue;
            }
            let Some(chat) = self.store.chat(&chat_id) else {
                continue;
            };
            if chat.project_id.as_deref() != Some(project_id) {
                continue;
            }
            let reply = chat
                .messages
                .iter()
                .rev()
                .find(|m| m.role == Role::Assistant && m.status == MessageStatus::Streaming);
            let Some(reply) = reply else { continue };
            let candidates = attribution
                .locked()
                .candidates(changes, burst, &reply.files);
            replies.push((chat_id, reply.id.clone(), attribution, candidates));
        }
        let decided = claims(
            &replies
                .iter()
                .map(|(.., candidates)| candidates.as_slice())
                .collect::<Vec<_>>(),
        );
        for ((chat_id, message_id, attribution, candidates), outcomes) in
            replies.into_iter().zip(decided)
        {
            let mut claimed = Vec::new();
            let mut attribution = attribution.locked();
            for (candidate, outcome) in candidates.into_iter().zip(outcomes) {
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
                publish(self, &chat_id, touched);
            }
        }
    }
}
