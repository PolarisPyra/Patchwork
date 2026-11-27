//! The gallery: every quilt as a card with its picture or as a row, filtered by kind and by
//! where it was made, searched, and grouped with the pinned first, then by project, with tiles
//! that start a chat making a new one.

// Rust guideline compliant 2026-10-09

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Bounds, Div, Pixels, canvas, point, FontWeight, KeyDownEvent, MouseButton,
    MouseDownEvent, ObjectFit, Role, SharedString, Stateful, Window, div, img, prelude::*, px,
};
use patchwork_backend::quilts::{Quilt, QuiltKind};

use super::patch::{Act, patch};
use super::layout::kind_icon;
use super::catalog::{QuiltsPage, Place};
use patchwork_ui_theme::{Theme, fs_lg, fs_md, fs_sm, fs_xxl, radius, radius_sm};
use patchwork_ui_components::{button, focus_ring, icon_button, input_box, scrollbar, spinner, tooltip};
use patchwork_ui_markdown::format_relative;
use patchwork_ui_theme::icon;

/// A card's width; its picture is 16 by 10, as the pictures are taken.
const CARD_WIDTH: f32 = 320.;

/// The group a card's picture and its border hover as.
const PICTURE: &str = "quilt-picture";

/// How long a kind's quilts stay lit after its tile is picked.
const FLASH: Duration = Duration::from_secs(1);

/// The room left above the first of a kind's quilts when the gallery scrolls to it.
const SCROLL_MARGIN: f32 = 24.;

impl QuiltsPage {
    /// The title, the search, and the switch between cards and rows.
    fn top(&self, sad: bool, t: &Theme, window: &Window, cx: &mut Context<Self>) -> Div {
        let focused = gpui::Focusable::focus_handle(self.search.read(cx), cx).is_focused(window);
        let grid = self.grid;
        div()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(
                button("quilts-patch", "Patch, wave hello")
                    .flex_none()
                    .rounded(px(radius()))
                    .child(patch(self.patch, sad, 104.))
                    .on_click(cx.listener(|this, _, _, cx| this.play(Act::Wave, cx))),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(fs_xxl() * 1.3))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Quilts"),
            )
            .child(
                input_box(focused, t)
                    .w(px(320.))
                    .gap(px(8.))
                    .child(icon("magnifying-glass", 16.))
                    .child(div().flex_1().min_w_0().child(self.search.clone())),
            )
            .child(
                icon_button(
                    "quilts-layout",
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
                .id(SharedString::from(format!("quilts-{name}-{i}")))
                .role(Role::Tab)
                .aria_label(label.clone())
                .aria_selected(on)
                .tab_index(0)
                .focus_visible(focus_ring)
                .cursor_pointer()
                .h(px(32.))
                .px(px(14.))
                .flex()
                .items_center()
                .rounded_full()
                .text_size(px(fs_md()))
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
            .id(SharedString::from(format!("quilts-{name}")))
            .role(Role::TabList)
            .aria_label(name)
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .children(tabs)
    }

    /// All, then each kind there is a quilt of.
    fn filters(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let kinds: Vec<(String, Option<QuiltKind>)> = std::iter::once(("All".to_owned(), None))
            .chain(
                QuiltKind::ALL
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

    /// Everywhere, the chat open when the page opened, then each project there is a quilt
    /// from; None when there is only everywhere.
    fn places(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let mut places = vec![("Everywhere".to_owned(), Place::All)];
        if self
            .came_from
            .as_ref()
            .is_some_and(|c| self.list.iter().any(|a| &a.chat_id == c))
        {
            places.push(("This chat".to_owned(), Place::Chat));
        }
        let mut projects: Vec<(String, Place)> = Vec::new();
        for a in &self.list {
            if let Some((id, name)) = self.projects.get(&a.chat_id)
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

    /// The tiles for docs, decks, sheets, designs, dashboards and boards, each going to its
    /// quilts (`QuiltKind::tile`); one with none is dimmed and goes nowhere.
    fn kind_tiles(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let (hover, border) = (t.bg_hover, t.border);
        let tiles = QuiltKind::MADE_NEW.into_iter().map(|kind| {
            let count = self
                .list
                .iter()
                .filter(|a| a.kind.tile() == Some(kind))
                .count();
            let label = match count {
                0 => format!("{}, none yet", kind.label()),
                1 => format!("{}, 1 quilt", kind.label()),
                n => format!("{}, {n} quilts", kind.label()),
            };
            button(
                SharedString::from(format!("quilts-kind-{}", kind.name())),
                label,
            )
            .flex_col()
            .justify_center()
            .gap(px(12.))
            .flex_1()
            .min_w(px(150.))
            .h(px(120.))
            .rounded(px(radius()))
            .border_1()
            .border_color(border)
            .bg(t.bg_panel)
            .map(|b| {
                if count == 0 {
                    b.cursor_default().opacity(0.5)
                } else {
                    b.hover(move |s| s.bg(hover))
                }
            })
            .child(
                div()
                    .text_color(t.text_muted)
                    .child(icon(kind_icon(kind), 32.)),
            )
            .child(div().text_size(px(fs_lg())).child(kind.label()))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if *hovered {
                    this.play(Act::Sew, cx);
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.go_to_kind(kind, cx)))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(px(fs_lg()))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Jump to"),
            )
            .child(
                div()
                    .id("quilts-kinds")
                    .role(Role::Group)
                    .aria_label("Jump to a kind")
                    .flex()
                    .flex_wrap()
                    .gap(px(12.))
                    .children(tiles),
            )
    }

    /// A quilt's picture for the theme's darkness, a spinner while it is taken, or its kind's
    /// icon. A picture not taken yet for the theme is asked for, as when the theme changed.
    fn picture(&self, quilt: &Quilt, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let frame = div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(t.text_subtle);
        let (id, n, dark) = (&quilt.id, quilt.versions, t.is_dark());
        let taken = self
            .thumbs
            .read(cx)
            .get(id, n, dark)
            .map(Option::<&std::path::PathBuf>::cloned);
        if taken.is_none() && quilt.kind.drawn() {
            self.thumbs
                .update(cx, |thumbs, cx| thumbs.want(id, n, dark, cx));
        }
        match taken {
            // The picture keeps the frame's corners, which clipping alone does not.
            Some(Some(path)) => img(path)
                .size_full()
                .rounded(px(radius()))
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
            None if quilt.kind.drawn() => {
                frame.child(spinner(16., t.text_subtle)).into_any_element()
            }
            _ => frame
                .child(icon(kind_icon(quilt.kind), 28.))
                .into_any_element(),
        }
    }

    /// A quilt as a card: its picture, title, kind and when it changed.
    fn card(&self, quilt: &Quilt, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let (hover, border) = (t.border_strong, t.border);
        let (lit, accent) = (self.lit(quilt), t.accent);
        let id = quilt.id.clone();
        button(
            SharedString::from(format!("quilt-{}", quilt.id)),
            quilt.title.clone(),
        )
        .role(Role::ListItem)
        .relative()
        .child(spot(&self.spots, &quilt.id))
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
                .child(self.picture(quilt, t, cx))
                // The border is drawn over the picture, so the picture's edge never shows past
                // its curve at the corners.
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .rounded(px(radius()))
                        .map(|d| {
                            if lit {
                                d.border_2().border_color(accent)
                            } else {
                                d.border_1()
                                    .border_color(border)
                                    .group_hover(PICTURE, move |s| s.border_color(hover))
                            }
                        }),
                )
                .when(quilt.pinned, |d| {
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
                        .text_size(px(fs_lg()))
                        .child(quilt.title.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(fs_md()))
                        .text_color(t.text_muted)
                        .child(format!(
                            "{} · {}",
                            quilt.kind.label(),
                            format_relative(quilt.updated_at)
                        )),
                ),
        )
        .map(|b| self.with_menu(b, quilt, cx))
        .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)))
    }

    /// `item` with the quilt's menu on a right click, and its state said.
    fn with_menu(
        &self,
        item: Stateful<Div>,
        quilt: &Quilt,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = quilt.id.clone();
        item.when(quilt.pinned, |b| {
            b.aria_label(format!("{}, pinned", quilt.title))
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                this.open_menu(&id, e.position, window, cx);
                cx.stop_propagation();
            }),
        )
    }

    /// A quilt as a row: its icon, title, kind, chat and when it changed.
    fn row(&self, quilt: &Quilt, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let hover = t.bg_hover;
        let lit = self.lit(quilt);
        let id = quilt.id.clone();
        let chat = self.chats.get(&quilt.chat_id).cloned().unwrap_or_default();
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
            SharedString::from(format!("quilt-{}", quilt.id)),
            quilt.title.clone(),
        )
        .role(Role::ListItem)
        .relative()
        .child(spot(&self.spots, &quilt.id))
        .gap(px(12.))
        .px(px(10.))
        .h(px(40.))
        .rounded(px(radius_sm()))
        .border_b_1()
        .border_color(t.border)
        .hover(move |s| s.bg(hover))
        .when(lit, |d| d.bg(t.bg_selected_muted))
        .child(
            div()
                .flex_none()
                .text_color(t.text_muted)
                .child(icon(kind_icon(quilt.kind), 16.)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(fs_md()))
                .child(quilt.title.clone()),
        )
        .when(quilt.pinned, |d| d.child(pin_marker(t)))
        .child(cell(quilt.kind.label().to_owned(), 90.))
        .child(cell(chat, 200.))
        .child(cell(format_relative(quilt.updated_at), 80.))
        .map(|b| self.with_menu(b, quilt, cx))
        .on_click(cx.listener(move |this, _, _, cx| this.select(id.clone(), cx)))
    }

    /// Whether `quilt` is counted under the tile just picked.
    fn lit(&self, quilt: &Quilt) -> bool {
        self.flash
            .is_some_and(|(kind, _)| quilt.kind.tile() == Some(kind))
    }

    /// Scrolls to the first quilt shown under the tile `kind` and lights all of them for a
    /// moment; does nothing when none is shown.
    fn go_to_kind(&mut self, kind: QuiltKind, cx: &mut Context<Self>) {
        let first = self
            .sections(cx)
            .into_iter()
            .flat_map(|(_, list)| list)
            .find(|a| a.kind.tile() == Some(kind));
        let Some(first) = first else {
            return;
        };
        if let Some(spot) = self.spots.borrow().get(&first.id) {
            let scroll = &self.gallery_scroll;
            let (view, offset) = (scroll.bounds(), scroll.offset());
            let lowest = -scroll.max_offset().y;
            let y =
                (offset.y - (spot.top() - view.top()) + px(SCROLL_MARGIN)).clamp(lowest, px(0.));
            scroll.set_offset(point(offset.x, y));
        }
        let since = Instant::now();
        self.flash = Some((kind, since));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FLASH).await;
            let _ = this.update(cx, |this, cx| {
                if this.flash.is_some_and(|(_, at)| at == since) {
                    this.flash = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// The quilts shown in groups, each with its heading: the pinned first, then each
    /// project's, then those from chats in no project.
    fn sections(&self, cx: &App) -> Vec<(String, Vec<Quilt>)> {
        let mut sections: Vec<(String, Vec<Quilt>)> = Vec::new();
        let mut put =
            |heading: &str, a: &Quilt| match sections.iter_mut().find(|(h, _)| h == heading) {
                Some((_, list)) => list.push(a.clone()),
                None => sections.push((heading.to_owned(), vec![a.clone()])),
            };
        let shown = self.matching(cx);
        for a in shown.iter().filter(|a| a.pinned) {
            put("Pinned", a);
        }
        for a in shown.iter().filter(|a| !a.pinned) {
            if let Some((_, name)) = self.projects.get(&a.chat_id) {
                put(name, a);
            }
        }
        for a in shown
            .iter()
            .filter(|a| !a.pinned && !self.projects.contains_key(&a.chat_id))
        {
            put("Other chats", a);
        }
        sections
    }

    /// The gallery: its top, the tiles, the filters, and the quilts.
    pub(super) fn gallery(&self, t: &Theme, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let sections = self.sections(cx);
        let headed = sections.len() > 1;
        let sad = sections.is_empty() && !self.list.is_empty();
        let body = if sections.is_empty() {
            div()
                .py(px(48.))
                .flex()
                .justify_center()
                .text_size(px(fs_lg()))
                .text_color(t.text_muted)
                .child(if self.list.is_empty() {
                    "No quilts yet"
                } else {
                    "No quilts match"
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
                                    .id(("quilts-heading", i))
                                    .role(Role::Heading)
                                    .text_size(px(fs_lg()))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(heading.clone()),
                            )
                        })
                        .child(
                            div()
                                .id(("quilts-items", i))
                                .role(Role::List)
                                .aria_label(if headed { heading } else { "Quilts".to_owned() })
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
                    .id("quilts-gallery")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.gallery_scroll)
                    .child(
                        div()
                            .px(px(32.))
                            .py(px(28.))
                            .flex()
                            .flex_col()
                            .gap(px(24.))
                            .child(self.top(sad, t, window, cx))
                            .child(self.kind_tiles(t, cx))
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

/// Notes where the card or row it is put in was drawn, under `id` in `spots`.
fn spot(spots: &Rc<RefCell<HashMap<String, Bounds<Pixels>>>>, id: &str) -> impl IntoElement {
    let (spots, id) = (Rc::clone(spots), id.to_owned());
    canvas(
        move |bounds, _, _| {
            spots.borrow_mut().insert(id, bounds);
        },
        |_, (), _, _| {},
    )
    .absolute()
    .inset_0()
}

/// The mark of a pinned quilt.
fn pin_marker(t: &Theme) -> Div {
    div()
        .flex_none()
        .p(px(3.))
        .rounded(px(radius_sm()))
        .bg(t.bg_panel)
        .text_color(t.text_muted)
        .child(icon("push-pin", 12.))
}
