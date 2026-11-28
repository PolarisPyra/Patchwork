//! A settings section: its heading and action over a card of rows, and the key caps the
//! shortcut rows show. Every page is built from these, so every page reads as one system.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, App, Div, FontWeight, SharedString, Window, div, prelude::*, px};
use patchwork_design_theme::{Theme, font_mono, fs_md, fs_xs, radius_lg, radius_sm, theme};
use super::found::anchored;

/// How far apart sections stand, so each reads as its own block.
const SECTION_GAP: f32 = 24.;

/// A run of settings under one heading, in a card with a line between each row.
///
/// A bare section drops the card, for content that draws its own boxes, such as a grid of
/// palettes or a chart.
#[derive(IntoElement)]
pub struct Section {
    title: Option<SharedString>,
    action: Option<AnyElement>,
    bare: bool,
    rows: Vec<AnyElement>,
}

impl Section {
    /// A section headed `title`.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::untitled()
        }
    }

    /// A section with no heading, for a page's first card.
    pub fn untitled() -> Self {
        Self {
            title: None,
            action: None,
            bare: false,
            rows: Vec::new(),
        }
    }

    /// A control at the right of the heading, such as an Add button or a filter.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }

    /// Draws the rows as they are, without the card.
    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }

    pub fn child(mut self, row: impl IntoElement) -> Self {
        self.rows.push(row.into_any_element());
        self
    }

    pub fn children(mut self, rows: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.rows
            .extend(rows.into_iter().map(IntoElement::into_any_element));
        self
    }
}

impl RenderOnce for Section {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = *theme(cx);
        let headed = self.title.is_some() || self.action.is_some();
        let heading = headed.then(|| {
            let name = self.title.clone().unwrap_or_default();
            anchored(
                div().id(SharedString::from(format!("heading-{name}"))),
                &name,
            )
            .flex()
            .items_end()
            .gap(px(12.))
            .min_h(px(28.))
            .mb(px(8.))
            .child(div().min_w_0().flex_1().children(self.title.map(|title| {
                div()
                    .text_size(px(fs_md()))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(t.text)
                    .child(title)
            })))
            .children(
                self.action
                    .map(|a| div().flex().flex_none().items_center().gap(px(8.)).child(a)),
            )
        });
        let body = if self.bare {
            div().flex().flex_col().children(self.rows)
        } else {
            card(self.rows, &t)
        };
        div()
            .flex()
            .flex_col()
            .mb(px(SECTION_GAP))
            .children(heading)
            .child(body)
    }
}

/// Rows in one rounded, faintly tinted box, a line between each.
pub fn card(rows: Vec<AnyElement>, t: &Theme) -> Div {
    let border = t.border;
    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(radius_lg()))
        .border_1()
        .border_color(border)
        .bg(tint(t))
        .children(rows.into_iter().enumerate().map(move |(i, row)| {
            div()
                .when(i > 0, |d| d.border_t_1().border_color(border))
                .child(row)
        }))
}

/// The card's fill: the window's back color showing faintly through the dialog's, so a card
/// stands off the page without a shadow.
pub fn tint(t: &Theme) -> gpui::Hsla {
    let mut back = t.bg_app;
    back.a *= 0.4;
    back
}

/// A key of a shortcut, drawn as a small cap.
pub fn key_cap(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(20.))
        .min_w(px(20.))
        .px(px(5.))
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_input)
        .font_family(font_mono())
        .text_size(px(fs_xs()))
        .text_color(t.text)
        .child(text.into())
}
