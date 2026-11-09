//! The box a message is written in (`features/chat/composer/Composer.tsx`): the editor, the files
//! attached to the message, and the toolbar of pickers, formatting, attach, stop and send.

mod chips;
mod menus;
mod model_notice;
mod model_picker;
mod render;
mod send;
mod sigil_menu;
mod state;

pub use state::*;
