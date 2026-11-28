//! Patchwork's own GPUI primitives, where gpui-component has none: the focusable base, inputs,
//! menus, dialogs, popovers, avatars, scrollbars and the animation clock.

// Rust guideline compliant 2026-11-27

pub mod avatar;
pub mod confirm_dialog;
pub mod dialog;
pub(crate) mod inputs;
pub mod menu;
pub(crate) mod motion;
pub(crate) mod popover;
pub(crate) mod scrollbar;

pub use inputs::{button, focus_ring, input_box, mix};
pub use motion::{motion_clock, next_step, overflows, redraw_after};
pub use popover::{
    above, dialog_shadow, menu_item, palette_shadow, popup, popup_shadow, suggestion_shadow,
};
pub use scrollbar::{list_scrollbar, scrollbar};
