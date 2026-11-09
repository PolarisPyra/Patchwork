//! What an agent reports: reading its output, the events streamed to the UI and saved on the
//! thread, and the tokens it used.

pub(in crate::orchestration) mod events;
pub(in crate::orchestration) mod ingest;
pub(in crate::orchestration) mod log;
pub(in crate::orchestration) mod stream;
pub(in crate::orchestration) mod usage;
