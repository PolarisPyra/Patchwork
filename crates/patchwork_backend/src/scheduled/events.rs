//! Tasks started by an event in their project's GitHub repository: a pull request opening, or
//! a GitHub Actions run failing. The repository is asked through `gh` at most once a minute,
//! and each event starts the task once. The events already there when a task is first watched
//! are only noted, so saving a task does not start it for every open pull request.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Deserialize;

use super::types::{Task, Trigger};
use crate::chats::Chat;
use crate::system::now_ms;
use crate::{Core, Error};

/// How often the repositories are asked for new events.
const POLL_EVERY_MS: u64 = 60_000;

/// When the repositories were last asked, in the app state.
const POLLED_KEY: &str = "scheduled_events_polled";

/// The events each task has seen, in the app state, by kind and task id.
const SEEN_KEY: &str = "scheduled_seen:";

/// How many seen events each task remembers.
const SEEN_KEPT: usize = 300;

/// How many runs one task starts on a single poll, so a burst of events does not start dozens.
const RUNS_PER_POLL: usize = 3;

/// An event in a repository, as it starts a task.
struct Event {
    /// What tells it apart from the repository's other events of its kind.
    id: String,
    /// What the run is told about it.
    detail: String,
}

/// An open pull request, as `gh pr list --json` gives it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequest {
    number: u64,
    title: String,
    url: String,
    #[serde(default)]
    head_ref_name: String,
    #[serde(default)]
    author: Option<Author>,
}

#[derive(Deserialize)]
struct Author {
    login: String,
}

/// A failed workflow run, as `gh run list --json` gives it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowRun {
    database_id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    display_title: String,
    url: String,
    #[serde(default)]
    head_branch: String,
}

impl Core {
    /// Starts the tasks whose event has come in their repository since the last poll, and
    /// returns their names. Asks GitHub at most once a minute; called on every scheduler tick.
    pub(super) fn run_event_tasks(self: &Arc<Self>, tasks: &[Task]) -> Vec<String> {
        let watching: Vec<&Task> = tasks
            .iter()
            .filter(|t| {
                t.enabled && matches!(t.trigger, Trigger::PullRequestOpened | Trigger::CheckFailed)
            })
            .collect();
        if watching.is_empty() {
            return Vec::new();
        }
        let now = now_ms();
        let polled: u64 = self
            .store
            .state(POLLED_KEY)
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        if now.saturating_sub(polled) < POLL_EVERY_MS {
            return Vec::new();
        }
        if let Err(e) = self.store.set_state(POLLED_KEY, &now.to_string()) {
            log::warn!(error:% = e; "could not note the repositories were asked: {{error}}");
        }
        // Tasks of one project and kind share one answer from GitHub.
        let mut asked: HashMap<(String, &'static str), Option<Vec<Event>>> = HashMap::new();
        let mut started = Vec::new();
        for task in watching {
            let Some(project) = task.project_id.as_deref() else {
                continue;
            };
            let Ok(root) = self.project_root(project) else {
                continue;
            };
            let kind = kind(&task.trigger);
            let events = asked.entry((root.clone(), kind)).or_insert_with(|| {
                match fetch(&root, &task.trigger) {
                    Ok(events) => Some(events),
                    Err(e) => {
                        log::warn!(
                            task:% = task.name, error:% = e;
                            "scheduled task {{task}} could not ask GitHub: {{error}}"
                        );
                        None
                    }
                }
            });
            let Some(events) = events else {
                continue;
            };
            for event in self.new_events(task, kind, events) {
                match self.start_on_event(task, &event.detail) {
                    Ok(_) => started.push(task.name.clone()),
                    Err(e) => log::warn!(
                        task:% = task.name, error:% = e;
                        "scheduled task {{task}} did not start: {{error}}"
                    ),
                }
            }
        }
        started
    }

    /// The events of `events` that `task` has not seen, newest few first, noting them all as
    /// seen. None the first time the task looks, so what is already there does not start it.
    fn new_events<'a>(&self, task: &Task, kind: &str, events: &'a [Event]) -> Vec<&'a Event> {
        let key = format!("{SEEN_KEY}{kind}:{}", task.id);
        let seen: Option<Vec<String>> = self
            .store
            .state(&key)
            .and_then(|s| serde_json::from_str(&s).ok());
        let fresh: Vec<&Event> = match &seen {
            Some(seen) => {
                let seen: HashSet<&str> = seen.iter().map(String::as_str).collect();
                events
                    .iter()
                    .filter(|e| !seen.contains(e.id.as_str()))
                    .take(RUNS_PER_POLL)
                    .collect()
            }
            None => Vec::new(),
        };
        let mut kept: Vec<String> = events.iter().map(|e| e.id.clone()).collect();
        for id in seen.into_iter().flatten() {
            if kept.len() >= SEEN_KEPT {
                break;
            }
            if !kept.contains(&id) {
                kept.push(id);
            }
        }
        let kept = serde_json::to_string(&kept).unwrap_or_else(|_| "[]".into());
        if let Err(e) = self.store.set_state(&key, &kept) {
            log::warn!(error:% = e; "could not note the events a task has seen: {{error}}");
        }
        fresh
    }

    /// Starts `task` with `detail` about the event that started it after its prompt, and
    /// records the run.
    pub(super) fn start_on_event(
        self: &Arc<Self>,
        task: &Task,
        detail: &str,
    ) -> Result<Chat, Error> {
        let run = Task {
            prompt: format!("{}\n\n{detail}", task.prompt.trim_end()),
            ..task.clone()
        };
        let now = now_ms() as i64;
        let started = self.start_scheduled_task(&run);
        let outcome = match &started {
            Ok(chat) => Ok(chat.id.as_str()),
            Err(e) => Err(e.to_string()),
        };
        let recorded = match &outcome {
            Ok(id) => self.store.scheduled_task_ran(&task.id, now, None, Ok(id)),
            Err(why) => self
                .store
                .scheduled_task_ran(&task.id, now, None, Err(why.as_str())),
        };
        if let Err(e) = recorded {
            log::warn!(task:% = task.id, error:% = e; "scheduled task {{task}}: {{error}}");
        }
        started
    }
}

/// The name a trigger's seen events are kept under.
fn kind(trigger: &Trigger) -> &'static str {
    match trigger {
        Trigger::CheckFailed => "check",
        _ => "pr",
    }
}

/// The repository's events of the kind `trigger` watches, newest first.
fn fetch(root: &str, trigger: &Trigger) -> Result<Vec<Event>, Error> {
    if matches!(trigger, Trigger::CheckFailed) {
        let out = crate::source_control::gh(
            root,
            &[
                "run",
                "list",
                "--status",
                "failure",
                "--limit",
                "20",
                "--json",
                "databaseId,name,displayTitle,url,headBranch",
            ],
        )?;
        let runs: Vec<WorkflowRun> = serde_json::from_str(&out)
            .map_err(|e| format!("GitHub's list of runs could not be read: {e}"))?;
        return Ok(runs
            .into_iter()
            .map(|r| Event {
                id: r.database_id.to_string(),
                detail: format!(
                    "This run was started because a check failed in the repository.\n\
                     Check: {}\nRun: {}\nBranch: {}\nLink: {}",
                    r.name, r.display_title, r.head_branch, r.url
                ),
            })
            .collect());
    }
    let out = crate::source_control::gh(
        root,
        &[
            "pr",
            "list",
            "--state",
            "open",
            "--limit",
            "30",
            "--json",
            "number,title,url,headRefName,author",
        ],
    )?;
    let prs: Vec<PullRequest> = serde_json::from_str(&out)
        .map_err(|e| format!("GitHub's list of pull requests could not be read: {e}"))?;
    Ok(prs
        .into_iter()
        .map(|p| Event {
            id: p.number.to_string(),
            detail: format!(
                "This run was started because a pull request opened in the repository.\n\
                 Pull request: #{} {}\nAuthor: {}\nBranch: {}\nLink: {}",
                p.number,
                p.title,
                p.author.map_or_else(|| "unknown".into(), |a| a.login),
                p.head_ref_name,
                p.url
            ),
        })
        .collect())
}
