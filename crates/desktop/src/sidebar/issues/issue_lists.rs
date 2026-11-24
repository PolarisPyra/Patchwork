//! The issue list as loaded: the open project's GitHub issues, read off the UI thread when the
//! view first shows and again on Refresh.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{ScrollHandle, prelude::*};
use patchwork_core::issues::Issue;

use crate::workspace::Workspace;

/// The repository's issue list.
#[derive(Default)]
pub enum Listed {
    /// Not asked for yet.
    #[default]
    Unread,
    Loading,
    Read(Vec<Issue>),
    Failed(String),
}

/// What the Issues view shows, kept between frames.
#[derive(Default)]
pub struct IssueLists {
    /// The project the GitHub list was read for.
    pub(super) project: Option<String>,
    pub(super) github: Listed,
    pub(super) scroll: ScrollHandle,
}

impl Workspace {
    /// Reads the list the view shows if it has not been read, or was read for another project.
    pub(super) fn load_issues(&mut self, cx: &mut Context<Self>) {
        let project = self.project().map(|p| p.id.clone());
        if self.issues.project != project {
            self.issues.project.clone_from(&project);
            self.issues.github = Listed::Unread;
        }
        if matches!(self.issues.github, Listed::Unread)
            && let Some(project) = project
        {
            self.issues.github = Listed::Loading;
            let core = Arc::clone(&self.core);
            let asked = project.clone();
            patchwork_ui::off_thread(
                cx,
                move || core.github_issues(&asked),
                move |this, read, cx| {
                    // A list read for a project no longer open is not shown.
                    if this.issues.project.as_deref() == Some(project.as_str()) {
                        this.issues.github = match read {
                            Ok(issues) => Listed::Read(issues),
                            Err(e) => Listed::Failed(e.to_string()),
                        };
                        cx.notify();
                    }
                },
            );
        }
    }

    /// Reads the list again.
    pub(super) fn refresh_issues(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.issues.github, Listed::Loading) {
            self.issues.github = Listed::Unread;
        }
        self.load_issues(cx);
        cx.notify();
    }

    /// Starts a task in the open project on `issue`, which the agent is asked to work on.
    pub(super) fn start_on_issue(&mut self, issue: &Issue, cx: &mut Context<Self>) {
        if self.project().is_none() {
            self.error = Some("Open a project to start a task on an issue.".into());
            return cx.notify();
        }
        self.start_task(&issue.task_title(), issue.prompt(), cx);
    }
}
