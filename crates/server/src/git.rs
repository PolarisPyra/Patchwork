//! Source Control git operations: commits, pushes, pull requests, and agent prompts.
//!
//! Git from the Source Control view, with the agents in it: committing, pushing, and opening
//! pull requests; which thread and replies each commit came from; and the prompts that hand a
//! pull request's feedback, or a merge's conflicts, to an agent. A message the user leaves
//! empty, and a pull request's text, are written by the model that writes titles.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::fmt::Write as _;
use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Server;
use crate::threads::Role;
use crate::threads::title::{self, Request, Writer};
use crate::workspace::{self, CheckState, CommitReply, GitCommit, PullRequest};

/// The `app_state` key of what was recorded about a commit Patchwork made, before its hash.
const COMMIT_THREAD: &str = "commit-thread:";
/// The `app_state` key of a thread's newest reply that went into a commit, before its id.
const LAST_COMMITTED: &str = "thread-committed:";

/// Failed jobs whose logs a pull request's feedback quotes, and how much of each log's end.
const FAILED_JOBS: usize = 4;
const LOG_TAIL: usize = 6_000;
/// How much of each conflicted file's conflicts the prompt quotes, and of all of them.
const FILE_CONFLICTS: usize = 4_000;
const ALL_CONFLICTS: usize = 24_000;

/// What was recorded about a commit made from a thread.
#[derive(Serialize, Deserialize)]
struct CommitRecord {
    thread: String,
    replies: Vec<CommitReply>,
}

impl Server {
    /// Stages every change and commits it with `message`, or with one a model writes from the
    /// staged patch when `message` is blank. The commit is recorded as the open thread's, with
    /// its replies whose changes it took in, for the graph. Returns the commit's short hash and
    /// its subject.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when `git` fails to stage or commit, or when
    /// `message` is blank and no model writes one.
    pub fn commit_changes(
        &self,
        project_id: &str,
        thread_id: Option<&str>,
        message: &str,
    ) -> Result<(String, String), Error> {
        let root = self.project_root(project_id)?;
        workspace::stage_all(&root)?;
        let message = match message.trim() {
            "" => self.staged_message(&root, thread_id)?,
            typed => typed.to_string(),
        };
        let hash = workspace::commit(&root, &message)?;
        if let Some(thread_id) = thread_id
            && let Err(e) = self.record_commit(&root, thread_id, &hash)
        {
            log::warn!(
                hash:%, error:% = e;
                "could not record the thread of commit {{hash}}: {{error}}"
            );
        }
        let subject = message.lines().next().unwrap_or_default().to_string();
        Ok((hash.chars().take(7).collect(), subject))
    }

    /// Records that `hash` was committed from the thread, with the replies since its last
    /// commit that changed a file the commit has.
    fn record_commit(&self, root: &str, thread_id: &str, hash: &str) -> Result<(), Error> {
        let files = workspace::run_git(
            root,
            &["show", "--name-only", "--relative", "--format=", hash],
        )?;
        let files: HashSet<&str> = files.lines().collect();
        let last = self.store.state(&format!("{LAST_COMMITTED}{thread_id}"));
        let replies = self
            .store
            .thread(thread_id)
            .map(|thread| {
                let messages = &thread.messages;
                let after = last
                    .and_then(|id| messages.iter().position(|m| m.id == id))
                    .map_or(0, |i| i + 1);
                (after..messages.len())
                    .filter(|&i| {
                        let m = &messages[i];
                        m.role == Role::Assistant
                            && m.turn.is_some()
                            && m.files.iter().any(|f| files.contains(f.as_str()))
                    })
                    .map(|i| CommitReply {
                        message_id: messages[i].id.clone(),
                        prompt: messages[..i]
                            .iter()
                            .rev()
                            .find(|m| m.role == Role::User)
                            .and_then(|m| m.text.lines().find(|l| !l.trim().is_empty()))
                            .map(|l| l.trim().chars().take(80).collect())
                            .unwrap_or_default(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Some(newest) = replies.last() {
            self.store
                .set_state(&format!("{LAST_COMMITTED}{thread_id}"), &newest.message_id)?;
        }
        let record = CommitRecord {
            thread: thread_id.to_string(),
            replies,
        };
        let record = serde_json::to_string(&record).map_err(|e| e.to_string())?;
        self.store
            .set_state(&format!("{COMMIT_THREAD}{hash}"), &record)
    }

    /// What was recorded about a commit Patchwork made from a thread.
    fn commit_record(&self, hash: &str) -> Option<CommitRecord> {
        let saved = self.store.state(&format!("{COMMIT_THREAD}{hash}"))?;
        // The first builds kept only the thread's id.
        Some(serde_json::from_str(&saved).unwrap_or(CommitRecord {
            thread: saved,
            replies: Vec::new(),
        }))
    }

    /// The branch's and its upstream's commits, each with the thread Patchwork committed it
    /// from and that thread's replies in it, if any.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown or `git log` fails.
    pub fn git_log(
        &self,
        project_id: &str,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<GitCommit>, Error> {
        let root = self.project_root(project_id)?;
        let mut commits = workspace::git_log(&root, skip, limit)?;
        for commit in &mut commits {
            if let Some(record) = self.commit_record(&commit.hash) {
                commit.thread_id = Some(record.thread);
                commit.replies = record.replies;
            }
        }
        Ok(commits)
    }

    /// A prompt with what the branch's open pull request needs: its failed checks with the end
    /// of each failed job's log, and its reviews and comments. Returned with the thread that
    /// made the branch's newest commit Patchwork recorded, to send it to.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when `gh` fails, or when the branch has no
    /// open pull request.
    pub fn pull_request_feedback(
        &self,
        project_id: &str,
    ) -> Result<(Option<String>, String), Error> {
        let root = self.project_root(project_id)?;
        let pr = workspace::pull_request_view(&root)?;
        let number = pr["number"].as_u64().unwrap_or_default();
        let mut sections = Vec::new();

        let failed: Vec<&Value> = workspace::pull_request_checks(&pr)
            .filter(|c| workspace::check_state(c) == CheckState::Failed)
            .collect();
        if !failed.is_empty() {
            let mut text = String::from("## Failed checks\n");
            for check in failed.iter().take(FAILED_JOBS) {
                let name = check["name"].as_str().or(check["context"].as_str());
                let _ = write!(text, "\n### {}\n", name.unwrap_or("Check"));
                let url = check["detailsUrl"].as_str().or(check["targetUrl"].as_str());
                let log = url
                    .and_then(job_id)
                    .and_then(|job| {
                        workspace::gh(&root, &["run", "view", "--job", job, "--log-failed"]).ok()
                    })
                    .filter(|log| !log.trim().is_empty());
                match (log, url) {
                    (Some(log), _) => {
                        let _ = write!(text, "```\n{}\n```\n", log_tail(&log));
                    }
                    (None, Some(url)) => {
                        let _ = writeln!(text, "Its log is at {url}");
                    }
                    (None, None) => text.push_str("It left no log.\n"),
                }
            }
            sections.push(text);
        }

        let mut said = Vec::new();
        for review in pr["reviews"].as_array().into_iter().flatten() {
            let body = review["body"].as_str().unwrap_or_default().trim();
            let state = match review["state"].as_str() {
                Some("CHANGES_REQUESTED") => " (changes requested)",
                Some("APPROVED") => " (approved)",
                _ => "",
            };
            if !body.is_empty() || !state.is_empty() {
                said.push(format!("- **{}**{state}: {body}", login(&review["author"])));
            }
        }
        let inline = format!("repos/{{owner}}/{{repo}}/pulls/{number}/comments?per_page=100");
        let inline = workspace::gh(&root, &["api", &inline])
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        for comment in inline
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let line = comment["line"]
                .as_u64()
                .or(comment["original_line"].as_u64());
            let place = match (comment["path"].as_str(), line) {
                (Some(path), Some(line)) => format!(" on `{path}:{line}`"),
                (Some(path), None) => format!(" on `{path}`"),
                _ => String::new(),
            };
            let body = comment["body"].as_str().unwrap_or_default().trim();
            said.push(format!("- **{}**{place}: {body}", login(&comment["user"])));
        }
        for comment in pr["comments"].as_array().into_iter().flatten() {
            let body = comment["body"].as_str().unwrap_or_default().trim();
            said.push(format!("- **{}**: {body}", login(&comment["author"])));
        }
        if !said.is_empty() {
            sections.push(format!("## Reviews and comments\n\n{}", said.join("\n")));
        }

        if sections.is_empty() {
            return Err("Nothing to send: no check failed and nobody commented".into());
        }
        let url = pr["url"].as_str().unwrap_or_default();
        let prompt = format!(
            "Pull request #{number} ({url}) needs work. Fix what its failed checks and its \
             reviewers point out, run the checks again where you can, and reply with what you \
             changed. Where a comment asks for something you think is wrong, say why instead of \
             changing it.\n\n{}",
            sections.join("\n\n")
        );
        let thread = workspace::base_ref(&root)
            .and_then(|base| {
                workspace::run_git(&root, &["rev-list", &format!("{base}..HEAD")]).ok()
            })
            .and_then(|hashes| {
                hashes
                    .lines()
                    .find_map(|hash| self.commit_record(hash))
                    .map(|record| record.thread)
            });
        Ok((thread, workspace::truncated(&prompt, 40_000)))
    }

    /// A prompt for an agent to resolve the project's merge conflicts: what git stopped in, the
    /// two sides, and each conflicted file's conflicts as they stand.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when `git` fails, or when no file has a
    /// conflict.
    pub fn conflict_prompt(&self, project_id: &str) -> Result<String, Error> {
        let root = self.project_root(project_id)?;
        let files = workspace::run_git(
            &root,
            &["diff", "--name-only", "--relative", "--diff-filter=U"],
        )?;
        let files: Vec<&str> = files.lines().filter(|f| !f.is_empty()).collect();
        if files.is_empty() {
            return Err("No file has a conflict to resolve".into());
        }
        let describe = |rev: &str| {
            workspace::run_git(&root, &["log", "-1", "--format=%h %s", rev])
                .map_or_else(|_| rev.to_string(), |s| s.trim().to_string())
        };
        let has =
            |rev: &str| workspace::run_git(&root, &["rev-parse", "--verify", "-q", rev]).is_ok();
        let (operation, theirs) = [
            ("MERGE_HEAD", "a merge"),
            ("REBASE_HEAD", "a rebase"),
            ("CHERRY_PICK_HEAD", "a cherry-pick"),
            ("REVERT_HEAD", "a revert"),
        ]
        .into_iter()
        .find(|(rev, _)| has(rev))
        .map_or_else(
            || ("applying changes".to_string(), None),
            |(rev, what)| (what.to_string(), Some(describe(rev))),
        );
        let branch = workspace::git_branch(&root).unwrap_or_else(|| "HEAD".into());
        let mut sides = format!("- Ours: {branch} at {}", describe("HEAD"));
        if let Some(theirs) = theirs {
            let _ = write!(sides, "\n- Theirs: {theirs}");
        }

        let mut conflicts = String::new();
        for file in &files {
            let text = match crate::system::remote::split(&root) {
                Some(at) => crate::system::remote::fs::read_inside(at.host, at.path, file, 1 << 20)
                    .map(|b| String::from_utf8_lossy(&b).into_owned()),
                None => std::fs::read_to_string(Path::new(&root).join(file)).map_err(Error::from),
            }
            .unwrap_or_else(|e| {
                log::warn!(error:% = e; "could not read a conflicted file: {{error}}");
                String::new()
            });
            let hunks = conflict_hunks(&text);
            let body = if hunks.is_empty() {
                "No conflict markers: one side deleted or renamed it.".to_string()
            } else {
                format!("```\n{}\n```", workspace::truncated(&hunks, FILE_CONFLICTS))
            };
            let _ = write!(conflicts, "\n### {file}\n{body}\n");
        }
        let prompt = format!(
            "Git stopped during {operation} with conflicts in {} file(s). Resolve them.\n\n{sides}\n\n\
             For each conflict, keep what both sides meant to do; where they truly clash, choose \
             and say why. Remove every conflict marker, check the project still builds and its \
             checks pass where you can run them, then stage each resolved file with git add. \
             Don't commit, and don't continue or abort the {}: I'll review it and finish it.\n\n\
             ## Conflicts\n{}",
            files.len(),
            operation.trim_start_matches("a "),
            workspace::truncated(&conflicts, ALL_CONFLICTS)
        );
        Ok(prompt)
    }

    /// Stages every change and has a model write a commit message for it, to edit before
    /// committing.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when `git` fails to stage the changes or read
    /// them, or when no model writes a message.
    pub fn write_commit_message(
        &self,
        project_id: &str,
        thread_id: Option<&str>,
    ) -> Result<String, Error> {
        let root = self.project_root(project_id)?;
        workspace::stage_all(&root)?;
        self.staged_message(&root, thread_id)
    }

    fn staged_message(&self, root: &str, thread_id: Option<&str>) -> Result<String, Error> {
        let summary = workspace::staged_summary(root)?;
        self.write(thread_id, COMMIT_PROMPT, COMMIT_SCHEMA, &summary)
            .and_then(|answer| commit_message(&answer))
            .ok_or_else(|| "No model wrote a commit message. Type one to commit.".into())
    }

    /// Opens a pull request for the pushed branch into the default branch, with a title and
    /// body a model writes from its commits, or gh's own from the commits when none answers.
    /// An open one is returned as it is.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when `git` cannot summarize the branch, or
    /// when `gh` fails to create the pull request.
    pub fn open_pull_request(
        &self,
        project_id: &str,
        thread_id: Option<&str>,
    ) -> Result<PullRequest, Error> {
        let root = self.project_root(project_id)?;
        let (base, summary) = workspace::branch_summary(&root)?;
        let text = self
            .write(thread_id, PR_PROMPT, PR_SCHEMA, &summary)
            .and_then(|answer| pull_request_text(&answer));
        let text = text.as_ref().map(|(t, b)| (t.as_str(), b.as_str()));
        workspace::create_pull_request(&root, &base, text)
    }

    /// Asks the title writer from Settings, else the open thread's provider and model.
    fn write(
        &self,
        thread_id: Option<&str>,
        prompt: &str,
        schema: &str,
        input: &str,
    ) -> Option<Value> {
        let tokens = self.store.settings().tokens;
        let writer = Writer {
            model: tokens.title_model,
            local_only: tokens.titles_local_only,
        };
        let thread = thread_id.and_then(|id| self.store.thread(id));
        let request = Request {
            prompt,
            schema,
            label: "Changes",
            input,
        };
        title::ask(
            &request,
            thread.as_ref().map(|t| t.agent),
            thread.as_ref().and_then(|t| t.model.as_deref()),
            &writer,
        )
    }
}

const COMMIT_PROMPT: &str = "You write concise git commit messages.
Return JSON with the keys subject and body.
Rules:
- subject is imperative, at most 72 characters, with no trailing period
- body is an empty string or a few short bullet points
- capture the primary user-visible or developer-visible change";

const COMMIT_SCHEMA: &str = r#"{"type":"object","properties":{"subject":{"type":"string"},"body":{"type":"string"}},"required":["subject","body"],"additionalProperties":false}"#;

const PR_PROMPT: &str = "You write pull request descriptions.
This is a new writing task, not a continuation of any conversation.
Return JSON with the keys title and body.
Rules:
- title is concise and specific
- body is Markdown with the headings '## Summary' and '## Testing'
- under Summary, short bullet points
- under Testing, bullet points with concrete checks, or 'Not run' where none were made";

const PR_SCHEMA: &str = r#"{"type":"object","properties":{"title":{"type":"string"},"body":{"type":"string"}},"required":["title","body"],"additionalProperties":false}"#;

/// The heading and body of a model's answer, from its `key` and `body`, or from the first line
/// and the rest of a plain-text one. The heading is one line without a trailing period, at most
/// 72 characters; None when it is empty.
fn split_answer(answer: &Value, key: &str) -> Option<(String, String)> {
    let (heading, body) = match answer.as_str() {
        Some(text) => {
            let (first, rest) = text.trim().split_once('\n').unwrap_or((text.trim(), ""));
            (first.to_string(), rest.to_string())
        }
        None => (
            answer[key].as_str()?.to_string(),
            answer["body"].as_str().unwrap_or_default().to_string(),
        ),
    };
    let heading = heading.lines().next().unwrap_or_default();
    let heading: String = heading
        .trim()
        .trim_matches(|c| matches!(c, '"' | '`'))
        .trim_end_matches('.')
        .chars()
        .take(72)
        .collect();
    (!heading.is_empty()).then(|| (heading, body.trim().to_string()))
}

fn commit_message(answer: &Value) -> Option<String> {
    let (subject, body) = split_answer(answer, "subject")?;
    Some(if body.is_empty() {
        subject
    } else {
        format!("{subject}\n\n{body}")
    })
}

fn pull_request_text(answer: &Value) -> Option<(String, String)> {
    split_answer(answer, "title")
}

/// The job's id in a GitHub Actions link, such as `…/actions/runs/1/job/2`.
fn job_id(url: &str) -> Option<&str> {
    let after = url.split("/job/").nth(1)?;
    after
        .split(['/', '?', '#'])
        .next()
        .filter(|id| !id.is_empty())
}

/// The end of a failed job's log, without the job, step, and time `gh` puts before each line.
fn log_tail(log: &str) -> String {
    let lines: Vec<&str> = log
        .lines()
        .map(|line| {
            let text = line.splitn(3, '\t').nth(2).unwrap_or(line);
            // Each line starts with its time, such as `2026-11-08T05:39:46.4388536Z`.
            match text.split_once(' ') {
                Some((time, rest)) if time.ends_with('Z') && time.contains('T') => rest,
                _ => text,
            }
        })
        .collect();
    let text = lines.join("\n");
    match text.char_indices().rev().nth(LOG_TAIL) {
        Some((start, _)) => format!("[earlier lines cut]\n{}", &text[start..]),
        None => text,
    }
}

fn login(user: &Value) -> &str {
    user["login"].as_str().unwrap_or("someone")
}

/// The conflicts in a file, marker lines included, each under the line it starts on.
fn conflict_hunks(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for (i, line) in text.lines().enumerate() {
        if line.starts_with("<<<<<<<") {
            inside = true;
            let _ = writeln!(out, "[line {}]", i + 1);
        }
        if inside {
            out.push_str(line);
            out.push('\n');
        }
        if line.starts_with(">>>>>>>") {
            inside = false;
        }
    }
    out.trim_end().to_string()
}
