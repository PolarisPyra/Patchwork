//! The panel under the chat (`features/panel/Panel.tsx`): the integrated terminal, the Output view
//! with this launch's logs, and with computer use on, the Screen view. It stays alive while
//! closed, so its shells keep running and it keeps its tab.

mod cells;
mod header;
mod input;
mod menus;
mod output;
mod render;
mod screen;
mod state;
mod terminal;

pub use state::*;
