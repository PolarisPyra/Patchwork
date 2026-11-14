//! The orchestrator page drawn: a dark pill to write a request in, over the backdrop, with the
//! place the request would go at its end, and under it a card for each request sent.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{
    AnyElement, Bounds, BoxShadow, Hsla, KeyDownEvent, ObjectFit, Pixels, Role, SharedString,
    Window, div, hsla, img, point, prelude::*, px, relative, size,
};
use patchwork_server::orchestration::Target;
use patchwork_server::themes::{Applied, Fit, Scene, Scheme, Shown};
use patchwork_server::threads::AskKind;

use gpui::{Anchor, FontWeight, MouseButton, anchored, deferred};

use super::state::Orchestrator;
use super::{background, scene};
use crate::ui::theme::{font_ui, FS_LG, FS_SM, radius_lg, applied, theme};
use crate::ui::{agent_logo, button, display_title, icon, spinner};

/// The pill: its size, the room kept at its sides, and how far down the page its top is.
const PILL_W: f32 = 640.;
const PILL_H: f32 = 52.;
const GUTTER: f32 = 24.;
const PILL_TOP: f32 = 0.4;

/// Whether the page is over a light backdrop, so its text is dark. The page's render sets it
/// before anything on the page is drawn, and the one window draws on one thread.
static LIGHT: AtomicBool = AtomicBool::new(false);

fn light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// The page's text color, at this strength.
pub(super) fn ink(alpha: f32) -> Hsla {
    if light() {
        hsla(0.6, 0.30, 0.12, alpha)
    } else {
        hsla(0.6, 0.25, 0.96, alpha)
    }
}

/// The page's glass, this opaque: dark over a dark backdrop, pale over a light one.
pub(super) fn glass(alpha: f32) -> Hsla {
    if light() {
        hsla(0.6, 0.30, 0.985, alpha)
    } else {
        hsla(0.64, 0.30, 0.05, alpha)
    }
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
    match &look.backdrop {
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

/// A small label inside the pill.
fn chip(label: impl Into<SharedString>) -> gpui::Div {
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
        .text_size(px(FS_SM))
        .text_color(ink(0.8))
        .child(div().min_w_0().truncate().child(label.into()))
}

impl Orchestrator {
    /// What is drawn behind the page: what the theme settings came to.
    fn backdrop(&self, cx: &Context<Self>) -> AnyElement {
        let look = applied(cx);
        let still = look.reduce_motion;
        let t = theme(cx);
        match &look.backdrop {
            Shown::None => div().size_full().bg(t.bg_app).into_any_element(),
            Shown::Butterfly => {
                let flight = Rc::clone(&self.flight);
                background::field(pill_bounds, still, flight, None).into_any_element()
            }
            // The picture is dimmed toward the window's color, so the page reads over it.
            Shown::Image { path, dim, fit } => div()
                .size_full()
                .relative()
                .bg(t.bg_app)
                .child(img(PathBuf::from(path)).size_full().object_fit(match fit {
                    Fit::Cover => ObjectFit::Cover,
                    Fit::Contain => ObjectFit::Contain,
                    Fit::Stretch => ObjectFit::Fill,
                }))
                .child(div().absolute().inset_0().bg(t.bg_app.opacity(*dim)))
                .into_any_element(),
            Shown::Scene(_) => {
                let fallback = scene::Fallback {
                    surface: t.bg_app,
                    faint: t.text_muted,
                };
                scene::field(look, shown_scene, fallback, pill_bounds, still, None)
                    .into_any_element()
            }
        }
    }

    /// A place a request can go, as its chip names it: a chat of a project under the project's
    /// name, and a new chat by whether it is in a project.
    fn place_name(&self, target: &Target, cx: &Context<Self>) -> String {
        let Some(workspace) = self.workspace.upgrade() else {
            return String::new();
        };
        let workspace = workspace.read(cx);
        let project = |id: &String| {
            let found = workspace.settings.projects.iter().find(|p| &p.id == id);
            found.map_or("Project", |p| p.name.as_str())
        };
        match target {
            Target::Chat(id) => {
                let Some(thread) = workspace.threads.iter().find(|t| &t.id == id) else {
                    return "Chat".into();
                };
                let title = display_title(&thread.title);
                match &thread.project_id {
                    Some(id) => format!("{} › {title}", project(id)),
                    None => title,
                }
            }
            Target::New(Some(id)) => format!("New chat in {}", project(id)),
            Target::New(None) => "New chat, no project".into(),
            Target::Split(ids) => {
                let names: Vec<&str> = ids.iter().map(project).collect();
                format!("New chats in {}", names.join(", "))
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
        if let Some(pinned) = &self.pinned {
            let name = self.place_name(pinned, cx);
            let pin = div().flex_none().child(icon("push-pin", 12.));
            let shown = chip(format!("→ {name}")).child(pin);
            return Some(
                status(format!("Goes to {name}, pinned"))
                    .child(shown)
                    .into_any_element(),
            );
        }
        let routing = self.routing.as_ref().filter(|_| self.route_is_fresh(cx));
        let Some(routing) = routing else {
            return self.deciding.then(|| {
                status("Finding where it goes".into())
                    .px(px(6.))
                    .child(spinner(14., ink(0.7)))
                    .into_any_element()
            });
        };
        // Both places show while the router cannot tell them apart; the arrows pick one.
        let shown = if routing.unsure && self.pick < 2 {
            0..2
        } else {
            self.pick..self.pick + 1
        };
        let picked = routing
            .targets
            .get(self.pick)
            .map(|t| self.place_name(t, cx));
        let chips = shown.clone().filter_map(|index| {
            let name = self.place_name(routing.targets.get(index)?, cx);
            let dim = shown.len() > 1 && index != self.pick;
            Some(
                chip(format!("→ {name}"))
                    .max_w(px(if shown.len() > 1 { 130. } else { 220. }))
                    .opacity(if dim { 0.45 } else { 1. }),
            )
        });
        let label = format!("Goes to {}", picked.unwrap_or_default());
        Some(status(label).children(chips).into_any_element())
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
                    .aria_label(entry.label.clone())
                    .aria_selected(on)
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .px(px(12.))
                    .rounded(px(6.))
                    .when(on, |row| row.bg(ink(0.10)))
                    .hover(|s| s.bg(ink(0.07)))
                    .child(div().min_w_0().truncate().child(entry.label))
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
            .text_size(px(FS_SM))
            .children(rows);
        Some(list.into_any_element())
    }
}

/// A chip under the pill that opens a menu.
fn launch_chip(id: &'static str, label: String, open: bool) -> gpui::Stateful<gpui::Div> {
    button(id, label)
        .aria_expanded(open)
        .h(px(28.))
        .gap(px(6.))
        .px(px(10.))
        .rounded_full()
        .text_size(px(FS_SM))
        .font_weight(FontWeight::MEDIUM)
        .text_color(ink(0.7))
        .when(open, |chip| chip.bg(ink(0.08)))
        .hover(|s| s.bg(ink(0.08)).text_color(ink(1.)))
        // The press leaves the keys in the field, where they go back once a pick is made.
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
}

/// A chip's open menu, placed under the chip.
fn under(menu: Option<AnyElement>) -> Option<gpui::Div> {
    let menu = menu?;
    Some(
        div().absolute().bottom_0().left_0().child(
            deferred(
                anchored()
                    .anchor(Anchor::TopLeft)
                    .offset(point(px(0.), px(4.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(menu),
            )
            .with_priority(1),
        ),
    )
}

impl Orchestrator {
    /// The chips under the pill: the agent and model a request's new chat starts with, then its
    /// effort, each with the menu that changes it open under it.
    fn model_chip(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let settings = &self.workspace.upgrade()?.read(cx).settings;
        let composer = self.composer.read(cx);
        let (agent, model) = composer.launch_pick(settings);
        let name = composer.model_name(agent, model.as_deref());
        let effort = composer.launch_effort_name(settings);
        let (open, effort_open) = (composer.launch_picker_open(), composer.launch_effort_open());
        let (picker, levels) = self.composer.update(cx, |composer, cx| {
            let picker = composer.launch_picker(window, cx);
            (picker, composer.launch_effort_menu(cx))
        });
        let composer = self.composer.clone();
        let model_chip = launch_chip("orchestrator-model", format!("Choose model: {name}"), open)
            .child(agent_logo(agent, model.as_deref(), 14.))
            .child(div().max_w(px(220.)).truncate().child(name))
            .child(icon("caret-down", 12.))
            .on_click(move |event, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.toggle_launch_picker(event, window, cx);
                });
            });
        let composer = self.composer.clone();
        let effort_chip = effort.map(|effort| {
            let chip = launch_chip(
                "orchestrator-effort",
                format!("Effort: {effort}"),
                effort_open,
            )
            .child(icon("lightbulb", 14.))
            .child(effort)
            .child(icon("caret-down", 12.))
            .on_click(move |event, window, cx| {
                composer.update(cx, |composer, cx| {
                    composer.toggle_launch_effort(event, window, cx);
                });
            });
            div().relative().flex().child(chip).children(under(levels))
        });
        let row = div()
            .w_full()
            .max_w(px(PILL_W))
            .mt(px(6.))
            .px(px(12.))
            .flex()
            .gap(px(2.))
            .child(
                div()
                    .relative()
                    .flex()
                    .child(model_chip)
                    .children(under(picker)),
            )
            .children(effort_chip);
        Some(row.into_any_element())
    }
}

impl Render for Orchestrator {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = applied(cx)
            .backdrop_scheme
            .map_or_else(|| theme(cx).is_dark(), |scheme| scheme == Scheme::Dark);
        LIGHT.store(!dark, Ordering::Relaxed);
        // The field keeps its own colors, so they follow the backdrop here.
        self.field.update(cx, |field, _| {
            field.text_color = ink(1.);
            field.placeholder_color = ink(0.45);
        });
        let empty = self.field.read(cx).content.trim().is_empty();
        let asked = self.asked().map(|(_, waiting)| waiting.ask.kind.clone());
        // What Enter does, which is what the button does.
        let (action, glyph, ready) = match &asked {
            Some(AskKind::Permission) => ("Allow", "check-bold", true),
            Some(AskKind::Question(_)) => ("Answer", "arrow-up-bold", !empty),
            Some(AskKind::InChat) => ("Open the chat", "arrow-right-bold", true),
            None => ("Send", "arrow-up-bold", !empty),
        };
        let workspace = self.workspace.upgrade();
        let (threads, projects) = workspace.as_ref().map_or_else(Default::default, |w| {
            let workspace = w.read(cx);
            (
                workspace.threads.clone(),
                workspace.settings.projects.clone(),
            )
        });
        let cards: Vec<AnyElement> = self
            .requests
            .iter()
            .map(|request| self.card(request, &threads, &projects, cx))
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
            .pl(px(if asked.is_some() || !self.waiting.is_empty() {
                12.
            } else {
                22.
            }))
            .pr(px(8.))
            .rounded_full()
            .border_1()
            .border_color(ink(0.10))
            .bg(glass(0.62))
            .shadow(vec![BoxShadow {
                color: hsla(0., 0., 0., if dark { 0.22 } else { 0.08 }),
                offset: point(px(0.), px(6.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .text_size(px(FS_LG))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" && this.escape(window, cx) {
                    cx.stop_propagation();
                }
            }))
            .children(self.before_field(cx))
            .child(div().flex_1().min_w_0().child(self.field.clone()))
            .children(self.after_field(cx))
            .child(
                button("orchestrator-send", action)
                    .size(px(36.))
                    .flex_none()
                    .justify_center()
                    .rounded_full()
                    .bg(ink(if ready { 0.92 } else { 0.14 }))
                    .text_color(if ready { glass(1.) } else { ink(0.5) })
                    .child(icon(glyph, 16.))
                    .on_click(cx.listener(|this, _, window, cx| this.enter(window, cx))),
            );
        let model = self.model_chip(window, cx);
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
        let backdrop = self.backdrop(cx);
        div()
            .id("orchestrator")
            .role(Role::Main)
            .aria_label("Orchestrator")
            .relative()
            .size_full()
            .overflow_hidden()
            .font_family(font_ui())
            .text_color(ink(0.9))
            .child(div().absolute().inset_0().child(backdrop))
            .child(column().child(pill).children(model).child(requests))
            .children(picker)
    }
}
