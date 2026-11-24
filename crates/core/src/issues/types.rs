//! A GitHub issue, and the first message of a task started on it.

// Rust guideline compliant 2026-10-09

/// The longest issue description carried into a task's first message, in characters.
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
