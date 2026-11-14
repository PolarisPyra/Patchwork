//! The orchestrator page's state: the request being written, and what became of each one sent.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{
    AppContext, Context, Entity, Focusable, SharedString, Subscription, WeakEntity, Window, hsla,
};
use patchwork_server::orchestration::Dispatched;

use crate::ui::text_area::{Submitted, TextArea};
use crate::workspace::Workspace;

/// The most requests the page lists. Older ones leave it; their chats stay in the task list.
const MAX_REQUESTS: usize = 6;

/// What became of a request.
pub(super) enum Outcome {
    /// The server is finding its chat.
    Routing,
    /// A chat has it.
    Routed(Dispatched),
    /// No chat got it, and why.
    Failed(SharedString),
}

/// A request sent from the page.
pub(super) struct Request {
    pub(super) id: usize,
    pub(super) text: SharedString,
    pub(super) outcome: Outcome,
}

/// The orchestrator page: one field that sends each request to the chat it belongs in.
pub struct Orchestrator {
    pub(super) workspace: WeakEntity<Workspace>,
    pub(super) field: Entity<TextArea>,
    /// The requests sent since the app started, newest first.
    pub(super) requests: Vec<Request>,
    next_id: usize,
    _subscriptions: [Subscription; 2],
}

impl Orchestrator {
    pub fn new(workspace: &Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| {
            let mut field = TextArea::new("Do something in a project", "Request", cx);
            field.single_line = true;
            // The page is dark in every theme, so its field is too.
            field.text_color = hsla(0.6, 0.25, 0.96, 1.);
            field.placeholder_color = hsla(0.6, 0.2, 0.9, 0.38);
            field.selection_color = hsla(0.55, 0.9, 0.65, 0.35);
            field
        });
        let submitted = cx.subscribe_in(&field, window, |this, _, _: &Submitted, window, cx| {
            this.submit(window, cx);
        });
        // A chat's title and whether it is working are read from the workspace.
        let changes = cx.observe(workspace, |_, _, cx| cx.notify());
        Self {
            workspace: workspace.downgrade(),
            field,
            requests: Vec::new(),
            next_id: 0,
            _subscriptions: [submitted, changes],
        }
    }

    /// Puts the cursor in the field.
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.field.read(cx).focus_handle(cx), cx);
    }

    /// Puts the cursor in the field with `text` added at its end, for a key typed outside it.
    pub fn type_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.field.update(cx, |field, cx| {
            let end = field.content.len();
            field.replace(end..end, text, window, cx);
        });
        self.focus(window, cx);
    }

    /// Sends what the field holds to the chat it belongs in, and lists it while that is found.
    pub(super) fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.field.read(cx).content.trim().to_string();
        let Some(workspace) = self.workspace.upgrade().filter(|_| !text.is_empty()) else {
            return;
        };
        let (server, new) = {
            let workspace = workspace.read(cx);
            let project = workspace.settings.active_project_id.clone();
            let new = workspace
                .composer
                .read(cx)
                .new_thread(project, &workspace.settings);
            (Arc::clone(&workspace.server), new)
        };
        self.field.update(cx, |field, cx| field.clear(window, cx));
        let id = self.next_id;
        self.next_id += 1;
        self.requests.insert(
            0,
            Request {
                id,
                text: text.clone().into(),
                outcome: Outcome::Routing,
            },
        );
        self.requests.truncate(MAX_REQUESTS);
        // The server asks a model which chat is meant, which takes a moment.
        let task = cx.background_spawn(async move { server.dispatch(&text, new) });
        cx.spawn(async move |this, cx| {
            let outcome = match task.await {
                Ok(dispatched) => Outcome::Routed(dispatched),
                Err(e) => {
                    log::warn!(error:% = e; "could not dispatch a request: {{error}}");
                    Outcome::Failed(e.to_string().into())
                }
            };
            this.update(cx, |this, cx| {
                if let Some(request) = this.requests.iter_mut().find(|r| r.id == id) {
                    request.outcome = outcome;
                }
                cx.notify();
            })
        })
        .detach();
    }
}
