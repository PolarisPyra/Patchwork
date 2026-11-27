//! The orchestrator page drawn: a dark pill to write a request in, over the backdrop, with the
//! place the request would go at its end, and under it a card for each request sent.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{
    AnyElement, Bounds, Entity, ExternalPaths, Hsla, KeyDownEvent, ObjectFit, Pixels, Role,
    SharedString, Subscription, WeakEntity, Window, div, hsla, img, point, prelude::*, px,
    relative, size,
};
use patchwork_backend::orchestration::Target;
use patchwork_themes::{Applied, Scene, Scheme, Shown};
use patchwork_backend::chats::AskKind;

use super::requests::Orchestrator;
use patchwork_ui_backdrop::{Fallback, butterfly_field, preset_fill, scene_field};
use patchwork_ui_theme::{
    IconName, applied, font_ui, fs_lg, fs_sm, glass_over, ink_over, radius_lg, theme,
};
use patchwork_ui_components::pill::{PILL_PAD, SEND, pill, send_button};
use patchwork_ui_components::button;
use patchwork_ui_markdown::display_title;
use patchwork_ui_theme::icon;

/// The pill: its size, the room kept at its sides, and how far down the page its top is.
pub(super) const PILL_W: f32 = 640.;
const PILL_H: f32 = SEND + PILL_PAD * 2.;
const GUTTER: f32 = 24.;
const PILL_TOP: f32 = 0.4;

/// Whether the page is over a light backdrop, so its text is dark. The page's render sets it
/// before anything on the page is drawn, and the one window draws on one chat.
static LIGHT: AtomicBool = AtomicBool::new(false);

fn light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// The page's text color, at this strength.
pub(super) fn ink(alpha: f32) -> Hsla {
    ink_over(!light(), alpha)
}

/// The page's glass, this opaque: dark over a dark backdrop, pale over a light one.
pub(super) fn glass(alpha: f32) -> Hsla {
    glass_over(!light(), alpha)
}

/// The color of something waiting on the person: the one warm color on the page.
pub(super) fn warm() -> Hsla {
    if light() {
        hsla(0.07, 0.90, 0.40, 1.)
    } else {
        hsla(0.09, 0.95, 0.66, 1.)
    }
}

/// The scene the settings picked for the backdrop, when it is one.
fn shown_scene(look: &Arc<Applied>) -> Option<&Scene> {
    match &look.backdrop.shown {
        Shown::Scene(scene) => Some(scene),
        _ => None,
    }
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

/// A place a request can go, as it is shown and as a screen reader says it.
#[derive(Default)]
pub(super) struct Place {
    /// The project it is in. None for a chat in no project.
    pub(super) project: Option<SharedString>,
    /// The chat: its title, or that it is new.
    pub(super) chat: SharedString,
    /// The whole of it in words.
    pub(super) spoken: SharedString,
}

impl Place {
    /// A chat in `project`; `spoken` is what comes before the project's name when it is said.
    fn inside(project: &str, chat: &str, spoken: &str) -> Self {
        Self {
            project: Some(project.to_string().into()),
            chat: chat.to_string().into(),
            spoken: format!("{spoken}{project}").into(),
        }
    }

    /// A chat in no project.
    fn outside(chat: String, spoken: String) -> Self {
        Self {
            project: None,
            chat: chat.into(),
            spoken: spoken.into(),
        }
    }

    /// The place drawn: a folder and the project's name before a chat in a project, and a chat
    /// bubble before one in none.
    fn shown(&self) -> gpui::Div {
        let row = div().min_w_0().flex().items_center().gap(px(6.));
        let chat = div().min_w_0().truncate().child(self.chat.clone());
        match &self.project {
            Some(project) => row
                .child(div().flex_none().child(icon(IconName::FolderFill, 13.)))
                .child(
                    div()
                        .flex_none()
                        .max_w(px(110.))
                        .truncate()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(project.clone()),
                )
                .child(div().flex_none().opacity(0.5).child("›"))
                .child(chat),
            None => row
                .child(div().flex_none().child(icon(IconName::ChatCircle, 13.)))
                .child(chat),
        }
    }
}

/// A small label inside the pill.
fn chip(label: impl Into<SharedString>) -> gpui::Div {
    chip_frame().child(div().min_w_0().truncate().child(label.into()))
}

/// The chip for the place a request goes.
fn route_chip(place: &Place) -> gpui::Div {
    chip_frame()
        .max_w(px(260.))
        .child(div().flex_none().opacity(0.5).child("→"))
        .child(place.shown())
}

/// The shape the pill's chips share.
fn chip_frame() -> gpui::Div {
    div()
        .h(px(26.))
        .max_w(px(220.))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .rounded_full()
        .bg(ink(0.08))
        .text_size(px(fs_sm()))
        .text_color(ink(0.8))
}

/// How much of the window's color is laid over a picture, so the page reads over it.
const PICTURE_DIM: f32 = 0.45;

impl Orchestrator {
    /// What is drawn behind the page: what the theme settings came to.
    fn backdrop(&mut self, cx: &Context<Self>) -> AnyElement {
        let look = applied(cx);
        let still = look.reduce_motion;
        let t = theme(cx);
        match &look.backdrop.shown {
            Shown::None => div().size_full().bg(t.bg_app).into_any_element(),
            Shown::Butterfly => {
                let flight = Rc::clone(&self.flight);
                butterfly_field(pill_bounds, still, flight, None).into_any_element()
            }
            Shown::Image { path } => {
                // The picture before this one stays under it until this one is loaded, so
                // changing it never shows the bare page.
                if self.pictures[1].as_ref() != Some(path) {
                    self.pictures[0] = self.pictures[1].replace(path.clone());
                }
                let layer = |path: &PathBuf| {
                    img(path.clone())
                        .absolute()
                        .inset_0()
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                };
                div()
                    .size_full()
                    .relative()
                    .bg(t.bg_app)
                    .children(self.pictures.iter().flatten().map(layer))
                    .child(div().absolute().inset_0().bg(t.bg_app.opacity(PICTURE_DIM)))
                    .into_any_element()
            }
            Shown::Preset(id) => div()
                .size_full()
                .relative()
                .bg(t.bg_app)
                .children(preset_fill(id, t))
                .into_any_element(),
            Shown::Scene(_) => {
                let fallback = Fallback {
                    surface: t.bg_app,
                    faint: t.text_muted,
                };
                scene_field(look, shown_scene, fallback, pill_bounds, still, None)
                    .into_any_element()
            }
        }
    }

    /// A place a request can go, as its chip and the `@` list show it: a folder and the
    /// project's name first for a chat in a project, and a chat bubble for one in none.
    pub(super) fn place(&self, target: &Target, cx: &Context<Self>) -> Place {
        let Some(workspace) = self.workspace.upgrade() else {
            return Place::default();
        };
        let workspace = workspace.read(cx);
        let project = |id: &String| {
            let found = workspace.settings.projects.iter().find(|p| &p.id == id);
            found.map_or("Project", |p| p.name.as_str())
        };
        match target {
            Target::Chat(id) => {
                let Some(chat) = workspace.chats.iter().find(|t| &t.id == id) else {
                    return Place::outside("Chat".into(), "Chat".into());
                };
                let title = display_title(&chat.title);
                match &chat.project_id {
                    Some(id) => Place::inside(project(id), &title, &format!("{title} in ")),
                    None => Place::outside(title.clone(), format!("{title}, in no project")),
                }
            }
            Target::New(Some(id)) => Place::inside(project(id), "New task", "New task in "),
            Target::New(None) => {
                Place::outside("New chat".into(), "New chat, in no project".into())
            }
            Target::Split(ids) => {
                let names: Vec<&str> = ids.iter().map(project).collect();
                Place::inside(&names.join(", "), "New tasks", "New tasks in ")
            }
        }
    }

    /// What comes before the field: the chat whose question is shown, or how many are waiting.
    fn before_field(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let count = self.waiting.len();
        if let Some((index, waiting)) = self.asked() {
            let place = if count > 1 {
                format!("{} of {count}", index + 1)
            } else {
                String::new()
            };
            let label = format!("{} {place}", waiting.question());
            let named = chip(waiting.chat.clone())
                .id("orchestrator-asking")
                .role(Role::Status)
                .aria_label(label.trim().to_string())
                .text_color(warm())
                .children((count > 1).then(|| div().flex_none().text_color(ink(0.6)).child(place)));
            return Some(named.into_any_element());
        }
        (count > 0).then(|| {
            button("orchestrator-waiting", format!("{count} waiting. Answer"))
                .child(chip(format!("{count} waiting")).text_color(warm()))
                .rounded_full()
                .on_click(cx.listener(|this, _, window, cx| this.tab(window, cx)))
                .into_any_element()
        })
    }

    /// What comes after the field: where the request would go.
    fn after_field(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if self.answering.is_some() || self.field.read(cx).content.trim().is_empty() {
            return None;
        }
        let status = |label: String| {
            div()
                .id("orchestrator-route")
                .role(Role::Status)
                .aria_label(label)
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
        };
        let place = self.place(self.pinned.as_ref()?, cx);
        let pin = div().flex_none().child(icon(IconName::PushPin, 12.));
        let shown = route_chip(&place).child(pin);
        Some(
            status(format!("Goes to {}, pinned", place.spoken))
                .child(shown)
                .into_any_element(),
        )
    }

    /// The `@` list: the projects and chats a request can be pinned to.
    fn picker_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let highlighted = self.picker.as_ref()?.highlighted;
        let rows = self
            .entries(cx)
            .into_iter()
            .enumerate()
            .map(|(index, entry)| {
                let on = index == highlighted;
                div()
                    .id(("orchestrator-pick", index))
                    .role(Role::ListBoxOption)
                    .aria_label(entry.place.spoken.clone())
                    .aria_selected(on)
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .px(px(8.))
                    .rounded(px(6.))
                    .when(on, |row| row.bg(ink(0.10)))
                    .hover(|s| s.bg(ink(0.07)))
                    .child(entry.place.shown())
                    .on_click(cx.listener(move |this, _, window, cx| this.pin(index, window, cx)))
            });
        let list = div()
            .id("orchestrator-picker")
            .role(Role::ListBox)
            .aria_label("Projects and chats")
            .occlude()
            .w_full()
            .max_w(px(PILL_W - 44.))
            .flex()
            .flex_col()
            .p(px(4.))
            .rounded(px(radius_lg()))
            .border_1()
            .border_color(ink(0.10))
            .bg(glass(0.96))
            .text_size(px(fs_sm()))
            .children(rows);
        Some(list.into_any_element())
    }
}

impl Render for Orchestrator {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_orchestrator(window, cx)
    }
}

impl Orchestrator {
    fn render_orchestrator(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let dark = applied(cx)
            .backdrop
            .scheme
            .map_or_else(|| theme(cx).is_dark(), |scheme| scheme == Scheme::Dark);
        LIGHT.store(!dark, Ordering::Relaxed);
        // The field keeps its own colors, so they follow the backdrop here.
        self.field.update(cx, |field, _| {
            field.text_color = Some(ink(1.));
            field.placeholder_color = Some(ink(0.45));
        });
        let empty = self.field.read(cx).content.trim().is_empty();
        let asked = self.asked().map(|(_, waiting)| waiting.ask.kind.clone());
        // What Enter does, which is what the button does.
        let (action, glyph, ready) = match &asked {
            Some(AskKind::Permission) => ("Allow", IconName::CheckBold, true),
            Some(AskKind::Question(_)) => ("Answer", IconName::ArrowUpBold, !empty),
            Some(AskKind::InChat) => ("Open the chat", IconName::ArrowRightBold, true),
            None => ("Send", IconName::ArrowUpBold, !empty),
        };
        let workspace = self.workspace.upgrade();
        let (chats, projects) = workspace.as_ref().map_or_else(Default::default, |w| {
            let workspace = w.read(cx);
            (workspace.chats.clone(), workspace.settings.projects.clone())
        });
        let cards: Vec<AnyElement> = self
            .requests
            .iter()
            .map(|request| self.card(request, &chats, &projects, cx))
            .collect();
        let shadow = hsla(0., 0., 0., if dark { 0.22 } else { 0.08 });
        let before = self.before_field(cx);
        let tight = before.is_none();
        let pill = pill("orchestrator-pill", ink(0.10), glass(0.62), shadow)
            .w_full()
            .max_w(px(PILL_W))
            .h(px(PILL_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(PILL_PAD))
            .text_size(px(fs_lg()))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.escape(window, cx) {
                    cx.stop_propagation();
                }
            }))
            // Files, folders or pictures held over the pill light it up; dropped, they go with
            // the request.
            .drag_over::<ExternalPaths>(|style, _, _, _| {
                style.border_color(ink(0.45)).bg(glass(0.8))
            })
            .on_drop(cx.listener(|this, files: &ExternalPaths, window, cx| {
                this.attach(files.paths(), cx);
                this.focus(window, cx);
                cx.stop_propagation();
            }))
            .child(self.attach_button(cx))
            .children(before)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    // Closer to the plus button than the row's gap, as in the chat box.
                    .when(tight, |d| d.ml(px(-4.)))
                    .child(self.field.clone()),
            )
            .children(self.after_field(cx))
            .child(
                send_button("orchestrator-send", action, ready, ink(1.), glass(1.))
                    .rounded_full()
                    .child(icon(glyph, 16.))
                    .on_click(cx.listener(|this, _, window, cx| this.enter(window, cx))),
            )
            .children(self.attach_menu_view());
        // What is attached sits above the pill, laid over the page so the pill does not move.
        let attachments = self.attachment_row(shadow, cx);
        let model = self.model_chip(window, cx);
        let requests = div()
            .id("orchestrator-requests")
            .role(Role::List)
            .aria_label("Requests")
            .w_full()
            .max_w(px(PILL_W))
            .mt(px(12.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .when(!cards.is_empty(), |list| {
                list.child(
                    div().flex().child(
                        button("orchestrator-clear", "Clear all")
                            .px(px(8.))
                            .py(px(2.))
                            .rounded_full()
                            .text_size(px(fs_sm()))
                            .text_color(ink(0.6))
                            .hover(|s| s.bg(ink(0.06)).text_color(ink(0.9)))
                            .child("Clear all")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.requests.clear();
                                cx.notify();
                            })),
                    ),
                )
            })
            .children(cards);
        let column = || {
            div()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .px(px(GUTTER))
                .child(div().flex_none().h(relative(PILL_TOP)))
        };
        // The list opens under the pill and over the cards, so it is laid out after them.
        let picker = self.picker_list(cx).map(|list| {
            column()
                .child(div().flex_none().h(px(PILL_H + 6.)))
                .child(list)
        });
        div()
            .id("orchestrator")
            .role(Role::Main)
            .aria_label("Orchestrator")
            .relative()
            .size_full()
            .overflow_hidden()
            .font_family(font_ui())
            .text_color(ink(0.9))
            .child(
                column()
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .max_w(px(PILL_W))
                            .flex_none()
                            .children(attachments)
                            .child(pill),
                    )
                    .children(model)
                    .child(requests),
            )
            .children(picker)
    }
}

/// The page's backdrop as a view of its own, which the workspace draws under the page: a
/// backdrop that moves redraws only itself, and the pill and cards over it keep their frame.
pub struct OrchestratorBackdrop {
    page: WeakEntity<Orchestrator>,
    _changes: Subscription,
}

impl OrchestratorBackdrop {
    pub fn new(page: &Entity<Orchestrator>, cx: &mut Context<Self>) -> Self {
        Self {
            page: page.downgrade(),
            _changes: cx.observe(page, |_, _, cx| cx.notify()),
        }
    }
}

impl Render for OrchestratorBackdrop {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let backdrop = self.page.update(cx, |page, cx| page.backdrop(cx)).ok();
        div().size_full().children(backdrop)
    }
}
