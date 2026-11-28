//! What the agents' replies used in tokens, and what those tokens cost.
//!
//! A reply's cost is the one saved with it: the agent's own figure where it gives one, else each
//! model request priced at API rates. A reply saved before costs were kept is priced by its
//! chat's model. Replies of deleted chats, and replies undone, stay counted from a ledger kept as
//! they go.
//!
//! - `types`: the report and its parts.
//! - `prices`: the API rates, and pricing a request or a reply.
//! - `ledger`: the replies counted, live and deleted, and keeping a reply's cost as it goes.
//! - `report`: adding them up over a range, and the report as CSV.

// Rust guideline compliant 2026-10-09

mod ledger;
mod prices;
mod report;
mod types;

pub(crate) use prices::{priced, reported, turn_model};
pub(crate) use types::ReplyCost;
pub use types::{
    Bucket, ChatCost, CostFilter, CostRange, CostReport, ModelCost, ProjectCost, ProviderCost,
    Tally,
};
