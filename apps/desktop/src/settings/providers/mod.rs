//! Settings → Providers (`providers/ProvidersPane.tsx`): the agent CLIs, whether each is installed
//! and connected, the API providers saved, the presets to add one from, and the model servers on
//! this computer. A CLI's Details and a provider's editor take the whole pane while open.

mod cli;
mod form;
mod local;
mod pickers;
mod prompt;
mod render;
mod rows;
mod state;

pub use state::*;
pub use render::*;
