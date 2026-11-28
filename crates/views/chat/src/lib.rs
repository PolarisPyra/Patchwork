//! The chat's pieces that the desktop and the phone share, drawn from its messages alone.
//!
//! - The transcript: a turn's steps, folds and tool labels, the pill a Patchwork tool shows, the
//!   images a sent message's text holds, the turns the rail down the transcript's edge lists, and
//!   the token use under the composer.
//! - The composer: the glass pill a message is written in, the characters that open its menus,
//!   and the chip an attachment shows as.
//! - A chat being dragged, from a pane's tabs or the sidebar's task tree.
//!
//! Nothing here reads the window or the client.

// Rust guideline compliant 2026-11-27

mod attachments;
pub mod chat_drag;
pub mod image_markdown;
pub mod note_pill;
pub mod pill;
pub mod sigils;
pub mod turn_contents;
pub mod turns;
pub mod usage;

pub use attachments::attachment_chip;
