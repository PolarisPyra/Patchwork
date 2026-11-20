//! Settings → Costs: the tokens the agents' replies used and what they cost, over a range of
//! days: by day, by kind of token, by chat, by provider, by model and by project.
//!
//! - `state`: the report, the range and the picked bar, and loading and exporting the report.
//! - `render`: the pane.
//! - `hero`: the overview's total and each provider's share.
//! - `chart`: the bars of cost or tokens per day or month, and the scale the charts share.
//! - `lines`: each provider's line over the range.
//! - `rows`: the summary strip and the rows of each breakdown.

// Rust guideline compliant 2026-10-09

mod chart;
mod hero;
mod lines;
mod render;
mod rows;
mod state;

pub use render::render;
pub use state::CostsState;
