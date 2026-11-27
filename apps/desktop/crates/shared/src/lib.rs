//! Shared desktop UI helpers that bind core domain types to theme marks and display labels.

// Rust guideline compliant 2026-10-09

mod logos;

pub use logos::{
    AGENT_KINDS, agent_label, agent_logo, api_model_place, api_provider_name, platform_logo,
    reload_api_providers, source_label,
};
