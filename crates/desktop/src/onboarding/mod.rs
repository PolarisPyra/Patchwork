//! First-run setup: connect the agents, import their conversations, pick a project, then start. The
//! window shows it instead of the workspace until `settings.onboarded` is set, which "Skip setup"
//! and the last step do, and "Run setup again" in Settings → Projects clears.

// Rust guideline compliant 2026-10-09

mod agent_step;
mod frame;
mod imports_step;
mod projects_step;
mod ready_step;
mod rows;
mod state;

use state::{
    AGENT_KINDS, IMPORTS_STEP, ImportProgress, ImportResult, LAST_STEP, LOGO_BLUE, LOGO_PINK,
    LOGO_VIOLET, PROJECTS_STEP, Probe, STEPS,
};
pub use state::{Onboarding, OnboardingEvent};
