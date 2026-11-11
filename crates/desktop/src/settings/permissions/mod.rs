//! Settings → Permissions: what agents may do without asking,
//! everywhere, in chats without a project, and in each project: a default for each kind of
//! request, and rules for commands. A rule in a project or in chats is checked before one for
//! everywhere; deny and ask rules win over allow rules. Every change saves at once.

// Rust guideline compliant 2026-10-09

mod state;
mod rows;
mod render;

pub use state::{PermissionsState};
pub(crate) use state::{kind_label, scope_name};
pub use render::{render};
