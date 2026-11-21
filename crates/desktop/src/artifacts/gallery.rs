//! The gallery: every artifact as a card with its picture or as a row, filtered by kind and by
//! where it was made, searched, and grouped with the pinned first, then by project, with tiles
//! that start a chat making a new one.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, App, Div, FontWeight, KeyDownEvent, MouseButton, MouseDownEvent, ObjectFit, Role,
    SharedString, Stateful, Window, div, img, prelude::*, px,
};
use patchwork_core::artifacts::{Artifact, ArtifactKind};

use super::render::kind_icon;
use super::state::{ArtifactsPage, Place};
use crate::ui::theme::{Theme, fs_md, fs_sm, fs_xxl, radius, radius_sm};
use crate::ui::{
    button, focus_ring, format_relative, icon, icon_button, input_box, scrollbar, spinner, tooltip,
};

/// A card's width; its picture is 16 by 10, as the pictures are taken.
const CARD_WIDTH: f32 = 248.;

/// The group a card's picture and its border hover as.
const PICTURE: &str = "artifact-picture";

/// The gallery's widest, so cards stay in tidy rows on a wide window.
const MAX_WIDTH: f32 = 1120.;

impl ArtifactsPage {
    /// The title, the search, and the switch between cards and rows.
    fn top(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> Div {
        let focused = gpui::Focusable::focus_handle(self.search.read(cx), cx).is_focused(window);
        let grid = self.grid;
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .text_size(px(fs_xxl()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Artifacts"),
            )
            .child(
                input_box(focused, t)
                    .w(px(240.))
                    .child(icon("magnifying-glass", 14.))
                    .child(div().flex_1().min_w_0().child(self.search.clone())),
            )
            .child(
                icon_button(
                    "artifacts-layout",
                    if grid {
                        "Show as list"
                    } else {
                        "Show as cards"
                    },
                    if grid { "list-bullets" } else { "square" },
                    16.,
                    cx,
                )
                .tooltip(tooltip(if grid {
                    "Show as list"
                } else {
                    "Show as cards"
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.set_grid(!grid, cx))),
            )
    }

    /// A row of tabs named `name`, the one at `at` picked, each picking its value with `pick`.
    fn tab_row<T: Clone + 'static>(
        &self,
        name: &'static str,
        options: Vec<(String, T)>,
        at: usize,
        pick: fn(&mut Self, T, &mut Context<Self>),
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (fg, muted, selected) = (t.text, t.text_muted, t.bg_selected_muted);
        let values: Vec<T> = options.iter().map(|(_, v)| v.clone()).collect();
        let count = options.len();
        let tabs = options.into_iter().enumerate().map(|(i, (label, value))| {
            let on = i == at;
            let all = values.clone();
            div()
                .id(SharedString::from(format!("artifacts-{name}-{i}")))
                .role(Role::Tab)
                .aria_label(label.clone())
                .aria_selected(on)
                .tab_index(0)
                .focus_visible(focus_ring)
                .cursor_pointer()
                .h(px(26.))
                .px(px(12.))
                .flex()
                .items_center()
                .rounded_full()
                .text_size(px(fs_sm()))
                .text_color(if on { fg } else { muted })
                .when(on, |d| d.bg(selected))
                .hover(move |s| s.text_color(fg))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| pick(this, value.clone(), cx)))
                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
                    let to = match e.keystroke.key.as_str() {
                        "left" => (i + count - 1) % count,
                        "right" => (i + 1) % count,
                        _ => return,
                    };
                    pick(this, all[to].clone(), cx);
                }))
        });
        div()
            .id(SharedString::from(format!("artifacts-{name}")))
            .role(Role::TabList)
            .aria_label(name)
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .children(tabs)
    }

    /// All, then each kind there is an artifact of.
    fn filters(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let kinds: Vec<(String, Option<ArtifactKind>)> = std::iter::once(("All".to_owned(), None))
            .chain(
                ArtifactKind::ALL
                    .into_iter()
                    .filter(|k| self.list.iter().any(|a| a.kind == *k))
                    .map(|k| (k.label().to_owned(), Some(k))),
            )
            .collect();
        let at = kinds
            .iter()
            .position(|(_, k)| *k == self.filter)
            .unwrap_or(0);
        self.tab_row("Kinds", kinds, at, Self::set_filter, t, cx)
    }

    /// Everywhere, the chat open when the page opened, then each project there is an artifact
    /// from; None when there is only everywhere.
    fn places(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let mut places = vec![("Everywhere".to_owned(), Place::All)];
        if self
            .came_from
            .as_ref()
            .is_some_and(|c| self.list.iter().any(|a| &a.thread_id == c))
        {
            places.push(("This chat".to_owned(), Place::Chat));
        }
        let mut projects: Vec<(String, Place)> = Vec::new();
        for a in &self.list {
            if let Some((id, name)) = self.projects.get(&a.thread_id)
                && !projects
                    .iter()
                    .any(|(_, p)| *p == Place::Project(id.clone()))
            {
                projects.push((name.clone(), Place::Project(id.clone())));
            }
        }
        projects.sort_by_key(|(name, _)| name.to_lowercase());
        places.extend(projects);
        if places.len() < 2 {
            return None;
        }
        let at = places
            .iter()
            .position(|(_, p)| *p == self.place)
            .unwrap_or(0);
        Some(self.tab_row("Places", places, at, Self::set_place, t, cx))
    }

    /// The tiles that start a chat making a doc, deck, sheet, design, dashboard or board.
    fn new_tiles(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let (hover, border) = (t.bg_hover, t.border);
        let tiles = ArtifactKind::MADE_NEW.into_iter().map(|kind| {
            button(
                SharedString::from(format!("artifacts-new-{}", kind.name())),
                format!("New {}", kind.label()),
            )
            .flex_col()
            .justify_center()
            .gap(px(8.))
            .w(px(112.))
            .h(px(84.))
            .rounded(px(radius()))
            .border_1()
            .border_color(border)
            .bg(t.bg_panel)
            .hover(move |s| s.bg(hover))
            .child(
                div()
                    .text_color(t.text_muted)
                    .child(icon(kind_icon(kind), 22.)),
            )
            .child(div().text_size(px(fs_sm())).child(kind.label()))
            .on_click(cx.listener(move |this, _, _, cx| this.make_new(kind, cx)))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(px(fs_md()))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Make something new"),
            )
            .child(
                div()
                    .id("artifacts-new")
                    .role(Role::Group)
                    .aria_label("Make something new")
                    .flex()
                    .flex_wrap()
                    .gap(px(10.))
                    .children(tiles),
            )
    }

    /// An artifact's picture, a spinner while it is taken, or its kind's icon.
    fn picture(&self, artifact: &Artifact, t: &Theme, cx: &App) -> AnyElement {
        let frame = div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(t.text_subtle);
        match self.thumbs.read(cx).get(&artifact.id, artifact.versions) {
            // The picture keeps the frame's corners, which clipping alone does not.
            Some(Some(path)) => img(path.clone())
                .size_full()
                .rounded(px(radius()))
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None if artifact.kind.drawn() => {
                frame.child(spinner(16., t.text_subtle)).into_any_element()
            }
            _ => frame
                .child(icon(kind_icon(artifact.kind), 28.))
                .into_any_element(),
        }
    }

    /// An artifact as a card: its picture, title, kind and when it changed.
    fn card(&self, artifact: &Artifact, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let (hover, border) = (t.border_strong, t.border);
        let id = artifact.id.clone();
        button(
            SharedString::from(format!("artifact-{}", artifact.id)),
            artifact.title.clone(),
        )
        .role(Role::ListItem)
        .flex_col()
        .items_start()
        .gap(px(8.))
        .w(px(CARD_WIDTH))
        .child(
            div()
                .group(PICTURE)
                .relative()
                .w(px(CARD_WIDTH))
                .h(px(CARD_WIDTH * 10. / 16.))
                .overflow_hidden()
                .rounded(px(radius()))
                .bg(t.bg_editor)
                .child(self.picture(artifact, t, cx))
                // The border is drawn over the picture, so the picture's edge never shows past
                // its curve at the corners.
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .rounded(px(radius()))
                        .border_1()
                        .border_color(border)
                        .group_hover(PICTURE, move |s| s.border_color(hover)),
                )
                .when(artifact.pinned, |d| {
                    d.child(pin_marker(t).absolute().top(px(6.)).right(px(6.)))
                }),
        )
        .child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .text_size(px(fs_md()))
                        .child(artifact.title.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(fs_sm()))
                        .text_color(t.text_muted)
                        .child(format!(
                            "{} · {}",
                            artifact.kind.label(),
                            format_relative(artifact.updated_at)
                        )),
                ),
        )
        .map(|b| self.with_menu(b, artifact, cx))
        .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)))
    }

    /// `item` with the artifact's menu on a right click, and its state said.
    fn with_menu(
        &self,
        item: Stateful<Div>,
        artifact: &Artifact,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = artifact.id.clone();
        item.when(artifact.pinned, |b| {
            b.aria_label(format!("{}, pinned", artifact.title))
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                this.open_menu(&id, e.position, window, cx);
                cx.stop_propagation();
            }),
        )
    }

    /// An artifact as a row: its icon, title, kind, chat and when it changed.
    fn row(&self, artifact: &Artifact, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let hover = t.bg_hover;
        let id = artifact.id.clone();
        let chat = self
            .chats
            .get(&artifact.thread_id)
            .cloned()
            .unwrap_or_default();
        let cell = |text: String, width: f32| {
            div()
                .flex_none()
                .w(px(width))
                .truncate()
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(text)
        };
        button(
            SharedString::from(format!("artifact-{}", artifact.id)),
            artifact.title.clone(),
        )
        .role(Role::ListItem)
        .gap(px(12.))
        .px(px(10.))
        .h(px(40.))
        .rounded(px(radius_sm()))
        .border_b_1()
        .border_color(t.border)
        .hover(move |s| s.bg(hover))
        .child(
            div()
                .flex_none()
                .text_color(t.text_muted)
                .child(icon(kind_icon(artifact.kind), 16.)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(fs_md()))
                .child(artifact.title.clone()),
        )
        .when(artifact.pinned, |d| d.child(pin_marker(t)))
        .child(cell(artifact.kind.label().to_owned(), 90.))
        .child(cell(chat, 200.))
        .child(cell(format_relative(artifact.updated_at), 80.))
        .map(|b| self.with_menu(b, artifact, cx))
        .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)))
    }

    /// The artifacts shown in groups, each with its heading: the pinned first, then each
    /// project's, then those from chats in no project.
    fn sections(&self, cx: &App) -> Vec<(String, Vec<Artifact>)> {
        let mut sections: Vec<(String, Vec<Artifact>)> = Vec::new();
        let mut put =
            |heading: &str, a: &Artifact| match sections.iter_mut().find(|(h, _)| h == heading) {
                Some((_, list)) => list.push(a.clone()),
                None => sections.push((heading.to_owned(), vec![a.clone()])),
            };
        let shown = self.matching(cx);
        for a in shown.iter().filter(|a| a.pinned) {
            put("Pinned", a);
        }
        for a in shown.iter().filter(|a| !a.pinned) {
            if let Some((_, name)) = self.projects.get(&a.thread_id) {
                put(name, a);
            }
        }
        for a in shown
            .iter()
            .filter(|a| !a.pinned && !self.projects.contains_key(&a.thread_id))
        {
            put("Other chats", a);
        }
        sections
    }

    /// The gallery: its top, the tiles, the filters, and the artifacts.
    pub(super) fn gallery(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let sections = self.sections(cx);
        let headed = sections.len() > 1;
        let body = if sections.is_empty() {
            div()
                .py(px(48.))
                .flex()
                .justify_center()
                .text_color(t.text_muted)
                .child(if self.list.is_empty() {
                    "No artifacts yet"
                } else {
                    "No artifacts match"
                })
                .into_any_element()
        } else {
            let groups: Vec<Div> = sections
                .into_iter()
                .enumerate()
                .map(|(i, (heading, shown))| {
                    let items: Vec<Stateful<Div>> = shown
                        .iter()
                        .map(|a| {
                            if self.grid {
                                self.card(a, t, cx)
                            } else {
                                self.row(a, t, cx)
                            }
                        })
                        .collect();
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .when(headed, |d| {
                            d.child(
                                div()
                                    .id(("artifacts-heading", i))
                                    .role(Role::Heading)
                                    .text_size(px(fs_md()))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(heading.clone()),
                            )
                        })
                        .child(
                            div()
                                .id(("artifacts-items", i))
                                .role(Role::List)
                                .aria_label(if headed {
                                    heading
                                } else {
                                    "Artifacts".to_owned()
                                })
                                .flex()
                                .when(self.grid, |d| d.flex_wrap().gap(px(20.)))
                                .when(!self.grid, gpui::Styled::flex_col)
                                .children(items),
                        )
                })
                .collect();
            div()
                .flex()
                .flex_col()
                .gap(px(28.))
                .children(groups)
                .into_any_element()
        };
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .id("artifacts-gallery")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.gallery_scroll)
                    .child(
                        div()
                            .mx_auto()
                            .max_w(px(MAX_WIDTH))
                            .px(px(32.))
                            .py(px(28.))
                            .flex()
                            .flex_col()
                            .gap(px(24.))
                            .child(self.top(t, window, cx))
                            .child(self.new_tiles(t, cx))
                            .when(!self.list.is_empty(), |d| {
                                d.child(self.filters(t, cx)).children(self.places(t, cx))
                            })
                            .child(body),
                    ),
            )
            .children(scrollbar(&self.gallery_scroll, 10., 2., t.border_strong))
            .into_any_element()
    }
}

/// The mark of a pinned artifact.
fn pin_marker(t: &Theme) -> Div {
    div()
        .flex_none()
        .p(px(3.))
        .rounded(px(radius_sm()))
        .bg(t.bg_panel)
        .text_color(t.text_muted)
        .child(icon("push-pin", 12.))
}
