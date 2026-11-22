//! The colors: each palette as a `Theme`, the look in use, and applying a new one.

// Rust guideline compliant 2026-10-09

use std::cell::Cell;
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use gpui::{App, Global, Hsla, Rgba, SharedString, WindowAppearance, hsla};
use patchwork_core::themes::{Applied, Color, Paint, Palette, Scheme, Syntax, ThemeFile};
use super::sizes::{CORNER, DENSITY, FONTS, FONT_MONO, FONT_UI, MAX_RADIUS, RADIUS, TEXT_SCALE};

/// The colors one of `theme.css`'s three palettes gives each token.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg_app: Hsla,
    pub bg_panel: Hsla,
    pub bg_editor: Hsla,
    pub bg_input: Hsla,
    pub bg_hover: Hsla,
    pub bg_selected: Hsla,
    pub bg_selected_muted: Hsla,
    /// The task tree's hover; the light palette has none and uses `bg_hover`.
    pub tree_hover: Hsla,
    /// A picked task row under the pointer.
    pub tree_selected: Hsla,
    /// Popups and dialogs.
    pub bg_elevated: Hsla,
    /// `--shadow-popup`'s colour: `0 6px 24px` of it.
    pub shadow: Hsla,
    pub bg_accent_soft: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_subtle: Hsla,
    pub text_on_accent: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_text: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    pub danger_soft: Hsla,
    pub warning: Hsla,
    pub warning_soft: Hsla,
    pub syn_keyword: Hsla,
    pub syn_string: Hsla,
    pub syn_property: Hsla,
    pub syn_type: Hsla,
    pub syn_number: Hsla,
    pub syn_comment: Hsla,
    pub syn_function: Hsla,
    pub syn_tag: Hsla,
    pub diff_add_bg: Hsla,
    pub diff_del_bg: Hsla,
    /// The user's message bubble: `color-mix(in srgb, var(--text) 10%, var(--bg-editor))`.
    pub bubble: Hsla,
    /// Inline code and pills: `color-mix(in srgb, var(--text) 6% or 8%, transparent)`.
    pub text_tint_6: Hsla,
    pub text_tint_8: Hsla,
}

pub(super) fn hex(value: u32) -> Hsla {
    gpui::rgb(value).into()
}

pub(super) fn hexa(value: u32) -> Hsla {
    gpui::rgba(value).into()
}

/// The placeholder colour a web page's own inputs show (`darkGray`), whatever the theme. The
/// composer, commit message and model search set theirs to `text_subtle`.
#[must_use]
pub fn input_placeholder() -> Hsla {
    hex(0x00a9_a9a9)
}

/// `--border-focus`, the same in every theme.
#[must_use]
pub fn border_focus() -> Hsla {
    hex(0x0035_74f0)
}

/// `color-mix(in srgb, fg pct%, bg)`, for the tokens mixed from two others.
pub(super) fn mix(fg: u32, bg: u32, pct: f32) -> Hsla {
    let (f, b) = (gpui::rgb(fg), gpui::rgb(bg));
    Rgba {
        r: f.r * pct + b.r * (1. - pct),
        g: f.g * pct + b.g * (1. - pct),
        b: f.b * pct + b.b * (1. - pct),
        a: 1.,
    }
    .into()
}

/// `color-mix(in srgb, color pct%, transparent)`.
pub(super) fn tint(color: u32, pct: f32) -> Hsla {
    let mut c = hex(color);
    c.a = pct;
    c
}

impl Theme {
    #[must_use]
    pub fn dark() -> Self {
        Self {
            bg_app: hex(0x001e_1f22),
            bg_panel: hex(0x002b_2d30),
            bg_editor: hex(0x001e_1f22),
            bg_input: hex(0x001e_1f22),
            bg_hover: hex(0x0039_3b40),
            bg_selected: hex(0x002e_436e),
            bg_selected_muted: hex(0x0043_454a),
            tree_hover: hexa(0xffff_ff14),
            tree_selected: hexa(0x3574_f04d),
            bg_elevated: hex(0x002b_2d30),
            shadow: hexa(0x0000_0073),
            bg_accent_soft: hexa(0x3574_f029),
            border: hex(0x0039_3b40),
            border_strong: hex(0x004e_5157),
            text: hex(0x00df_e1e5),
            text_muted: hex(0x009f_a2a8),
            text_subtle: hex(0x006f_737a),
            text_on_accent: hex(0x00ff_ffff),
            accent: hex(0x0035_74f0),
            accent_hover: hex(0x0046_82fa),
            accent_text: hex(0x0054_8af7),
            success: hex(0x005f_b865),
            danger: hex(0x00db_5c5c),
            danger_soft: hexa(0xdb5c_5c24),
            warning: hex(0x00f2_c55c),
            warning_soft: hexa(0xf2c5_5c24),
            syn_keyword: hex(0x00cf_8e6d),
            syn_string: hex(0x006a_ab73),
            syn_property: hex(0x00c7_7dbb),
            syn_type: hex(0x0016_baac),
            syn_number: hex(0x002a_acb8),
            syn_comment: hex(0x007a_7e85),
            syn_function: hex(0x0056_a8f5),
            syn_tag: hex(0x00d5_b778),
            diff_add_bg: hexa(0x549b_5a2e),
            diff_del_bg: hexa(0xdb5c_5c2e),
            bubble: mix(0x00df_e1e5, 0x001e_1f22, 0.10),
            text_tint_6: tint(0x00df_e1e5, 0.06),
            text_tint_8: tint(0x00df_e1e5, 0.08),
        }
    }

    /// The dark palette with its surfaces pushed to true black.
    #[must_use]
    pub fn oled() -> Self {
        Self {
            bg_app: hex(0x0000_0000),
            bg_panel: hex(0x000a_0a0a),
            bg_editor: hex(0x0000_0000),
            bg_input: hex(0x0000_0000),
            bg_hover: hex(0x001a_1a1b),
            bg_selected_muted: hex(0x001a_1a1b),
            bg_elevated: hex(0x000e_0e0e),
            shadow: hexa(0x0000_00b3),
            border: hex(0x0022_2327),
            border_strong: hex(0x0034_363b),
            bubble: mix(0x00df_e1e5, 0x0000_0000, 0.10),
            ..Self::dark()
        }
    }

    #[must_use]
    pub fn light() -> Self {
        Self {
            bg_app: hex(0x00f7_f8fa),
            bg_panel: hex(0x00f7_f8fa),
            bg_editor: hex(0x00ff_ffff),
            bg_input: hex(0x00ff_ffff),
            bg_hover: hex(0x00df_e1e5),
            bg_selected: hex(0x00d4_e2ff),
            bg_selected_muted: hex(0x00eb_ecf0),
            tree_hover: hex(0x00df_e1e5),
            tree_selected: hex(0x00d4_e2ff),
            bg_elevated: hex(0x00ff_ffff),
            shadow: hexa(0x0000_001f),
            bg_accent_soft: hex(0x00ed_f3ff),
            border: hex(0x00eb_ecf0),
            border_strong: hex(0x00c9_ccd6),
            text: hex(0x0000_0000),
            text_muted: hex(0x006c_707e),
            text_subtle: hex(0x0081_8594),
            text_on_accent: hex(0x00ff_ffff),
            accent: hex(0x0035_74f0),
            accent_hover: hex(0x002d_63cc),
            accent_text: hex(0x0035_74f0),
            success: hex(0x0020_8a3c),
            danger: hex(0x00e5_5765),
            danger_soft: hex(0x00fd_eef0),
            warning: hex(0x00c2_7d04),
            warning_soft: hex(0x00fd_f5e1),
            syn_keyword: hex(0x0000_33b3),
            syn_string: hex(0x0006_7d17),
            syn_property: hex(0x0087_1094),
            syn_type: hex(0x0000_8080),
            syn_number: hex(0x0017_50eb),
            syn_comment: hex(0x008c_8c8c),
            syn_function: hex(0x0000_627a),
            syn_tag: hex(0x0000_33b3),
            diff_add_bg: hex(0x00e6_f5ea),
            diff_del_bg: hex(0x00fd_eef0),
            bubble: mix(0x0000_0000, 0x00ff_ffff, 0.10),
            text_tint_6: tint(0x0000_0000, 0.06),
            text_tint_8: tint(0x0000_0000, 0.08),
        }
    }

    /// The palette of a color scheme's file: mixed from its four colors, then the colors it
    /// names itself. What neither reaches (the diff and status colors) is the light or dark
    /// palette's.
    #[must_use]
    pub fn from_file(file: &ThemeFile) -> Self {
        let anchors = file.colors;
        let (surface, text) = (anchors.surface.0, anchors.text.0);
        let (muted, accent) = (anchors.muted.0, anchors.accent.0);
        let over = |pct: f32| mix(text, surface, pct);
        let lit = gpui::rgb(accent);
        let bright = 0.299 * lit.r + 0.587 * lit.g + 0.114 * lit.b > 0.6;
        let mut theme = Self {
            bg_app: hex(surface),
            bg_panel: over(0.04),
            bg_editor: hex(surface),
            bg_input: hex(surface),
            bg_hover: over(0.10),
            bg_selected: mix(accent, surface, 0.30),
            bg_selected_muted: over(0.14),
            tree_hover: tint(text, 0.08),
            tree_selected: tint(accent, 0.30),
            bg_elevated: over(0.05),
            bg_accent_soft: tint(accent, 0.16),
            border: over(0.12),
            border_strong: over(0.22),
            text: hex(text),
            text_muted: hex(muted),
            text_subtle: mix(muted, surface, 0.7),
            text_on_accent: hex(if bright { 0x0010_1114 } else { 0x00ff_ffff }),
            accent: hex(accent),
            accent_hover: mix(text, accent, 0.12),
            accent_text: mix(text, accent, 0.20),
            bubble: over(0.10),
            text_tint_6: tint(text, 0.06),
            text_tint_8: tint(text, 0.08),
            ..match file.scheme {
                Scheme::Light => Self::light(),
                Scheme::Dark => Self::dark(),
            }
        };
        let named = file.overrides;
        macro_rules! lay {
            ($($token:ident),*) => {
                $(if let Some(color) = named.$token {
                    theme.$token = hex(color.0);
                })*
            };
        }
        lay!(
            bg_panel,
            bg_editor,
            bg_input,
            bg_hover,
            bg_selected,
            bg_selected_muted,
            bg_elevated,
            border,
            border_strong,
            text_subtle,
            text_on_accent,
            accent_hover,
            accent_text,
            success,
            danger,
            warning,
            bubble
        );
        // The washes behind a status follow the status color the file names.
        if let Some(color) = named.success {
            theme.diff_add_bg = tint(color.0, 0.18);
        }
        if let Some(color) = named.danger {
            theme.danger_soft = tint(color.0, 0.14);
            theme.diff_del_bg = tint(color.0, 0.18);
        }
        if let Some(color) = named.warning {
            theme.warning_soft = tint(color.0, 0.14);
        }
        if let Some(syntax) = named.syntax {
            theme.highlight(syntax);
        }
        theme
    }

    /// Draws buttons, links and what is selected in `accent`, in place of the palette's own.
    fn accented(&mut self, Color(accent): Color) {
        let (surface, text) = (code(self.bg_app), code(self.text));
        let lit = gpui::rgb(accent);
        let bright = 0.299 * lit.r + 0.587 * lit.g + 0.114 * lit.b > 0.6;
        self.accent = hex(accent);
        self.accent_hover = mix(text, accent, 0.12);
        self.accent_text = mix(text, accent, 0.20);
        self.bg_selected = mix(accent, surface, 0.30);
        self.tree_selected = tint(accent, 0.30);
        self.bg_accent_soft = tint(accent, 0.16);
        self.text_on_accent = hex(if bright { 0x0010_1114 } else { 0x00ff_ffff });
    }

    /// Highlights code in a code theme's colors.
    fn highlight(&mut self, syntax: Syntax) {
        self.syn_keyword = hex(syntax.keyword.0);
        self.syn_string = hex(syntax.string.0);
        self.syn_property = hex(syntax.property.0);
        self.syn_type = hex(syntax.ty.0);
        self.syn_number = hex(syntax.number.0);
        self.syn_comment = hex(syntax.comment.0);
        self.syn_function = hex(syntax.function.0);
        self.syn_tag = hex(syntax.tag.0);
    }

    /// Whether the window is dark, so what is drawn over it should be light.
    #[must_use]
    pub fn is_dark(&self) -> bool {
        self.bg_app.l < 0.5
    }

    /// The colors of a palette.
    #[must_use]
    pub fn of(palette: &Palette) -> Self {
        match palette {
            Palette::Light => Self::light(),
            Palette::Dark => Self::dark(),
            Palette::Oled => Self::oled(),
            Palette::File(file) => Self::from_file(file),
        }
    }

    /// `base` drawn over a backdrop as the Orchestrator page is: its text in that page's ink and
    /// its surfaces in its glass, light over a `dark` backdrop and dark over a light one. The
    /// accent stays `base`'s; the status and code colors are those of the built-in palette of
    /// that scheme, which read over it.
    fn glazed(base: &Self, dark: bool) -> Self {
        let mut t = if dark { Self::dark() } else { Self::light() };
        let ink = |alpha| ink_over(dark, alpha);
        let glass = |alpha| glass_over(dark, alpha);
        t.text = ink(1.);
        t.text_muted = ink(0.6);
        t.text_subtle = ink(0.45);
        t.bg_app = glass(1.);
        t.bg_editor = glass(1.);
        t.bg_panel = glass(0.8);
        t.bg_elevated = glass(0.96);
        t.bg_input = glass(0.62);
        t.bg_hover = ink(0.08);
        t.tree_hover = ink(0.06);
        t.bg_selected_muted = ink(0.10);
        t.bubble = ink(0.08);
        t.border = ink(0.10);
        t.border_strong = ink(0.14);
        t.accented(Color(code(base.accent)));
        t
    }
}

/// A color as `0xRRGGBB`, its alpha dropped.
pub(super) fn code(color: Hsla) -> u32 {
    let Rgba { r, g, b, .. } = color.into();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a channel is between zero and one"
    )]
    let [r, g, b] = [r, g, b].map(|c| (c.clamp(0., 1.) * 255.).round() as u32);
    r << 16 | g << 8 | b
}

/// Text over a backdrop, at this strength: light over a `dark` one, dark over a light one. The
/// Orchestrator page and a chat over a background draw their text in it.
#[must_use]
pub fn ink_over(dark: bool, alpha: f32) -> Hsla {
    if dark {
        hsla(0.6, 0.25, 0.96, alpha)
    } else {
        hsla(0.6, 0.30, 0.12, alpha)
    }
}

/// The glass over a backdrop, this opaque: dark over a `dark` one, pale over a light one.
#[must_use]
pub fn glass_over(dark: bool, alpha: f32) -> Hsla {
    if dark {
        hsla(0.64, 0.30, 0.05, alpha)
    } else {
        hsla(0.6, 0.30, 0.985, alpha)
    }
}

impl Global for Theme {}

/// The theme in use: the window's, or the one over a backdrop while a part drawn over one is.
pub fn theme(cx: &App) -> &Theme {
    let base = cx.global::<Theme>();
    match (SCOPE.get(), cx.try_global::<Schemed>()) {
        (Some(scheme), Some(schemed)) => &schemed.0[usize::from(scheme == Scheme::Dark)],
        _ => base,
    }
}

/// The window's theme over a light backdrop and over a dark one, as the Orchestrator page draws
/// over its backdrop: for a part of the window drawn over one.
pub(super) struct Schemed(Box<[Theme; 2]>);

impl Global for Schemed {}

thread_local! {
    /// The scheme of the backdrop a part of the window being drawn is over, when it is over one.
    static SCOPE: Cell<Option<Scheme>> = const { Cell::new(None) };
}

/// While it lives, [`theme`] returns the theme over a backdrop of `scheme`; dropped, it returns what it did.
#[must_use]
#[derive(Debug)]
pub struct InScheme(Option<Scheme>);

impl InScheme {
    pub fn enter(scheme: Scheme) -> Self {
        Self(SCOPE.replace(Some(scheme)))
    }
}

impl Drop for InScheme {
    fn drop(&mut self) {
        SCOPE.set(self.0);
    }
}

/// What the theme settings came to, kept for the Orchestrator's backdrop.
pub(super) struct Look(Arc<Applied>);

impl Global for Look {}

/// Whether a window of this appearance is dark.
pub(super) fn dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

/// Draws the app in what the appearance settings came to. `appearance` picks the palette when
/// the app follows the system.
pub fn apply(mut applied: Applied, appearance: WindowAppearance, cx: &mut App) {
    let night = dark(appearance);
    if let (Paint::System { .. }, Some(pair)) = (&applied.paint, &applied.system_backdrops) {
        applied.backdrop = pair[usize::from(night)].clone();
    }
    let palette = match &applied.paint {
        Paint::Fixed(palette) => palette,
        Paint::System { light, dark } => {
            if night {
                dark
            } else {
                light
            }
        }
    };
    let mut theme = Theme::of(palette);
    let file = match palette {
        Palette::File(file) => Some(file),
        _ => None,
    };
    let family = |named: Option<&String>, own: &'static str| {
        named
            .filter(|font| !font.trim().is_empty())
            .map_or(SharedString::new_static(own), SharedString::from)
    };
    *FONTS.write().unwrap_or_else(PoisonError::into_inner) = [
        family(
            applied
                .ui_font
                .as_ref()
                .or(file.and_then(|f| f.fonts.ui.as_ref())),
            FONT_UI,
        ),
        family(file.and_then(|f| f.fonts.code.as_ref()), FONT_MONO),
    ];
    let roomy = applied.density.scale();
    DENSITY.store(roomy.to_bits(), Ordering::Relaxed);
    let corner = file.and_then(|f| f.radius).unwrap_or(RADIUS);
    CORNER.store(corner.clamp(0., MAX_RADIUS).to_bits(), Ordering::Relaxed);
    let finish = |theme: &mut Theme| {
        let code = if theme.is_dark() {
            applied.code_dark
        } else {
            applied.code_light
        };
        if let Some(syntax) = code {
            theme.highlight(syntax);
        }
        if let Some(accent) = applied.accent {
            theme.accented(accent);
        }
    };
    finish(&mut theme);
    let schemed = [false, true].map(|dark| {
        let mut glazed = Theme::glazed(&theme, dark);
        finish(&mut glazed);
        glazed
    });
    TEXT_SCALE.store(applied.text_size.scale().to_bits(), Ordering::Relaxed);
    cx.set_reduce_motion(applied.reduce_motion);
    cx.set_global(theme);
    cx.set_global(Schemed(Box::new(schemed)));
    cx.set_global(Look(Arc::new(applied)));
}

/// Draws the app for a new appearance of the system, from the settings last applied.
pub fn follow(appearance: WindowAppearance, cx: &mut App) {
    let look = applied(cx);
    if matches!(look.paint, Paint::System { .. }) {
        apply(Applied::clone(&look), appearance, cx);
        cx.refresh_windows();
    }
}

/// What the theme settings came to.
pub fn applied(cx: &App) -> Arc<Applied> {
    cx.try_global::<Look>()
        .map_or_else(Arc::default, |look| Arc::clone(&look.0))
}
