//! The logos and names of agents, API providers and chat apps, from the model's types.

// Rust guideline compliant 2026-11-27

mod files;
mod logos;

pub use files::{logo_file, provider_logo};
pub use logos::{
    AGENT_KINDS, agent_label, agent_logo, api_model_place, api_provider_name, platform_logo,
    reload_api_providers, set_api_provider_loader, source_label,
};
