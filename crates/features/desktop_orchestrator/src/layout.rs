//! The orchestrator page drawn: the chat box over the backdrop, the question an agent is waiting
//! on above it, and under it a card for each request sent.

// Rust guideline compliant 2026-10-09

use patchwork_client::AppState;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{
    AnyElement, App, Bounds, Entity, Hsla, ObjectFit, Pixels, Role, SharedString, Subscription,
    WeakEntity, Window, div, hsla, img, point, prelude::*, px, size,
};
use patchwork_client::local::core::orchestration::Target;
use patchwork_model::chats::AskKind;
use patchwork_themes::{Applied, Scene, Scheme, Shown};

use super::requests::Orchestrator;
use patchwork_design_backgrounds::{Fallback, butterfly_field, preset_fill, scene_field};
use patchwork_design_components::button;
use patchwork_design_icons::IconName;
use patchwork_design_icons::icon;
use patchwork_design_text::display_title;
use patchwork_design_theme::{Theme, applied, font_ui, fs_sm, glass_over, ink_over, radius_lg, theme};

/// The chat box: as wide as it grows, about as tall as it is empty, the room kept at its sides,
/// and the room under it, as a chat keeps them.
const BOX_W: f32 = 760.;
const BOX_H: f32 = 128.;
const GUTTER: f32 = 24.;
const BOX_BOTTOM: f32 = 16.;
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

/// Where the chat box is on a page with these bounds, which is where the layout below puts it.
/// The backdrop draws what is behind it as glows.
fn box_bounds(page: Bounds<Pixels>) -> Bounds<Pixels> {
    let width = (page.size.width - px(GUTTER * 2.)).clamp(px(0.), px(BOX_W));
    Bounds::new(
        point(
            page.center().x - width / 2.,
            page.bottom() - px(BOX_BOTTOM + BOX_H),
        ),
        size(width, px(BOX_H)),
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

/// A small label on the page.
fn chip(label: impl Into<SharedString>) -> gpui::Div {
    chip_frame().child(div().min_w_0().truncate().child(label.into()))
}

/// The shape the page's chips share.
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
                butterfly_field(box_bounds, still, flight, None).into_any_element()
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
                scene_field(look, shown_scene, fallback, box_bounds, still, None).into_any_element()
            }
        }
    }

    /// A place a request can go, as its pinned chip shows it: a folder and the project's name
    /// first for a chat in a project, and a chat bubble for one in none.
    pub(super) fn place(&self, target: &Target, cx: &App) -> Place {
        let project = |id: &String| {
            let found = AppState::get(cx)
                .settings
                .projects
                .iter()
                .find(|p| &p.id == id);
            found.map_or("Project", |p| p.name.as_str())
        };
        match target {
            Target::Chat(id) => {
                let Some(chat) = AppState::get(cx).chats.iter().find(|t| &t.id == id) else {
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

    /// The place `@` pinned, in the chat box's row, with a button that unpins it.
    pub(super) fn pinned_chip(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let place = self.place(self.pinned.as_ref()?, cx);
        let (hover, fg) = (t.bg_hover, t.text);
        let unpin = button("orchestrator-unpin", "Unpin")
            .size(px(16.))
            .flex_none()
            .justify_center()
            .rounded_full()
            .hover(move |s| s.bg(hover).text_color(fg))
            .child(icon(IconName::X, 10.))
            .on_click(cx.listener(|this, _, _, cx| {
                this.pinned = None;
                cx.notify();
            }));
        let chip = div()
            .id("orchestrator-route")
            .role(Role::Status)
            .aria_label(format!("Goes to {}, pinned", place.spoken))
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(4.))
            .text_color(t.text_muted)
            .child(div().flex_none().child("→"))
            .child(place.shown())
            .child(unpin);
        Some(chip.into_any_element())
    }

    /// The question an agent is waiting on, above the chat box: its chat and what it asks, how
    /// to answer it, and the next one while several wait. Put aside, it is a count to bring
    /// them back.
    fn question_strip(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let count = self.waiting.len();
        if self.declined {
            return (count > 0).then(|| {
                div()
                    .flex()
                    .child(
                        button("orchestrator-waiting", format!("{count} waiting. Answer"))
                            .child(chip(format!("{count} waiting")).text_color(warm()))
                            .rounded_full()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.declined = false;
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            });
        }
        let (index, waiting) = self.asked()?;
        let action = |id: &'static str, label: &'static str, strong: bool| {
            button(id, label)
                .h(px(28.))
                .flex_none()
                .px(px(12.))
                .rounded_full()
                .text_size(px(fs_sm()))
                .map(|b| {
                    if strong {
                        b.bg(ink(0.9))
                            .text_color(glass(1.))
                            .hover(|s| s.bg(ink(1.)))
                    } else {
                        b.bg(ink(0.08)).hover(|s| s.bg(ink(0.14)))
                    }
                })
                .child(label)
        };
        let actions: Vec<AnyElement> = match &waiting.ask.kind {
            AskKind::Permission => vec![
                action("orchestrator-deny", "Deny", false)
                    .on_click(cx.listener(|this, _, _, cx| this.respond(false, None, cx)))
                    .into_any_element(),
                action("orchestrator-allow", "Allow", true)
                    .on_click(cx.listener(|this, _, _, cx| this.respond(true, None, cx)))
                    .into_any_element(),
            ],
            AskKind::InChat => vec![
                action("orchestrator-open-asking", "Open chat", true)
                    .on_click(cx.listener(|this, _, _, cx| this.open_asking(cx)))
                    .into_any_element(),
            ],
            // The chat box answers it.
            AskKind::Question(_) => Vec::new(),
        };
        let quiet = |id: &'static str, label: &'static str, glyph: IconName| {
            button(id, label)
                .size(px(28.))
                .flex_none()
                .justify_center()
                .rounded_full()
                .text_color(ink(0.6))
                .hover(|s| s.bg(ink(0.08)).text_color(ink(0.9)))
                .child(icon(glyph, 14.))
        };
        let several = count > 1;
        let strip = div()
            .id("orchestrator-asking")
            .role(Role::Status)
            .aria_label(waiting.question())
            .occlude()
            .w_full()
            .min_h(px(44.))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(14.))
            .pr(px(8.))
            .py(px(8.))
            .rounded(px(radius_lg()))
            .border_1()
            .border_color(ink(0.10))
            .bg(glass(0.72))
            .text_size(px(fs_sm()))
            .text_color(ink(0.9))
            .child(
                icon(IconName::CircleFill, 8.)
                    .flex_none()
                    .text_color(warm()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(waiting.question()),
            )
            .when(several, |strip| {
                strip
                    .child(
                        div()
                            .flex_none()
                            .text_color(ink(0.6))
                            .child(format!("{} of {count}", index + 1)),
                    )
                    .child(
                        quiet("orchestrator-next", "Next question", IconName::CaretRight)
                            .on_click(cx.listener(|this, _, _, cx| this.next_question(cx))),
                    )
            })
            .children(actions)
            .child(
                quiet("orchestrator-aside", "Put the questions aside", IconName::X).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.declined = true;
                        cx.notify();
                    }),
                ),
            );
        Some(strip.into_any_element())
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
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let dark = applied(cx)
            .backdrop
            .scheme
            .map_or_else(|| theme(cx).is_dark(), |scheme| scheme == Scheme::Dark);
        LIGHT.store(!dark, Ordering::Relaxed);
        let state = AppState::get(cx);
        let (chats, projects) = (Arc::clone(&state.chats), state.settings.projects.clone());
        let cards: Vec<AnyElement> = self
            .requests
            .iter()
            .map(|request| self.card(request, &chats, &projects, cx))
            .collect();
        // The question sits on the line above the chat box.
        let strip = self.question_strip(cx).map(|strip| {
            div()
                .w_full()
                .max_w(px(BOX_W))
                .mx_auto()
                .pb(px(8.))
                .child(strip)
        });
        let requests = div()
            .id("orchestrator-requests")
            .role(Role::List)
            .aria_label("Requests")
            .w_full()
            .max_w(px(BOX_W))
            .py(px(12.))
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
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("orchestrator-scroll")
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .items_center()
                            .px(px(GUTTER))
                            // The newest requests sit just over the chat box, as a chat's do.
                            .child(div().flex_1())
                            .child(requests),
                    )
                    .child(
                        div()
                            .w_full()
                            .flex_none()
                            .px(px(GUTTER))
                            .pb(px(BOX_BOTTOM))
                            // The chat box keeps the theme's text color, not the backdrop's.
                            .text_color(theme(cx).text)
                            .children(strip)
                            .child(self.composer.clone()),
                    ),
            )
    }
}

/// The page's backdrop as a view of its own, which the workspace draws under the page: a
/// backdrop that moves redraws only itself, and the chat box and cards over it keep their frame.
pub struct OrchestratorBackdrop {
    page: WeakEntity<Orchestrator>,
    _changes: Subscription,
}

impl std::fmt::Debug for OrchestratorBackdrop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrchestratorBackdrop")
            .finish_non_exhaustive()
    }
}

impl OrchestratorBackdrop {
    /// The backdrop of `page`, drawn again whenever the page is.
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
