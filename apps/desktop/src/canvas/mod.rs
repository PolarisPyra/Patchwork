//! The file panel right of the chat (`features/canvas/Canvas.tsx` and
//! `workspace/components/CanvasArea.tsx`): a file's code, its diff, or a reply's plan, with the
//! file's name, its actions, and its staged state.

mod actions;
mod body;
mod diff;
mod header;
mod render;
mod state;

pub use state::*;
