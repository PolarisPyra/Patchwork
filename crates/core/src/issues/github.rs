//! The open issues of a project's GitHub repository, read with the GitHub CLI.

// Rust guideline compliant 2026-10-09

use serde::Deserialize;

use super::Issue;
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
