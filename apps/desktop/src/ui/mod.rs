//! What two or more features share: the theme, the embedded assets, the controls every view is
//! built from, and the menus, dialogs and text tools more than one feature uses.

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

pub use controls::*;
pub use format::*;
pub use logos::*;
