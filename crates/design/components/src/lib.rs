//! Patchwork's GPUI primitives: buttons, inputs, menus, dialogs, popovers, badges, avatars,
//! scrollbars, the spinner and the animation clock.

// Rust guideline compliant 2026-11-27

pub mod avatar;
pub mod confirm_dialog;
pub mod dialog;
pub(crate) mod inputs;
pub mod menu;
pub(crate) mod motion;
pub(crate) mod popover;
pub(crate) mod scrollbar;

pub use inputs::{
    BtnKind, badge, btn, button, checkbox, dropdown_chip, focus_ring, icon_button, input_box, mix,
};
pub use motion::{motion_clock, next_step, overflows, redraw_after, spinner};
pub use popover::{
    above, dialog_shadow, menu_item, palette_shadow, popup, popup_shadow, suggestion_shadow,
    tooltip,
};
pub use scrollbar::{list_scrollbar, scrollbar};
