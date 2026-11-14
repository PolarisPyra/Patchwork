//! The rows of Settings → Backgrounds that set how the picture behind the Orchestrator page is
//! shown: how much it is dimmed, how it fills the page, and how much it is blurred.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, prelude::*};
use patchwork_server::themes::{BackdropChoice, Dim, Fit, MAX_BLUR};

use super::slider::{Range, slider};
use crate::settings::SettingsDialog;
use crate::settings::controls::{segmented, token_row};
use crate::ui::theme::theme;

const DIMS: [(Dim, &str); 4] = [
    (Dim::None, "None"),
    (Dim::Light, "Light"),
    (Dim::Medium, "Medium"),
    (Dim::Strong, "Strong"),
];

const FITS: [(Fit, &str); 4] = [
    (Fit::Cover, "Fill"),
    (Fit::Contain, "Fit"),
    (Fit::Stretch, "Stretch"),
    (Fit::Tile, "Tile"),
];

impl SettingsDialog {
    /// Changes how the picture behind the Orchestrator page is shown.
    fn set_picture(
        &mut self,
        set: impl FnOnce(&mut Dim, &mut Fit, &mut u8),
        cx: &mut Context<Self>,
    ) {
        let slot = self.backdrop_slot(cx);
        self.update(
            |s| {
                if let BackdropChoice::Image { dim, fit, blur, .. } = s.backdrop_mut(slot) {
                    set(dim, fit, blur);
                }
            },
            cx,
        );
    }
}

/// How the picked picture is shown behind the Orchestrator page: dimmed, laid out, blurred.
pub(super) fn picture_rows(
    dim: Dim,
    fit: Fit,
    blur: u8,
    cx: &mut Context<SettingsDialog>,
) -> Vec<AnyElement> {
    let t = *theme(cx);
    let dims = segmented(
        "picture-dim",
        "Picture dimming",
        dim,
        &DIMS,
        &t,
        |d, dim, cx| d.set_picture(|was, _, _| *was = dim, cx),
        cx,
    );
    let fits = segmented(
        "picture-fit",
        "Picture size",
        fit,
        &FITS,
        &t,
        |d, fit, cx| d.set_picture(|_, was, _| *was = fit, cx),
        cx,
    );
    let range = Range {
        min: 0.,
        max: f32::from(MAX_BLUR),
        step: 1.,
    };
    let blurred = slider(
        "picture-blur",
        "Picture blur",
        f32::from(blur),
        range,
        &t,
        move |d, level, cx| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the slider keeps the level between zero and the most blur"
            )]
            let level = level.round() as u8;
            if level != blur {
                d.set_picture(|_, _, was| *was = level, cx);
            }
        },
        cx,
    );
    vec![
        token_row("Picture dimming", None, &t)
            .child(dims)
            .into_any_element(),
        token_row("Picture size", None, &t)
            .child(fits)
            .into_any_element(),
        token_row("Picture blur", None, &t)
            .child(blurred)
            .into_any_element(),
    ]
}
