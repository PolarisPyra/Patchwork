//! The rows of Settings → Themes that set how the picture behind the Orchestrator page is
//! shown: how much it is dimmed, how it fills the page, and whether it is blurred.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, prelude::*};
use patchwork_server::themes::{BackdropChoice, Dim, Fit};

use crate::settings::SettingsDialog;
use crate::settings::controls::{segmented, switch, token_row};
use crate::ui::theme::theme;

const DIMS: [(Dim, &str); 4] = [
    (Dim::None, "None"),
    (Dim::Light, "Light"),
    (Dim::Medium, "Medium"),
    (Dim::Strong, "Strong"),
];

const FITS: [(Fit, &str); 3] = [
    (Fit::Cover, "Fill"),
    (Fit::Contain, "Fit"),
    (Fit::Stretch, "Stretch"),
];

impl SettingsDialog {
    /// Changes how the picture behind the Orchestrator page is shown.
    fn set_picture(
        &mut self,
        set: impl FnOnce(&mut Dim, &mut Fit, &mut bool),
        cx: &mut Context<Self>,
    ) {
        self.update(
            |s| {
                if let BackdropChoice::Image { dim, fit, blur, .. } = &mut s.orchestrator_backdrop {
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
    blur: bool,
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
    let blurred = switch("picture-blur", "Blur the picture", blur, &t)
        .on_click(cx.listener(move |d, _, _, cx| d.set_picture(|_, _, was| *was = !blur, cx)));
    vec![
        token_row("Picture dimming", None, &t)
            .child(dims)
            .into_any_element(),
        token_row("Picture size", None, &t)
            .child(fits)
            .into_any_element(),
        token_row("Blur the picture", None, &t)
            .child(blurred)
            .into_any_element(),
    ]
}
