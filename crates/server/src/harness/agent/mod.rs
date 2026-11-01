//! The harness's agent loop, for API providers: it runs one turn against an OpenAI-compatible Chat
//! Completions endpoint, calling the harness's tools until the model answers. Patchwork
//! starts it as the turn's process: the first stdin line is the turn (with the key, which so
//! stays out of the arguments and environment), later lines answer approvals and questions or
//! steer the turn, and stdout carries `api.*` events as JSON lines. Each session's
//! conversation is a JSON file in the sessions folder.

mod calls;
mod context;
mod prompt;
mod request;
mod turn;
mod vision;

pub use turn::*;
