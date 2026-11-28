//! The agent CLIs Patchwork runs over the Agent Client Protocol beside Antigravity.
//!
//! Static specs, binary discovery, and config option normalization live in
//! `patchwork-provider-acp`; this module adapts them to `core`'s [`AgentProvider`] trait.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::Value;

use super::agent::{AgentProvider, Capabilities, Identity};
use super::antigravity;
use super::line_reader::LineReader;
use super::{AgentKind, AgentStatus, ModelOption, Parsed, Prompt, TurnSpec};
use crate::chats::AccessMode;

pub(crate) use patchwork_provider_acp::{
    AcpSpec, LEGACY, SPECS, auth, binary, config_options, env, hides_command, label, models,
    own_command, remember_models, spec,
};

/// The session mode `mode` runs as, when the agent has modes.
pub(crate) fn mode_for(spec: &AcpSpec, mode: AccessMode) -> Option<&'static str> {
    let [supervised, accept_edits, auto, full] = spec.modes?;
    Some(match mode {
        AccessMode::Supervised => supervised,
        AccessMode::AutoAcceptEdits => accept_edits,
        AccessMode::Auto => auto,
        AccessMode::FullAccess => full,
    })
}

#[derive(Debug)]
struct AcpProvider(&'static AcpSpec);

static PROVIDERS: [AcpProvider; 3] = [
    AcpProvider(&SPECS[0]),
    AcpProvider(&SPECS[1]),
    AcpProvider(&SPECS[2]),
];

/// Returns the [`AgentProvider`] adapter for one of the listed ACP agents.
pub(crate) fn provider(kind: AgentKind) -> Option<&'static dyn AgentProvider> {
    PROVIDERS
        .iter()
        .find(|p| p.0.kind == kind)
        .map(|p| p as &'static dyn AgentProvider)
}

impl AgentProvider for AcpProvider {
    fn kind(&self) -> AgentKind {
        self.0.kind
    }

    fn identity(&self) -> Identity {
        Identity {
            id: self.0.id,
            name: self.0.label,
            title: self.0.label,
            logo: self.0.logo,
            cli: Some(self.0.binary),
        }
    }

    fn sign_in(&self, status: &mut AgentStatus) {
        auth(self.0, status);
    }

    fn models(&self) -> (Option<String>, Vec<ModelOption>) {
        (None, models(self.0.kind))
    }

    fn locate(&self) -> Option<PathBuf> {
        binary(self.0)
    }

    fn capabilities(&self) -> Capabilities {
        antigravity::ACP
    }

    fn turn_args(&self, spec: &TurnSpec) -> Vec<String> {
        antigravity::turn_args(spec)
    }

    fn turn_env(&self, spec: &TurnSpec) -> Vec<(&'static str, String)> {
        antigravity::turn_env(spec)
    }

    fn turn_input(&self, spec: &TurnSpec) -> Vec<String> {
        antigravity::turn_input(spec)
    }

    fn parse(&self, value: &Value, spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
        antigravity::parse(value, spec, message_id)
    }

    fn reader(&self) -> Box<dyn LineReader> {
        Box::new(antigravity::AcpReader::default())
    }

    // A prompt sent during a reply may be dropped, so a steer stops the reply instead.
    fn steer_input(
        &self,
        _prompt: Prompt<'_>,
        session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> Option<String> {
        Some(antigravity::interrupt(session_id?))
    }

    fn resume_input(
        &self,
        prompt: Prompt<'_>,
        session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> Option<String> {
        antigravity::resume_input(self.0.kind, prompt, session_id)
    }

    fn interrupt(&self, session_id: Option<&str>, _turn_id: Option<&str>) -> Option<String> {
        Some(antigravity::interrupt(session_id?))
    }

    fn approval_reply(
        &self,
        reply: &Value,
        allow: bool,
        answers: &HashMap<String, Vec<String>>,
    ) -> String {
        antigravity::approval_reply(reply, allow, answers)
    }

    fn own_key_vars(&self) -> &'static [&'static str] {
        self.0.key_vars
    }
}
