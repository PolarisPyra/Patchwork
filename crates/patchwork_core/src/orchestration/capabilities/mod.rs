//! Everything Patchwork can be asked to do from a chat app, as one table of capabilities.
//!
//! A capability names one operation of Patchwork with what it takes, what it gives back and
//! what it needs before it runs. A chat app's dispatcher is given them as its tools and has
//! nothing else, so its agent reaches Patchwork only through `registry::invoke`. The few
//! commands that act on the conversation itself are answered in `crate::gateway::commands`.

// Rust guideline compliant 2026-10-09

mod chat_table;
mod chats;
mod control_table;
mod integrations;
mod navigate;
mod registry;
mod service;
mod settings;
mod table;
mod tasks;
mod types;

pub(crate) use registry::{Ran, tools};
