//! What the page does with a request and a question: it sends a request to the place `@` pinned
//! or a new chat in the chat box's project, and answers the agent whose question is shown.

// Rust guideline compliant 2026-10-09

use patchwork_client::AppState;
use std::collections::HashMap;
use std::sync::Arc;

use gpui::{App, AppContext, Context};
use patchwork_client::local::core::chats::AskKind;
use patchwork_client::local::core::orchestration::Target;
use patchwork_design_icons::IconName;
use patchwork_design_text::display_title;
use patchwork_desktop_chat::{Mention, Outgoing};

use crate::host::OrchestratorEvent;
use super::requests::{Orchestrator, Outcome, Waiting};

/// How `@` names a new chat in no project.
const NO_PROJECT: &str = "none";

impl Orchestrator {
    /// The question shown above the chat box.
    pub(super) fn asked(&self) -> Option<(usize, &Waiting)> {
        let id = self.answering.as_ref()?;
        self.waiting
            .iter()
            .enumerate()
            .find(|(_, w)| &w.ask.approval_id == id)
    }

    /// The chat box answers the open question shown above it, rather than sending a request.
    pub(super) fn answering_question(&self) -> bool {
        !self.declined
            && self
                .asked()
                .is_some_and(|(_, w)| matches!(w.ask.kind, AskKind::Question(_)))
    }

    /// The project the chats a request starts go in: the one open in the sidebar. None for no
    /// project.
    pub(super) fn project_id(&self, cx: &App) -> Option<String> {
        let settings = &AppState::get(cx).settings;
        settings
            .window
            .active_project_id
            .clone()
            .filter(|id| settings.projects.iter().any(|p| &p.id == id))
    }

    /// Shows the first question once the one shown is answered.
    pub(super) fn settle(&mut self) {
        if self.asked().is_none() {
            self.answering = self.waiting.first().map(|w| w.ask.approval_id.clone());
        }
    }

    /// Shows the next question, going round to the first after the last.
    pub(super) fn next_question(&mut self, cx: &mut Context<Self>) {
        let count = self.waiting.len();
        let at = self
            .asked()
            .map_or(0, |(index, _)| (index + 1) % count.max(1));
        self.answering = self.waiting.get(at).map(|w| w.ask.approval_id.clone());
        cx.notify();
    }

    /// What `@` offers: a new chat in no project; each project, whose menu starts a new task in
    /// it or picks one of its chats; and the chats in no project.
    pub(super) fn mentions(&self, cx: &App) -> Vec<Mention> {
        let state = AppState::get(cx);
        let chats: Vec<_> = state
            .chats
            .iter()
            .filter(|t| t.parent.is_none() && t.archived_at.is_none())
            .collect();
        let chat = |chat: &patchwork_model::chats::Chat, place: &str| Mention {
            id: format!("chat:{}", chat.id),
            label: display_title(&chat.title),
            detail: place.to_string(),
            icon: IconName::ChatCircle,
            children: Vec::new(),
            separated: false,
        };
        let new = Mention {
            id: NO_PROJECT.into(),
            label: "New chat".into(),
            detail: "No project".into(),
            icon: IconName::NotePencil,
            children: Vec::new(),
            separated: false,
        };
        let projects = state
            .settings
            .projects
            .iter()
            .enumerate()
            .map(|(i, project)| {
                let id = format!("project:{}", project.id);
                let new_task = Mention {
                    id: id.clone(),
                    label: "New task".into(),
                    detail: project.name.clone(),
                    icon: IconName::NotePencil,
                    children: Vec::new(),
                    separated: false,
                };
                let tasks = chats
                    .iter()
                    .filter(|t| t.project_id.as_deref() == Some(project.id.as_str()))
                    .enumerate()
                    .map(|(i, t)| Mention {
                        separated: i == 0,
                        ..chat(t, &project.name)
                    });
                Mention {
                    id,
                    label: project.name.clone(),
                    detail: String::new(),
                    icon: IconName::Folder,
                    children: std::iter::once(new_task).chain(tasks).collect(),
                    separated: i == 0,
                }
            });
        let loose = chats
            .iter()
            .filter(|t| t.project_id.is_none())
            .enumerate()
            .map(|(i, t)| Mention {
                separated: i == 0,
                ..chat(t, "No project")
            });
        std::iter::once(new).chain(projects).chain(loose).collect()
    }

    /// Pins the place `@` picked, by its mention's id.
    pub(super) fn pin(&mut self, id: &str, cx: &mut Context<Self>) {
        self.pinned = if id == NO_PROJECT {
            Some(Target::New(None))
        } else if let Some(project) = id.strip_prefix("project:") {
            Some(Target::New(Some(project.to_string())))
        } else {
            id.strip_prefix("chat:")
                .map(|chat| Target::Chat(chat.to_string()))
        };
        cx.notify();
    }

    /// Opens the chat the question shown was asked in, which answers it there.
    pub(super) fn open_asking(&self, cx: &mut Context<Self>) {
        if let Some((_, waiting)) = self.asked() {
            cx.emit(OrchestratorEvent::OpenChat(waiting.ask.chat_id.clone()));
        }
    }

    /// Opens a card's chat read-only, as a subagent shows: the chat box sends it what comes next.
    pub(super) fn open(&self, chat_id: &str, cx: &mut Context<Self>) {
        cx.emit(OrchestratorEvent::PeekChat(chat_id.to_string()));
    }

    /// Answers the question shown: allows or denies it, with `words` as the answer to an open
    /// question. The answer goes to the agent's chat, not through the router.
    pub(super) fn respond(&mut self, allow: bool, words: Option<String>, cx: &mut Context<Self>) {
        let Some((_, waiting)) = self.asked() else {
            return;
        };
        let ask = &waiting.ask;
        let answers: HashMap<String, Vec<String>> = match (&ask.kind, words) {
            (AskKind::Question(question), Some(words)) => {
                HashMap::from([(question.clone(), vec![words])])
            }
            _ => HashMap::new(),
        };
        let core = Arc::clone(AppState::get(cx).core());
        if let Err(e) = core
            .chats()
            .answer(&ask.chat_id, &ask.approval_id, allow, answers, None)
        {
            log::warn!(error:% = e; "could not answer an agent from the orchestrator: {{error}}");
        }
    }

    /// Sends what the chat box sent to the place pinned with `@`, or to a new chat in the chat
    /// box's project, and lists it.
    pub(super) fn submit(&mut self, message: Outgoing, cx: &mut Context<Self>) {
        let Outgoing {
            text,
            attachments,
            mut new,
            ..
        } = message;
        new.project_id = self.project_id(cx);
        let target = self
            .pinned
            .take()
            .unwrap_or_else(|| Target::New(new.project_id.clone()));
        let core = Arc::clone(AppState::get(cx).core());
        let id = self.list(&text);
        let task = cx
            .background_spawn(async move { core.dispatch_to(&text, &target, &new, &attachments) });
        cx.spawn(async move |this, cx| {
            let outcome = task.await;
            this.update(cx, |this, cx| {
                let outcome = match outcome {
                    Ok(dispatched) => Outcome::Routed(dispatched),
                    Err(e) => {
                        log::warn!(error:% = e; "could not dispatch a request: {{error}}");
                        Outcome::Failed(e.to_string().into())
                    }
                };
                this.settle_request(id, outcome);
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }
}
