//! Icons: compile-time typed Phosphor icons backed by `patchwork-ui-tokens`.
//!
//! Every [`IconName`] variant is generated at build time from `assets/icons/*.svg` and packed
//! into a compressed binary pool. Icons with a `<name>.secondary.svg` companion automatically
//! paint their duotone secondary layer underneath the primary path.

// Rust guideline compliant 2026-10-09

use gpui::{App, Hsla, IntoElement, SharedString, StyleRefinement, Window, canvas, prelude::*, px};
pub use patchwork_ui_tokens::IconName;

/// A compile-time checked icon from `assets/icons/`, drawn at `size` square.
#[must_use]
pub fn icon(name: IconName, size: f32) -> Icon {
    let primary = crate::brand::asset(name.asset_path());
    let secondary = name.secondary_asset_path().map(crate::brand::asset);
    Icon {
        path: primary,
        secondary_path: secondary,
        secondary_color: None,
        style: StyleRefinement::default(),
    }
    .size(px(size))
    .flex_none()
}

/// Resolves an icon dynamically by kebab-case name, logging and falling back to
/// [`IconName::Warning`] if unknown. Prefer [`icon`] with [`IconName`] for all static icons.
#[must_use]
pub fn icon_by_name(name: &str, size: f32) -> Icon {
    let resolved = IconName::from_kebab(name).unwrap_or_else(|| {
        log::warn!(name; "unknown icon name {{name}}, falling back to warning icon");
        IconName::Warning
    });
    icon(resolved, size)
}

/// A one-colour SVG asset, `size` square.
pub(crate) fn mono_svg(path: SharedString, size: f32) -> Icon {
    Icon {
        path,
        secondary_path: None,
        secondary_color: None,
        style: StyleRefinement::default(),
    }
    .size(px(size))
    .flex_none()
}

/// A one-colour or duotone SVG drawn in the text colour around it, so it
/// follows its button's hover colour. A colour set on the icon itself wins.
#[derive(Debug, IntoElement)]
pub struct Icon {
    pub(crate) path: SharedString,
    pub(crate) secondary_path: Option<SharedString>,
    pub(crate) secondary_color: Option<Hsla>,
    pub(crate) style: StyleRefinement,
}

impl Icon {
    /// Overrides the colour used for this icon's `.secondary.svg` duotone layer, when present.
    #[must_use]
    pub fn secondary_color(mut self, color: impl Into<Hsla>) -> Self {
        self.secondary_color = Some(color.into());
        self
    }
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
        let secondary_path = self.secondary_path;
        let secondary_color = self.secondary_color;
        // The colour around it is known while laying out; painting no longer has it.
        let mut drawn = canvas(
            move |_, window, _| own.unwrap_or_else(|| window.text_style().color),
            move |bounds, color, window: &mut Window, cx| {
                if let Some(sec_path) = secondary_path.clone() {
                    let sec_color = secondary_color.unwrap_or_else(|| color.opacity(0.45));
                    if let Err(e) = window.paint_svg(
                        bounds,
                        sec_path.clone(),
                        None,
                        gpui::TransformationMatrix::default(),
                        sec_color,
                        cx,
                    ) {
                        log::warn!(path:% = sec_path, error:% = e; "could not draw {{path}}: {{error}}");
                    }
                }
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
