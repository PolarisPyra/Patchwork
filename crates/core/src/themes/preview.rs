//! A picture of the app drawn in a color scheme, for where the app itself cannot be seen.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;

use super::types::{Color, ThemeFile};
use crate::Error;

/// The picture's size, in pixels.
const WIDTH: u32 = 720;
const HEIGHT: u32 = 450;

/// `over` laid on `under` at `share` of full strength.
fn mix(over: Color, under: Color, share: f32) -> Color {
    let channel = |shift: u32| {
        let part = |c: Color| f32::from(u8::try_from((c.0 >> shift) & 0xff).unwrap_or(u8::MAX));
        let mixed = part(over) * share + part(under) * (1. - share);
        // The mix of two channels is within a channel's range.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to a color channel's range"
        )]
        let value = mixed.round().clamp(0., 255.) as u32;
        value << shift
    };
    Color(channel(16) | channel(8) | channel(0))
}

/// A rectangle of the picture, with rounded corners.
fn rect(page: &mut String, at: [f32; 4], radius: f32, fill: Color) {
    let [x, y, width, height] = at;
    // Writing to a string cannot fail.
    let _ = write!(
        page,
        "<rect x=\"{x}\" y=\"{y}\" width=\"{width}\" height=\"{height}\" rx=\"{radius}\" \
         fill=\"#{:06x}\"/>",
        fill.0
    );
}

impl ThemeFile {
    /// A picture of the app's window in this color scheme, as a PNG: the chat list, a chat with
    /// a message, a reply and code, the composer, and the scheme's colors along the bottom.
    /// Text is drawn as bars, in the colors text has.
    ///
    /// # Errors
    ///
    /// Returns an error when the picture cannot be drawn.
    pub(crate) fn preview(&self) -> Result<Vec<u8>, Error> {
        let colors = self.colors;
        let own = self.overrides;
        let (surface, text, muted, accent) =
            (colors.surface, colors.text, colors.muted, colors.accent);
        let over = |share: f32| mix(text, surface, share);
        let panel = own.bg_panel.unwrap_or_else(|| over(0.04));
        let editor = own.bg_editor.unwrap_or(surface);
        let input = own.bg_input.unwrap_or(surface);
        let border = own.border.unwrap_or_else(|| over(0.12));
        let selected = own.bg_selected.unwrap_or_else(|| mix(accent, panel, 0.22));
        let bubble = own.bubble.unwrap_or_else(|| mix(text, editor, 0.10));
        let on_accent = own.text_on_accent.unwrap_or(Color(0x00ff_ffff));
        let success = own.success.unwrap_or(Color(0x003f_b950));
        let danger = own.danger.unwrap_or(Color(0x00f8_5149));
        let warning = own.warning.unwrap_or(Color(0x00d2_9922));
        let radius = self.radius.unwrap_or(6.).clamp(0., 14.);

        let mut page = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{HEIGHT}\">"
        );
        rect(&mut page, [0., 0., 720., 450.], 0., editor);
        // The chat list, with the open chat selected.
        rect(&mut page, [0., 0., 200., 450.], 0., panel);
        rect(&mut page, [199., 0., 1., 450.], 0., border);
        rect(&mut page, [16., 18., 96., 10.], 5., muted);
        for (row, width) in [(0_u8, 132.), (1, 108.), (2, 148.), (3, 92.), (4, 124.)] {
            let y = 46. + f32::from(row) * 34.;
            if row == 1 {
                rect(&mut page, [8., y - 9., 184., 28.], radius, selected);
            }
            rect(
                &mut page,
                [16., y, 10., 10.],
                5.,
                if row == 0 { success } else { muted },
            );
            rect(&mut page, [34., y, width, 10.], 5., text);
        }
        // The chat's header.
        rect(&mut page, [200., 0., 520., 44.], 0., panel);
        rect(&mut page, [200., 43., 520., 1.], 0., border);
        rect(&mut page, [220., 17., 150., 10.], 5., text);
        rect(&mut page, [640., 12., 60., 20.], radius, accent);
        rect(&mut page, [652., 19., 36., 6.], 3., on_accent);
        // The person's message.
        rect(&mut page, [440., 64., 260., 48.], radius + 4., bubble);
        rect(&mut page, [456., 80., 228., 8.], 4., text);
        rect(&mut page, [456., 96., 150., 8.], 4., text);
        // The reply, with a link in it.
        rect(&mut page, [220., 132., 400., 8.], 4., text);
        rect(&mut page, [220., 150., 250., 8.], 4., text);
        rect(
            &mut page,
            [478., 150., 110., 8.],
            4.,
            own.accent_text.unwrap_or(accent),
        );
        rect(&mut page, [220., 168., 320., 8.], 4., muted);
        // Code.
        rect(&mut page, [220., 192., 480., 104.], radius, panel);
        for (row, parts) in [
            (0_u8, [(accent, 60.), (text, 120.), (muted, 0.)]),
            (1, [(muted, 30.), (success, 140.), (text, 70.)]),
            (2, [(muted, 30.), (warning, 90.), (text, 150.)]),
            (3, [(accent, 44.), (danger, 80.), (muted, 0.)]),
        ] {
            let mut x = 236.;
            for (color, width) in parts {
                if width > 0. {
                    rect(
                        &mut page,
                        [x, 210. + f32::from(row) * 20., width, 8.],
                        4.,
                        color,
                    );
                    x += width + 10.;
                }
            }
        }
        // The composer.
        rect(&mut page, [219., 319., 482., 62.], radius + 3., border);
        rect(&mut page, [220., 320., 480., 60.], radius + 2., input);
        rect(&mut page, [236., 336., 180., 8.], 4., over(0.35));
        rect(&mut page, [656., 346., 30., 24.], radius, accent);
        // The scheme's colors.
        for (place, color) in [surface, text, muted, accent, success, warning, danger]
            .into_iter()
            .enumerate()
        {
            let x = 220. + f32::from(u8::try_from(place).unwrap_or(0)) * 44.;
            rect(&mut page, [x - 1., 401., 36., 30.], radius, border);
            rect(&mut page, [x, 402., 34., 28.], radius, color);
        }
        page.push_str("</svg>");

        let tree = resvg::usvg::Tree::from_str(&page, &resvg::usvg::Options::default())
            .map_err(|e| format!("Patchwork could not draw the preview: {e}"))?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(WIDTH, HEIGHT)
            .ok_or("Patchwork could not draw the preview")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
        );
        pixmap
            .encode_png()
            .map_err(|e| format!("Patchwork could not draw the preview: {e}").into())
    }
}
