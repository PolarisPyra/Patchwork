//! The agents in the desktop store.

// Rust guideline compliant 2026-10-09

use std::ops::Deref;
use std::sync::Arc;

use patchwork_core::Core;
use patchwork_core::agents::Agent;

use super::AppState;

/// Every agent, oldest first.
#[derive(Debug, Clone, Default)]
pub struct AgentsStore(Arc<Vec<Agent>>);

impl AgentsStore {
    pub(super) fn read(core: &Core) -> Self {
        Self(Arc::new(core.agents()))
    }
}

impl Deref for AgentsStore {
    type Target = [Agent];

    fn deref(&self) -> &[Agent] {
        &self.0
    }
}

impl AppState {
    /// Reads the agents again.
    pub fn reload_agents(&mut self) {
        self.agents = AgentsStore::read(&self.core);
    }

    /// Agent `id`.
    #[must_use]
    pub fn agent(&self, id: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.id == id)
    }
}
