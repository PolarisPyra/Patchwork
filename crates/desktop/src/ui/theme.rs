//! Patchwork's design tokens. Every view reads its colors and sizes from here, never hard-coded.

// Rust guideline compliant 2026-10-09

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
    hex(0x00a9_a9a9)
}

/// `--border-focus`, the same in every theme.
pub fn border_focus() -> Hsla {
    hex(0x0035_74f0)
}

/// `color-mix(in srgb, fg pct%, bg)`, for the tokens mixed from two others.
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
    let mut c = hex(color);
    c.a = pct;
    c
}

impl Theme {
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
