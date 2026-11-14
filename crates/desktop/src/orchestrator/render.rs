//! The orchestrator page drawn: a dark pill to write a request in, over the butterfly, and under
//! it the requests sent, each with the chat that has it.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Bounds, BoxShadow, Div, Hsla, Pixels, Role, SharedString, Stateful, Window, div,
    hsla, point, prelude::*, px, relative, size,
};
use patchwork_server::orchestration::Route;

use super::butterfly;
use super::state::{Orchestrator, Outcome};
use crate::ui::theme::{FONT_UI, FS_LG, FS_SM, RADIUS_LG};
use crate::ui::{button, display_title, icon, spinner};
use crate::workspace::is_busy;

/// The pill: its size, the room kept at its sides, and how far down the page its top is.
const PILL_W: f32 = 640.;
const PILL_H: f32 = 52.;
const GUTTER: f32 = 24.;
const PILL_TOP: f32 = 0.4;

/// The page's text color, at this strength.
fn ink(alpha: f32) -> Hsla {
    hsla(0.6, 0.25, 0.96, alpha)
}

/// The page's dark glass, this opaque.
fn glass(alpha: f32) -> Hsla {
    hsla(0.64, 0.30, 0.05, alpha)
}

/// Where the pill is on a page with these bounds, which is where the layout below puts it. The
/// backdrop draws what is behind it as glows.
fn pill_bounds(page: Bounds<Pixels>) -> Bounds<Pixels> {
    let width = (page.size.width - px(GUTTER * 2.)).clamp(px(0.), px(PILL_W));
    Bounds::new(
        point(
            page.center().x - width / 2.,
            page.top() + page.size.height * PILL_TOP,
        ),
        size(width, px(PILL_H)),
    )
}

/// What a request's row shows after its text.
enum Shown {
    Routing,
    /// The chat that has it: its id and title, whether it is working, and how it got the request.
    Chat(String, String, bool, Route),
    /// Its chat has been deleted.
    Gone,
    Failed(SharedString),
}

impl Render for Orchestrator {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.field.read(cx).content.trim().is_empty();
        let workspace = self.workspace.upgrade();
        let threads = workspace
            .as_ref()
            .map_or(&[][..], |w| w.read(cx).threads.as_slice());
        let rows: Vec<_> =
            self.requests
                .iter()
                .map(|request| {
                    let shown = match &request.outcome {
                        Outcome::Routing => Shown::Routing,
                        Outcome::Failed(error) => Shown::Failed(error.clone()),
                        Outcome::Routed(to) => threads
                            .iter()
                            .find(|t| t.id == to.thread_id)
                            .map_or(Shown::Gone, |t| {
                                Shown::Chat(
                                    t.id.clone(),
                                    display_title(&t.title),
                                    is_busy(t),
                                    to.route,
                                )
                            }),
                    };
                    (request.id, request.text.clone(), shown)
                })
                .collect();
        let pill = div()
            .id("orchestrator-pill")
            .w_full()
            .max_w(px(PILL_W))
            .h(px(PILL_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(22.))
            .pr(px(8.))
            .rounded_full()
            .border_1()
            .border_color(ink(0.10))
            .bg(glass(0.62))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., 0.55),
                offset: point(px(0.), px(14.)),
                blur_radius: px(44.),
                spread_radius: px(0.),
                inset: false,
            }])
            .text_size(px(FS_LG))
            .child(div().flex_1().min_w_0().child(self.field.clone()))
            .child(
                button("orchestrator-send", "Send")
                    .size(px(36.))
                    .flex_none()
                    .justify_center()
                    .rounded_full()
                    .bg(ink(if empty { 0.14 } else { 0.92 }))
                    .text_color(if empty { ink(0.5) } else { glass(1.) })
                    .child(icon("arrow-up-bold", 16.))
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            );
        let requests = div()
            .id("orchestrator-requests")
            .role(Role::List)
            .aria_label("Requests")
            .w_full()
            .max_w(px(PILL_W))
            .mt(px(14.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(
                rows.into_iter()
                    .map(|(id, text, shown)| request_row(id, &text, shown, cx)),
            );
        div()
            .id("orchestrator")
            .role(Role::Main)
            .aria_label("Orchestrator")
            .relative()
            .size_full()
            .overflow_hidden()
            .font_family(FONT_UI)
            .text_color(ink(0.9))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .child(butterfly::field(pill_bounds)),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .px(px(GUTTER))
                    .child(div().flex_none().h(relative(PILL_TOP)))
                    .child(pill)
                    .child(requests),
            )
    }
}

/// The look every request's row shares.
fn plate(row: Stateful<Div>) -> Stateful<Div> {
    row.h(px(34.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(14.))
        .rounded(px(RADIUS_LG))
        .border_1()
        .border_color(ink(0.06))
        .bg(glass(0.5))
        .text_size(px(FS_SM))
}

/// One request: how it stands, what was asked, and the chat it went to. A row with a chat opens it.
fn request_row(
    id: usize,
    text: &SharedString,
    shown: Shown,
    cx: &mut Context<Orchestrator>,
) -> AnyElement {
    let row_id = ("orchestrator-request", id);
    let asked = div().flex_1().min_w_0().truncate().child(text.clone());
    let mark = |name: &str| {
        div()
            .flex_none()
            .text_color(ink(0.5))
            .child(icon(name, 14.))
    };
    let note = |text: SharedString, color: Hsla| {
        div()
            .flex_none()
            .max_w(relative(0.45))
            .truncate()
            .text_color(color)
            .child(text)
    };
    match shown {
        Shown::Routing => plate(div().id(row_id))
            .role(Role::Status)
            .aria_label(format!("Finding a chat for: {text}"))
            .child(spinner(14., ink(0.8)))
            .child(asked)
            .into_any_element(),
        Shown::Failed(error) => plate(div().id(row_id))
            .role(Role::Alert)
            .aria_label(format!("{error}: {text}"))
            .child(mark("warning"))
            .child(asked)
            .child(note(error, hsla(0.0, 0.85, 0.72, 1.)))
            .into_any_element(),
        Shown::Gone => plate(div().id(row_id))
            .role(Role::ListItem)
            .aria_label(format!("Chat deleted: {text}"))
            .child(mark("trash"))
            .child(asked)
            .into_any_element(),
        Shown::Chat(thread_id, title, busy, route) => {
            let (verb, arrow) = match route {
                Route::Started => ("Started", "plus"),
                Route::Sent => ("Sent to", "arrow-right"),
            };
            let state = if busy { "working" } else { "done" };
            let label = format!("{verb} {title}, {state}: {text}. Open the chat");
            plate(button(row_id, label))
                .hover(|s| s.bg(glass(0.72)).border_color(ink(0.14)))
                .child(if busy {
                    spinner(14., ink(0.8)).into_any_element()
                } else {
                    mark("check").into_any_element()
                })
                .child(asked)
                .child(mark(arrow))
                .child(note(title.into(), ink(0.6)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(workspace) = this.workspace.upgrade() {
                        workspace.update(cx, |w, cx| w.open_thread(&thread_id, cx));
                    }
                }))
                .into_any_element()
        }
    }
}
