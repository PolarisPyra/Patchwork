//! The pages that fill the window in place of the chat: the orchestrator over its backdrop, and
//! the Quilts page over its own.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Div, Entity, Stateful, StyleRefinement, Window, div, prelude::*};
use patchwork_desktop_quilts::QuiltsPage;

use patchwork_desktop_chat::backdrop::quilts_scheme;
use patchwork_design_theme::in_scheme;
use crate::nav::Page;
use crate::window::Workspace;

impl Workspace {
    /// The orchestrator page. The backdrop is its own view under the page, so as it moves the
    /// page's frame is reused.
    pub(super) fn home_panel(&self) -> Stateful<Div> {
        div()
            .id("panels.home")
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                self.orchestrator_backdrop
                    .clone()
                    .cached(StyleRefinement::default().absolute().inset_0()),
            )
            .child(
                self.orchestrator
                    .clone()
                    .cached(StyleRefinement::default().flex_1().min_w_0().min_h_0()),
            )
    }

    /// The Quilts page while it shows. The page and its browser are made when it shows and
    /// closed when it is left.
    pub(super) fn quilts_shown(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<QuiltsPage>> {
        if self.page == Page::Quilts {
            Some(self.quilts_page(window, cx))
        } else {
            if self.quilts.take().is_some() {
                self.cool_browser_unless_shown(cx);
            }
            self.quilt_chat = false;
            None
        }
    }

    /// The Quilts page over its background and in that background's scheme, as a chat is, with
    /// the chat a quilt came from in a card over it. Cached, so a backdrop that moves redraws
    /// only itself, not the gallery over it.
    pub(super) fn quilts_panel(
        &self,
        page: Entity<QuiltsPage>,
        quilt_chat: Option<AnyElement>,
        cx: &gpui::App,
    ) -> Stateful<Div> {
        let page = div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(page.cached(StyleRefinement::default().flex_1().min_w_0().min_h_0()));
        let page = match quilts_scheme(self, cx) {
            Some(scheme) => in_scheme(scheme, page).into_any_element(),
            None => page.into_any_element(),
        };
        div()
            .id("panels.quilts")
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                self.quilts_backdrop_region
                    .clone()
                    .cached(StyleRefinement::default().absolute().inset_0()),
            )
            .child(page)
            .children(quilt_chat)
    }
}
