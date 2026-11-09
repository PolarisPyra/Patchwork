//! First-run setup (`features/onboarding/Onboarding.tsx`): connect the agents, import their
//! conversations, pick a project, then start. The window shows it instead of the workspace until
//! `settings.onboarded` is set, which "Skip setup" and the last step do, and "Run setup again" in
//! Settings → Projects clears.

mod agent_step;
mod frame;
mod imports_step;
mod projects_step;
mod ready_step;
mod rows;
mod state;

pub use state::*;
