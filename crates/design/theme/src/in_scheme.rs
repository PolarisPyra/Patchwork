//! An element that draws its child in another scheme's theme, so a chat over its background, as
//! the Orchestrator page over its backdrop, takes that page's ink and glass: light over a night
//! sky and dark over a pale one, whatever the window's theme.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, App, Bounds, ElementId, GlobalElementId, InspectorElementId, LayoutId, Pixels,
    Window, prelude::*,
};
use patchwork_themes::Scheme;

use crate::InScheme;

/// Its child, laid out, prepainted and painted in `scheme`'s theme. The views it holds, such as
/// the composer, draw while it does, so they take that theme too.
pub struct InSchemeElement {
    scheme: Scheme,
    child: AnyElement,
}

/// `child`, drawn in `scheme`'s theme.
pub fn in_scheme(scheme: Scheme, child: impl IntoElement) -> InSchemeElement {
    InSchemeElement {
        scheme,
        child: child.into_any_element(),
    }
}

impl std::fmt::Debug for InSchemeElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InSchemeElement")
            .field("scheme", &self.scheme)
            .finish_non_exhaustive()
    }
}

impl IntoElement for InSchemeElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for InSchemeElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let _scope = InScheme::enter(self.scheme);
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let _scope = InScheme::enter(self.scheme);
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        (): &mut Self::RequestLayoutState,
        (): &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let _scope = InScheme::enter(self.scheme);
        self.child.paint(window, cx);
    }
}
