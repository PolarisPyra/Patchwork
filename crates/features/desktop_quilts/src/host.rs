//! What the Quilts page asks of the window: the view a quilt's page is drawn in.

// Rust guideline compliant 2026-11-27

use std::rc::Rc;

use gpui::{AnyView, App, WeakEntity};

use crate::QuiltsPage;

/// What the window gives the Quilts page.
pub trait QuiltsHost {
    /// Makes the view that draws the page at `url`, titled `title`, for `page`. The view tells
    /// `page` when its errors or picking change ([`QuiltsPage::preview_changed`]) and when an
    /// element is picked on it ([`QuiltsPage::element_picked`]).
    fn preview(
        &self,
        url: String,
        title: String,
        page: WeakEntity<QuiltsPage>,
        cx: &mut App,
    ) -> Rc<dyn QuiltPreview>;
}

/// The view a quilt's page is drawn in.
pub trait QuiltPreview {
    /// The view, to draw.
    fn view(&self) -> AnyView;
    /// Opens `url` in it, titled `title`.
    fn open(&self, url: &str, title: String, cx: &mut App);
    /// The errors the page met.
    fn errors(&self, cx: &App) -> Vec<String>;
    /// Whether the next click on it picks an element.
    fn picking(&self, cx: &App) -> bool;
    /// Starts or stops picking an element on it.
    fn set_picking(&self, on: bool, cx: &mut App);
}

/// An element picked on the page.
#[derive(Debug, Clone)]
pub struct PickedElement {
    /// A selector that finds it.
    pub selector: String,
    /// Its HTML, cut short when long.
    pub html: String,
    /// The styles it is drawn with.
    pub styles: String,
}
