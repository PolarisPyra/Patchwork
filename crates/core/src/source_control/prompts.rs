//! The prompts that hand a pull request's feedback, or a merge's conflicts, to an agent.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;

use crate::source_control::{self, CheckState};
use crate::{Core, Error};

/// Failed jobs whose logs a pull request's feedback quotes, and how much of each log's end.
const FAILED_JOBS: usize = 4;
const LOG_TAIL: usize = 6_000;
/// How much of each conflicted file's conflicts the prompt quotes, and of all of them.
const FILE_CONFLICTS: usize = 4_000;
const ALL_CONFLICTS: usize = 24_000;

impl Core {
    /// A prompt with what the branch's open pull request needs: its failed checks with the end
    /// of each failed job's log, and its reviews and comments. Returned with the chat that
    /// made the branch's newest commit Patchwork recorded, to send it to.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when `gh` fails, or when the branch has no
    /// open pull request.
    pub fn pull_request_feedback(
        &self,
        project_id: &str,
        chat_id: Option<&str>,
    ) -> Result<(Option<String>, String), Error> {
        let root = self.source_root(project_id, chat_id)?;
        let pr = source_control::pull_request_view(&root)?;
        let number = pr["number"].as_u64().unwrap_or_default();
        let mut sections = Vec::new();

        let failed: Vec<&Value> = source_control::pull_request_checks(&pr)
            .filter(|c| source_control::check_state(c) == CheckState::Failed)
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
                        source_control::gh(&root, &["run", "view", "--job", job, "--log-failed"])
                            .ok()
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
        let inline = source_control::gh(&root, &["api", &inline])
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
        let chat = source_control::base_ref(&root)
            .and_then(|base| {
                source_control::run_git(&root, &["rev-list", &format!("{base}..HEAD")]).ok()
            })
            .and_then(|hashes| {
                hashes
                    .lines()
                    .find_map(|hash| self.commit_record(hash))
                    .map(|record| record.chat)
            });
        Ok((chat, source_control::truncated(&prompt, 40_000)))
    }

    /// A prompt for an agent to resolve the project's merge conflicts: what git stopped in, the
    /// two sides, and each conflicted file's conflicts as they stand.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when `git` fails, or when no file has a
    /// conflict.
    pub fn conflict_prompt(
        &self,
        project_id: &str,
        chat_id: Option<&str>,
    ) -> Result<String, Error> {
        let root = self.source_root(project_id, chat_id)?;
        let files = source_control::run_git(
            &root,
            &["diff", "--name-only", "--relative", "--diff-filter=U"],
        )?;
        let files: Vec<&str> = files.lines().filter(|f| !f.is_empty()).collect();
        if files.is_empty() {
            return Err("No file has a conflict to resolve".into());
        }
        let describe = |rev: &str| {
            source_control::run_git(&root, &["log", "-1", "--format=%h %s", rev])
                .map_or_else(|_| rev.to_string(), |s| s.trim().to_string())
        };
        let has = |rev: &str| {
            source_control::run_git(&root, &["rev-parse", "--verify", "-q", rev]).is_ok()
        };
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
        let branch = source_control::git_branch(&root).unwrap_or_else(|| "HEAD".into());
        let mut sides = format!("- Ours: {branch} at {}", describe("HEAD"));
        if let Some(theirs) = theirs {
            let _ = write!(sides, "\n- Theirs: {theirs}");
        }

        let mut conflicts = String::new();
        for file in &files {
            let text = match crate::system::ssh::split(&root) {
                Some(at) => crate::system::ssh::fs::read_inside(at.host, at.path, file, 1 << 20)
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
                format!(
                    "```\n{}\n```",
                    source_control::truncated(&hunks, FILE_CONFLICTS)
                )
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
            source_control::truncated(&conflicts, ALL_CONFLICTS)
        );
        Ok(prompt)
    }
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
