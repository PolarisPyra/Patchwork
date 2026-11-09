//! Approvals a turn waits on, answered by the user or by the permission rules, including the
//! permission hooks Codex and Antigravity run before a tool call, and the agent's requests for a
//! secret.

pub(in crate::orchestration) mod answer;
pub(in crate::orchestration) mod hooks;
pub(in crate::orchestration) mod secrets;
