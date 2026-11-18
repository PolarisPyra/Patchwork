//! Bots: a named agent with standing instructions, files it knows, and the projects it may work
//! in. Each run of a bot is a thread of its own that names the bot.

// Rust guideline compliant 2026-10-09

mod knowledge;
mod manage;
mod run;
mod types;

#[doc(inline)]
pub use types::{Bot, BotRun, BotUsage, KnowledgeFile, STARTING_POINTS, StartingPoint};
