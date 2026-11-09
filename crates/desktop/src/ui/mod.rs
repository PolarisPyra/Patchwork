//! What two or more features share: the theme, the embedded assets, the controls every view is
//! built from, and the menus, dialogs and text tools more than one feature uses.

// Rust guideline compliant 2026-10-09

pub mod assets;
pub mod background_picker;
mod controls;
mod format;
pub mod highlight;
mod logos;
pub mod menu;
pub mod prompt;
pub mod sigils;
pub mod theme;

pub use controls::{
    BtnKind, above, badge, btn, button, checkbox, focus_ring, icon, icon_button, list_scrollbar,
    menu_item, mix, overflows, popup, popup_shadow, project_tile, scrollbar, spinner, tooltip,
};
pub use format::{
    base_name, compact_elapsed, dir_name, display_title, format_clock, format_relative,
    format_tokens, full_elapsed, now_ms, slug_file_name,
};
pub use logos::{
    AGENT_KINDS, agent_label, agent_logo, api_model_place, api_provider_name, provider_logo,
    reload_api_providers,
};
