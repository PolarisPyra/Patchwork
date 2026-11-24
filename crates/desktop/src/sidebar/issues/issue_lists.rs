//! The issue lists as loaded: the open project's GitHub issues and the person's Linear issues,
//! read off the UI thread when the view first shows and again on Refresh, and the field the
//! Linear API key is typed into.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{Entity, ScrollHandle, prelude::*};
use patchwork_core::issues::Issue;
use patchwork_ui::text::text_area::{Changed, Submitted, TextArea};

use crate::workspace::Workspace;

/// One tracker's list.
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
    pub(super) linear: Listed,
    /// Whether a Linear key is saved, once looked up.
    pub(super) linear_key: Option<bool>,
    /// The field a Linear key is typed into, while none is saved.
    pub(super) key_field: Option<Entity<TextArea>>,
    pub(super) key_error: Option<String>,
    pub(super) scroll: ScrollHandle,
}

impl Workspace {
    /// Reads the lists the view shows that have not been read, or were read for another project.
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
        if self.issues.linear_key.is_none() {
            self.issues.linear_key = Some(false);
            let core = Arc::clone(&self.core);
            patchwork_ui::off_thread(
                cx,
                move || core.has_linear_key(),
                |this, saved, cx| {
                    this.issues.linear_key = Some(saved);
                    if saved {
                        this.issues.linear = Listed::Unread;
                        this.load_issues(cx);
                    }
                    cx.notify();
                },
            );
            return;
        }
        if self.issues.linear_key == Some(true) && matches!(self.issues.linear, Listed::Unread) {
            self.issues.linear = Listed::Loading;
            let core = Arc::clone(&self.core);
            patchwork_ui::off_thread(
                cx,
                move || core.linear_issues(),
                |this, read, cx| {
                    this.issues.linear = match read {
                        Ok(issues) => Listed::Read(issues),
                        Err(e) => Listed::Failed(e.to_string()),
                    };
                    cx.notify();
                },
            );
        }
    }

    /// Reads both lists again.
    pub(super) fn refresh_issues(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.issues.github, Listed::Loading) {
            self.issues.github = Listed::Unread;
        }
        if !matches!(self.issues.linear, Listed::Loading) {
            self.issues.linear = Listed::Unread;
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

    /// The field the Linear key is typed into, made the first time it shows.
    pub(super) fn linear_key_field(&mut self, cx: &mut Context<Self>) -> Entity<TextArea> {
        if let Some(field) = &self.issues.key_field {
            return field.clone();
        }
        let field = cx.new(|cx| {
            let mut field = TextArea::new("lin_api_…".to_owned(), "Linear API key".to_owned(), cx);
            field.single_line = true;
            field.masked = true;
            field.placeholder_color = Some(patchwork_ui::theme::input_placeholder());
            field
        });
        cx.subscribe(&field, |this, _, _: &Submitted, cx| {
            this.save_linear_key(cx);
        })
        .detach();
        cx.subscribe(&field, |_, _, _: &Changed, cx| cx.notify())
            .detach();
        self.issues.key_field = Some(field.clone());
        field
    }

    /// Saves the key typed into the field, then lists its issues.
    pub(super) fn save_linear_key(&mut self, cx: &mut Context<Self>) {
        let Some(field) = self.issues.key_field.clone() else {
            return;
        };
        let key = field.read(cx).content.trim().to_owned();
        if key.is_empty() {
            return;
        }
        self.issues.key_error = None;
        let core = Arc::clone(&self.core);
        patchwork_ui::off_thread(
            cx,
            move || core.set_linear_key(Some(&key)),
            move |this, saved, cx| {
                match saved {
                    Ok(()) => {
                        field.update(cx, |f, cx| f.set_text("", cx));
                        this.issues.linear_key = Some(true);
                        this.issues.linear = Listed::Unread;
                        this.load_issues(cx);
                    }
                    Err(e) => this.issues.key_error = Some(e.to_string()),
                }
                cx.notify();
            },
        );
    }

    /// Forgets the saved Linear key and its list.
    pub(super) fn forget_linear_key(&mut self, cx: &mut Context<Self>) {
        let core = Arc::clone(&self.core);
        patchwork_ui::off_thread(
            cx,
            move || core.set_linear_key(None),
            |this, forgot, cx| {
                match forgot {
                    Ok(()) => {
                        this.issues.linear_key = Some(false);
                        this.issues.linear = Listed::Unread;
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            },
        );
    }
}
