//! Patchwork's design tokens. Every view reads its colors and sizes from here, never hard-coded.

use gpui::{App, Global, Hsla, Rgba, WindowAppearance};
use patchwork_server::settings::Theme as ThemeSetting;

/// The colors one of `theme.css`'s three palettes gives each token.
#[derive(Clone, Copy)]
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

fn hex(value: u32) -> Hsla {
    gpui::rgb(value).into()
}

fn hexa(value: u32) -> Hsla {
    gpui::rgba(value).into()
}

/// The placeholder colour a web page's own inputs show (`darkGray`), whatever the theme. The
/// composer, commit message and model search set theirs to `text_subtle`.
pub fn input_placeholder() -> Hsla {
    hex(0xa9a9a9)
}

/// `--border-focus`, the same in every theme.
pub fn border_focus() -> Hsla {
    hex(0x3574f0)
}

/// `color-mix(in srgb, fg pct%, bg)`, for the tokens the web UI mixes in place.
fn mix(fg: u32, bg: u32, pct: f32) -> Hsla {
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
fn tint(color: u32, pct: f32) -> Hsla {
    let mut c: Hsla = hex(color);
    c.a = pct;
    c
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            bg_app: hex(0x1e1f22),
            bg_panel: hex(0x2b2d30),
            bg_editor: hex(0x1e1f22),
            bg_input: hex(0x1e1f22),
            bg_hover: hex(0x393b40),
            bg_selected: hex(0x2e436e),
            bg_selected_muted: hex(0x43454a),
            tree_hover: hexa(0xffffff14),
            tree_selected: hexa(0x3574f04d),
            bg_elevated: hex(0x2b2d30),
            shadow: hexa(0x00000073),
            bg_accent_soft: hexa(0x3574f029),
            border: hex(0x393b40),
            border_strong: hex(0x4e5157),
            text: hex(0xdfe1e5),
            text_muted: hex(0x9fa2a8),
            text_subtle: hex(0x6f737a),
            text_on_accent: hex(0xffffff),
            accent: hex(0x3574f0),
            accent_hover: hex(0x4682fa),
            accent_text: hex(0x548af7),
            success: hex(0x5fb865),
            danger: hex(0xdb5c5c),
            danger_soft: hexa(0xdb5c5c24),
            warning: hex(0xf2c55c),
            warning_soft: hexa(0xf2c55c24),
            syn_keyword: hex(0xcf8e6d),
            syn_string: hex(0x6aab73),
            syn_property: hex(0xc77dbb),
            syn_type: hex(0x16baac),
            syn_number: hex(0x2aacb8),
            syn_comment: hex(0x7a7e85),
            syn_function: hex(0x56a8f5),
            syn_tag: hex(0xd5b778),
            diff_add_bg: hexa(0x549b5a2e),
            diff_del_bg: hexa(0xdb5c5c2e),
            bubble: mix(0xdfe1e5, 0x1e1f22, 0.10),
            text_tint_6: tint(0xdfe1e5, 0.06),
            text_tint_8: tint(0xdfe1e5, 0.08),
        }
    }

    /// The dark palette with its surfaces pushed to true black.
    pub fn oled() -> Self {
        Self {
            bg_app: hex(0x000000),
            bg_panel: hex(0x0a0a0a),
            bg_editor: hex(0x000000),
            bg_input: hex(0x000000),
            bg_hover: hex(0x1a1a1b),
            bg_selected_muted: hex(0x1a1a1b),
            bg_elevated: hex(0x0e0e0e),
            shadow: hexa(0x000000b3),
            border: hex(0x222327),
            border_strong: hex(0x34363b),
            bubble: mix(0xdfe1e5, 0x000000, 0.10),
            ..Self::dark()
        }
    }

    pub fn light() -> Self {
        Self {
            bg_app: hex(0xf7f8fa),
            bg_panel: hex(0xf7f8fa),
            bg_editor: hex(0xffffff),
            bg_input: hex(0xffffff),
            bg_hover: hex(0xdfe1e5),
            bg_selected: hex(0xd4e2ff),
            bg_selected_muted: hex(0xebecf0),
            tree_hover: hex(0xdfe1e5),
            tree_selected: hex(0xd4e2ff),
            bg_elevated: hex(0xffffff),
            shadow: hexa(0x0000001f),
            bg_accent_soft: hex(0xedf3ff),
            border: hex(0xebecf0),
            border_strong: hex(0xc9ccd6),
            text: hex(0x000000),
            text_muted: hex(0x6c707e),
            text_subtle: hex(0x818594),
            text_on_accent: hex(0xffffff),
            accent: hex(0x3574f0),
            accent_hover: hex(0x2d63cc),
            accent_text: hex(0x3574f0),
            success: hex(0x208a3c),
            danger: hex(0xe55765),
            danger_soft: hex(0xfdeef0),
            warning: hex(0xc27d04),
            warning_soft: hex(0xfdf5e1),
            syn_keyword: hex(0x0033b3),
            syn_string: hex(0x067d17),
            syn_property: hex(0x871094),
            syn_type: hex(0x008080),
            syn_number: hex(0x1750eb),
            syn_comment: hex(0x8c8c8c),
            syn_function: hex(0x00627a),
            syn_tag: hex(0x0033b3),
            diff_add_bg: hex(0xe6f5ea),
            diff_del_bg: hex(0xfdeef0),
            bubble: mix(0x000000, 0xffffff, 0.10),
            text_tint_6: tint(0x000000, 0.06),
            text_tint_8: tint(0x000000, 0.08),
        }
    }

    /// The palette a theme setting resolves to; `System` follows the window's appearance.
    pub fn for_setting(setting: ThemeSetting, appearance: WindowAppearance) -> Self {
        match setting {
            ThemeSetting::Light => Self::light(),
            ThemeSetting::Dark => Self::dark(),
            ThemeSetting::Oled => Self::oled(),
            ThemeSetting::System => match appearance {
                WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            },
        }
    }
}

impl Global for Theme {}

/// The theme in use.
pub fn theme(cx: &App) -> &Theme {
    cx.global::<Theme>()
}

/// Font families and sizes, from `theme.css`'s `:root`.
pub const FONT_UI: &str = "Inter";
pub const FONT_MONO: &str = "JetBrains Mono";
pub const FS_XS: f32 = 11.;
pub const FS_SM: f32 = 12.;
pub const FS_MD: f32 = 13.;
pub const FS_LG: f32 = 15.;
pub const FS_XL: f32 = 20.;
pub const FS_XXL: f32 = 28.;
pub const RADIUS_SM: f32 = 4.;
pub const RADIUS: f32 = 6.;
pub const RADIUS_LG: f32 = 10.;
pub const TOOLBAR_H: f32 = 40.;
pub const STATUSBAR_H: f32 = 27.;
pub const ACTIVITY_BAR_W: f32 = 40.;
