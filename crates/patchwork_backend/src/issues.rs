//! Issues to start tasks from: the open issues of a project's GitHub repository, read with `gh`.

// Rust guideline compliant 2026-10-09

use serde::Deserialize;

use crate::{Core, Error};

/// How many issues are listed, newest first.
const LIMIT: &str = "100";

#[derive(Deserialize)]
struct GhIssue {
    number: u64,
    title: String,
    url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    labels: Vec<GhLabel>,
}

#[derive(Deserialize)]
struct GhLabel {
    name: String,
}

impl Core {
    /// The open issues of project `project_id`'s GitHub repository, newest first.
    ///
    /// This runs `gh` and waits on the network, so call it off the UI thread.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, `gh` is missing or signed out, or the
    /// project's folder is not a GitHub repository.
    pub fn github_issues(&self, project_id: &str) -> Result<Vec<Issue>, Error> {
        let root = self.project_root(project_id)?;
        let out = crate::source_control::gh(
            &root,
            &[
                "issue",
                "list",
                "--state",
                "open",
                "--limit",
                LIMIT,
                "--json",
                "number,title,url,body,labels",
            ],
        )?;
        let issues: Vec<GhIssue> = serde_json::from_str(&out)
            .map_err(|e| Error::Message(format!("Could not read gh's issue list: {e}")))?;
        Ok(issues
            .into_iter()
            .map(|i| Issue {
                key: format!("#{}", i.number),
                title: i.title,
                url: i.url,
                body: i.body,
                labels: i.labels.into_iter().map(|l| l.name).collect(),
            })
            .collect())
    }
}

const BODY_LIMIT: usize = 6000;

/// An open issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Its short name on GitHub, e.g. `#123`.
    pub key: String,
    pub title: String,
    pub url: String,
    /// Its description, in Markdown.
    pub body: String,
    pub labels: Vec<String>,
}

impl Issue {
    /// The first message of a task started on the issue: its title, link and description, and
    /// the ask to work on it.
    #[must_use]
    pub fn prompt(&self) -> String {
        let mut body: String = self.body.trim().chars().take(BODY_LIMIT).collect();
        if body.len() < self.body.trim().len() {
            body.push_str("\n\n[description cut short]");
        }
        let mut text = format!(
            "Work on GitHub issue {}: {}\n{}\n",
            self.key, self.title, self.url
        );
        if !body.is_empty() {
            text.push('\n');
            text.push_str(&body);
            text.push('\n');
        }
        text.push_str(
            "\nRead the code the issue concerns, make the change it asks for, and say what you \
             changed and how you checked it.",
        );
        text
    }

    /// The task's name: the issue's key and title.
    #[must_use]
    pub fn task_title(&self) -> String {
        format!("{} {}", self.key, self.title)
    }
}
