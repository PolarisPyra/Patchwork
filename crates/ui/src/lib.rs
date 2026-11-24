//! What two or more features share: the theme, the embedded assets, the controls every view is
//! built from, the app's actions, and the menus, dialogs and text tools more than one feature
//! uses.

// Rust guideline compliant 2026-10-09

#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

mod actions;
pub mod backdrop;
pub mod background_picker;
pub mod brand;
pub mod controls;
mod tasks;
pub mod text;
pub mod theme;

pub use actions::{
    APP_TITLE, CloseChatTab, DeleteChat, FindInChat, FocusNext, FocusPrev, GoBack, GoForward,
    NewChat, NewTerminal, OpenSettings, Quit, ShowArchived, ShowChanges, ShowIssues,
    ShowOrchestrator, ShowQuilts, ShowTasks, ToggleOutput, TogglePalette, TogglePanel,
    ToggleTerminal,
};
pub use controls::icons::{icon, kind_icon};
pub use controls::inputs::{
    BtnKind, badge, btn, button, checkbox, dropdown_chip, focus_ring, icon_button, input_box, mix,
};
pub use controls::motion::{motion_clock, next_step, overflows, redraw_after, spinner};
pub use controls::popups::{above, menu_item, popup, popup_shadow, tooltip};
pub use controls::scrollbar::{list_scrollbar, scrollbar};
pub use tasks::{after, off_thread, pick_paths};
pub use text::format::{
    LOG_LEVELS, base_name, compact_elapsed, dir_name, display_title, effort_label, format_clock,
    format_date, format_relative, resets_in, format_tokens, full_elapsed, now_ms, slug_file_name,
};
pub use brand::logos::{
    AGENT_KINDS, agent_label, agent_logo, api_model_place, api_provider_name, platform_logo,
    provider_logo, reload_api_providers,
};
