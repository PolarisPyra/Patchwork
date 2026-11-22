//! Commit messages and pull request text, written by the model that writes titles.

// Rust guideline compliant 2026-10-09

use serde_json::Value;

use crate::chats::title::{self, Request, Writer};
use crate::source_control::{self, PullRequest};
use crate::{Core, Error};

impl Core {
    /// Has a model write a commit message for what is staged, or for every change, staged
    /// first, when nothing is. The message is to edit before committing.
    ///
    /// # Errors
    ///
    /// Returns an error when the project is unknown, when `git` fails to stage the changes or read
    /// them, or when no model writes a message.
    pub fn write_commit_message(
        &self,
        project_id: &str,
        chat_id: Option<&str>,
    ) -> Result<String, Error> {
        let root = self.source_root(project_id, chat_id)?;
        source_control::stage_for_commit(&root, false)?;
        self.staged_message(&root, chat_id)
    }

    pub(super) fn staged_message(
        &self,
        root: &str,
        chat_id: Option<&str>,
    ) -> Result<String, Error> {
        let summary = source_control::staged_summary(root)?;
        self.write(chat_id, COMMIT_PROMPT, COMMIT_SCHEMA, &summary)
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
        chat_id: Option<&str>,
    ) -> Result<PullRequest, Error> {
        let root = self.source_root(project_id, chat_id)?;
        let (base, summary) = source_control::branch_summary(&root)?;
        let text = self
            .write(chat_id, PR_PROMPT, PR_SCHEMA, &summary)
            .and_then(|answer| pull_request_text(&answer));
        let text = text.as_ref().map(|(t, b)| (t.as_str(), b.as_str()));
        source_control::create_pull_request(&root, &base, text)
    }

    /// Asks the title writer from Settings, else the open chat's provider and model.
    fn write(
        &self,
        chat_id: Option<&str>,
        prompt: &str,
        schema: &str,
        input: &str,
    ) -> Option<Value> {
        let tokens = self.store.settings().tokens;
        let writer = Writer {
            model: tokens.title_model,
            local_only: tokens.titles_local_only,
        };
        let chat = chat_id.and_then(|id| self.store.chat(id));
        let request = Request {
            prompt,
            schema,
            label: "Changes",
            input,
        };
        title::ask(
            &request,
            chat.as_ref().map(|t| t.agent),
            chat.as_ref().and_then(|t| t.model.as_deref()),
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
