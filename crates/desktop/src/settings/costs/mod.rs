//! Settings → Costs: the tokens the agents' replies used and what they cost, over a range of
//! days: by day, by kind of token, by chat, by provider, by model and by project.
//!
//! - `state`: the report, the range and the picked bar, and loading and exporting the report.
//! - `render`: the pane.
//! - `chart`: the bars of cost per day or month.
//! - `rows`: the tiles and the rows of each breakdown.

// Rust guideline compliant 2026-10-09

mod chart;
mod render;
mod rows;
mod state;

pub use render::render;
pub use state::CostsState;
