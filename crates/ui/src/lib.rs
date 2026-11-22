//! What two or more features share: the theme, the embedded assets, the controls every view is
//! built from, the app's actions, and the menus, dialogs and text tools more than one feature
//! uses.

// Rust guideline compliant 2026-10-09

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

mod actions;
pub mod assets;
pub mod avatar;
pub mod backdrop;
pub mod background_picker;
pub mod dialog;
mod controls;
mod format;
pub mod highlight;
mod logos;
pub mod markdown;
pub mod menu;
pub mod pill;
pub mod prompt;
mod scrollbar;
mod tasks;
pub mod selection;
pub mod sigils;
pub mod text_area;
pub mod theme;

pub use actions::{
    APP_TITLE, DeleteChat, FindInChat, FocusNext, FocusPrev, GoBack, GoForward, NewChat,
    NewTerminal, OpenSettings, Quit, ShowArchived, ShowChanges, ShowOrchestrator, ShowQuilts,
    ShowTasks, ToggleOutput, TogglePalette, TogglePanel, ToggleTerminal,
};
pub use controls::{
    BtnKind, above, badge, btn, button, checkbox, dropdown_chip, focus_ring, icon, icon_button,
    kind_icon, input_box, menu_item, mix, motion_clock, next_step, overflows, popup, popup_shadow,
    redraw_after, spinner, tooltip,
};
pub use scrollbar::{list_scrollbar, scrollbar};
pub use tasks::{after, off_thread, pick_paths};
pub use format::{
    LOG_LEVELS, base_name, compact_elapsed, dir_name, display_title, effort_label, format_clock,
    format_date, format_relative, resets_in, format_tokens, full_elapsed, now_ms, slug_file_name,
};
pub use logos::{
    AGENT_KINDS, agent_label, agent_logo, api_model_place, api_provider_name, platform_logo,
    provider_logo, reload_api_providers,
};
