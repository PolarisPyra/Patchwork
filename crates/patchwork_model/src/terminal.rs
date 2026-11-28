//! The integrated terminal's settings.

// Rust guideline compliant 2026-10-09

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The integrated terminal: its environment, font, cursor, and scrollback, with their defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate setting the user turns on or off"
)]
pub struct TerminalSettings {
    /// Variables added to the shell's environment; null removes one. Values may use
    /// `${env:NAME}` and `${workspaceFolder}`.
    pub env: BTreeMap<String, Option<String>>,
    /// Whether the shell starts from Patchwork's environment, rather than a minimal one.
    pub inherit_env: bool,
    /// The folder shells start in; None is the project's. May use `${workspaceFolder}`.
    pub cwd: Option<String>,
    /// Whether `LANG` is set to a UTF-8 locale: `auto` when it is missing or not UTF-8.
    pub detect_locale: DetectLocale,
    /// None uses the app's monospace font.
    pub font_family: Option<String>,
    pub font_size: f32,
    pub line_height: f32,
    pub letter_spacing: f32,
    pub font_weight: String,
    pub font_weight_bold: String,
    pub cursor_style: CursorStyle,
    pub cursor_blinking: bool,
    /// The cursor's width when `cursor_style` is `line`.
    pub cursor_width: u32,
    pub scrollback: u32,
    pub copy_on_selection: bool,
    pub right_click_behavior: RightClickBehavior,
    pub alt_click_moves_cursor: bool,
    pub mac_option_is_meta: bool,
    pub draw_bold_text_in_bright_colors: bool,
    pub minimum_contrast_ratio: f32,
    pub smooth_scrolling: bool,
    pub mouse_wheel_scroll_sensitivity: f32,
    /// The scroll multiplier while Alt is held.
    pub fast_scroll_sensitivity: f32,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        let mac = cfg!(target_os = "macos");
        Self {
            env: BTreeMap::new(),
            inherit_env: true,
            cwd: None,
            detect_locale: DetectLocale::Auto,
            font_family: None,
            font_size: if mac { 12.0 } else { 14.0 },
            line_height: if cfg!(target_os = "linux") { 1.1 } else { 1.0 },
            letter_spacing: 0.0,
            font_weight: "normal".into(),
            font_weight_bold: "bold".into(),
            cursor_style: CursorStyle::Block,
            cursor_blinking: false,
            cursor_width: 1,
            scrollback: 1000,
            copy_on_selection: false,
            right_click_behavior: if mac {
                RightClickBehavior::SelectWord
            } else if cfg!(windows) {
                RightClickBehavior::CopyPaste
            } else {
                RightClickBehavior::Default
            },
            alt_click_moves_cursor: true,
            mac_option_is_meta: false,
            draw_bold_text_in_bright_colors: true,
            minimum_contrast_ratio: 4.5,
            smooth_scrolling: false,
            mouse_wheel_scroll_sensitivity: 1.0,
            fast_scroll_sensitivity: 5.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DetectLocale {
    #[default]
    Auto,
    Off,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Block,
    Line,
    Underline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RightClickBehavior {
    #[default]
    Default,
    CopyPaste,
    Paste,
    SelectWord,
    Nothing,
}
