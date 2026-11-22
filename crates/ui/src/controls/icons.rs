//! Icons: the Phosphor set by name, and a device kind's icon.

// Rust guideline compliant 2026-10-09

use gpui::{App, IntoElement, SharedString, StyleRefinement, Window, canvas, prelude::*, px};

/// A Phosphor icon by its kebab-case name, as `@phosphor-icons/react` draws it at `size`.
#[must_use]
pub fn icon(name: &str, size: f32) -> Icon {
    mono_svg(SharedString::from(format!("icons/{name}.svg")), size)
}

/// The icon for a kind of machine: a desktop, a laptop, or a drive for anything else.
#[must_use]
pub fn kind_icon(kind: patchwork_core::system::machine::device::Kind) -> &'static str {
    use patchwork_core::system::machine::device::Kind;
    match kind {
        Kind::Desktop => "desktop",
        Kind::Laptop => "laptop",
        Kind::Other => "hard-drives",
    }
}

/// A one-colour SVG asset, `size` square.
pub(crate) fn mono_svg(path: SharedString, size: f32) -> Icon {
    Icon {
        path,
        style: StyleRefinement::default(),
    }
    .size(px(size))
    .flex_none()
}

/// A one-colour SVG drawn in the text colour around it, so it
/// follows its button's hover colour. A colour set on the icon itself wins.
#[derive(Debug, IntoElement)]
pub struct Icon {
    pub(crate) path: SharedString,
    pub(crate) style: StyleRefinement,
}

impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Icon {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let own = self.style.text.color;
        let path = self.path;
        // The colour around it is known while laying out; painting no longer has it.
        let mut drawn = canvas(
            move |_, window, _| own.unwrap_or_else(|| window.text_style().color),
            move |bounds, color, window: &mut Window, cx| {
                if let Err(e) = window.paint_svg(
                    bounds,
                    path.clone(),
                    None,
                    gpui::TransformationMatrix::default(),
                    color,
                    cx,
                ) {
                    log::warn!(path:%, error:% = e; "could not draw {{path}}: {{error}}");
                }
            },
        );
        *drawn.style() = self.style;
        drawn
    }
}
