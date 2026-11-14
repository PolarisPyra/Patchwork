//! The cards under the pill: one for each request, with the chat that has it, how that chat
//! stands, and the line its agent last wrote. A request split across projects has a row for each
//! of its chats.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use gpui::{AnyElement, Div, Hsla, Role, SharedString, Stateful, div, hsla, prelude::*, px};
use patchwork_server::projects::Project;
use patchwork_server::threads::{Standing, Thread};

use super::render::{glass, ink, warm};
use super::state::{Orchestrator, Outcome, Request};
use crate::ui::theme::{FS_SM, FS_XS, radius_lg};
use crate::ui::{button, display_title, icon, spinner};

/// A chat as a card shows it.
struct Chat {
    id: String,
    /// Its title, then its project when it has one.
    name: String,
    standing: Standing,
    line: Option<String>,
}

impl Chat {
    fn read(thread: &Thread, projects: &[Project]) -> Self {
        let project = thread
            .project_id
            .as_ref()
            .and_then(|id| projects.iter().find(|p| &p.id == id));
        let title = display_title(&thread.title);
        Self {
            id: thread.id.clone(),
            name: project.map_or_else(|| title.clone(), |p| format!("{title} · {}", p.name)),
            standing: thread.standing(),
            line: thread.latest_line(),
        }
    }
}

/// How a chat stands, in words.
fn word(standing: Standing) -> &'static str {
    match standing {
        Standing::Working => "working",
        Standing::Waiting => "waiting on you",
        Standing::Done => "done",
        Standing::Failed => "failed",
    }
}

/// How a chat stands, as a mark at the start of its row.
fn mark(standing: Standing) -> AnyElement {
    let glyph = |name: &str, color: Hsla| {
        div()
            .flex_none()
            .text_color(color)
            .child(icon(name, 14.))
            .into_any_element()
    };
    match standing {
        Standing::Working => spinner(14., ink(0.8)).into_any_element(),
        Standing::Waiting => glyph("circle-fill", warm()),
        Standing::Done => glyph("check", ink(0.5)),
        Standing::Failed => glyph("warning", ink(0.5)),
    }
}

/// How the chats of one request stand together: waiting on the person before anything else.
fn together(chats: &[Chat]) -> Standing {
    [Standing::Waiting, Standing::Working, Standing::Failed]
        .into_iter()
        .find(|standing| chats.iter().any(|chat| chat.standing == *standing))
        .unwrap_or(Standing::Done)
}

/// The look every card shares.
fn plate(card: Stateful<Div>) -> Stateful<Div> {
    card.min_h(px(34.))
        .flex_none()
        .flex()
        .flex_col()
        // A button centers what it holds; a card's lines fill it from the left.
        .items_start()
        .justify_center()
        .w_full()
        .overflow_hidden()
        .gap(px(2.))
        .px(px(14.))
        .py(px(7.))
        .rounded(px(radius_lg()))
        .border_1()
        .border_color(ink(0.06))
        .bg(glass(0.5))
        .text_size(px(FS_SM))
}

/// A card's first line: a mark, what was asked, and a note at its end.
fn line(mark: AnyElement, text: SharedString, note: Option<(SharedString, Hsla)>) -> Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(mark)
        .child(div().flex_1().min_w_0().truncate().child(text))
        .children(note.map(|(note, color)| {
            div()
                .flex_none()
                .max_w(px(260.))
                .truncate()
                .text_color(color)
                .child(note)
        }))
}

/// The line an agent last wrote, under what it was asked.
fn latest(text: Option<String>) -> Option<Div> {
    text.map(|text| {
        div()
            .w_full()
            .min_w_0()
            .pl(px(24.))
            .truncate()
            .text_size(px(FS_XS))
            .text_color(ink(0.5))
            .child(text)
    })
}

impl Orchestrator {
    /// One request's card. A card with a chat opens it on the Chats page.
    pub(super) fn card(
        &self,
        request: &Request,
        threads: &[Arc<Thread>],
        projects: &[Project],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row_id = ("orchestrator-request", request.id);
        let text = request.text.clone();
        let chats: Vec<Chat> = match &request.outcome {
            Outcome::Routing => {
                return plate(div().id(row_id))
                    .role(Role::Status)
                    .aria_label(format!("Routing: {text}"))
                    .child(line(spinner(14., ink(0.8)).into_any_element(), text, None))
                    .into_any_element();
            }
            Outcome::Failed(error) => {
                let note = (error.clone(), hsla(0.0, 0.85, 0.72, 1.));
                return plate(div().id(row_id))
                    .role(Role::Alert)
                    .aria_label(format!("Failed, {error}: {text}"))
                    .child(line(mark(Standing::Failed), text, Some(note)))
                    .into_any_element();
            }
            Outcome::Routed(to) => to
                .thread_ids
                .iter()
                .filter_map(|id| threads.iter().find(|t| &t.id == id))
                .map(|thread| Chat::read(thread, projects))
                .collect(),
        };
        match chats.as_slice() {
            [] => plate(div().id(row_id))
                .role(Role::ListItem)
                .aria_label(format!("Chat deleted: {text}"))
                .child(line(mark(Standing::Done), text, None))
                .into_any_element(),
            [chat] => {
                let state = word(chat.standing);
                let label = format!("{}, {state}: {text}. Open the chat", chat.name);
                let note = (SharedString::from(chat.name.clone()), ink(0.6));
                plate(button(row_id, label))
                    .hover(|s| s.bg(glass(0.72)).border_color(ink(0.14)))
                    .child(line(mark(chat.standing), text, Some(note)))
                    .children(latest(chat.line.clone()))
                    .on_click(cx.listener({
                        let id = chat.id.clone();
                        move |this, _: &gpui::ClickEvent, _, cx| this.open(&id, cx)
                    }))
                    .into_any_element()
            }
            chats => {
                let all = together(chats);
                let count = SharedString::from(format!("{} chats", chats.len()));
                let label = format!("{count}, {}: {text}", word(all));
                let rows = chats.iter().enumerate().map(|(index, chat)| {
                    let label = format!("{}, {}. Open the chat", chat.name, word(chat.standing));
                    let said = chat.line.clone().map(|line| (line.into(), ink(0.5)));
                    button(("orchestrator-child", request.id * 64 + index), label)
                        .w_full()
                        .pl(px(24.))
                        .py(px(2.))
                        .rounded(px(6.))
                        .text_size(px(FS_XS))
                        .text_color(ink(0.75))
                        .hover(|s| s.bg(ink(0.06)))
                        .child(line(mark(chat.standing), chat.name.clone().into(), said))
                        .on_click(cx.listener({
                            let id = chat.id.clone();
                            move |this, _: &gpui::ClickEvent, _, cx| this.open(&id, cx)
                        }))
                });
                plate(div().id(row_id))
                    .role(Role::Group)
                    .aria_label(label)
                    .child(line(mark(all), text, Some((count, ink(0.6)))))
                    .children(rows)
                    .into_any_element()
            }
        }
    }
}
