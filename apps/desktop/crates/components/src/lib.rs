//! Patchwork's generic GPUI design-system primitives and application actions.

// Rust guideline compliant 2026-10-09

mod actions;
pub mod avatar;
pub mod confirm_dialog;
pub mod dialog;
pub(crate) mod inputs;
pub mod menu;
pub(crate) mod motion;
pub mod pill;
pub(crate) mod popover;
pub(crate) mod scrollbar;

pub use actions::{
    APP_TITLE, CloseChatTab, DeleteChat, FindInChat, FocusNext, FocusPane1, FocusPane2, FocusPane3,
    FocusPane4, FocusPaneDown, FocusPaneLeft, FocusPaneRight, FocusPaneUp, FocusPrev, GoBack,
    GoForward, MaximizePane, MoveTabDown, MoveTabLeft, MoveTabRight, MoveTabUp, NewChat,
    NewTerminal, NextTab, OpenSettings, PrevTab, Quit, ReopenTab, ShowArchived, ShowChanges,
    ShowOrchestrator, ShowQuilts, ShowTasks, SplitPaneDown, SplitPaneRight, SwapPaneDown,
    SwapPaneLeft, SwapPaneRight, SwapPaneUp, ToggleOutput, TogglePalette, TogglePanel,
    ToggleTerminal,
};
pub use inputs::{
    BtnKind, badge, btn, button, checkbox, dropdown_chip, focus_ring, icon_button, input_box, mix,
};
pub use motion::{motion_clock, next_step, overflows, redraw_after, spinner};
pub use popover::{
    above, dialog_shadow, menu_item, palette_shadow, popup, popup_shadow, suggestion_shadow,
    tooltip,
};
pub use scrollbar::{list_scrollbar, scrollbar};
