//! Patchwork's own agent, run against an API provider, behind the provider interface.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use serde_json::Value;

use crate::extensions::skills::Skill;
use crate::provider::agent::{AgentProvider, Capabilities, Identity, Logo};
use crate::settings::Settings;
use crate::provider::turn::prompt_text;
use crate::provider::{AgentKind, Parsed, Prompt, TurnSpec};

/// Patchwork's own agent, which reads its turn, answers and steers from stdin.
#[derive(Debug)]
pub(crate) struct Api;

impl AgentProvider for Api {
    fn kind(&self) -> AgentKind {
        AgentKind::Api
    }

    fn identity(&self) -> Identity {
        Identity {
            id: "api",
            name: "API",
            title: "API",
            logo: Logo::ApiProvider,
            cli: None,
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            runs_in_patchwork: true,
            streams_text: true,
            dispatches: true,
            patchwork_runs_commands: true,
            ..Capabilities::BASE
        }
    }

    // Patchwork's own agent has no skill loader, so it is told where the skills are.
    fn instructions(&self, settings: &Settings, skills: &[Skill]) -> Option<String> {
        let shell = Some(crate::orchestration::agent_instructions(
            settings,
            AgentKind::Api,
        ))
        .filter(|t| !t.is_empty());
        let parts = [shell, crate::extensions::skills::instructions(skills)];
        Some(parts.into_iter().flatten().collect::<Vec<_>>().join("\n\n")).filter(|t| !t.is_empty())
    }

    fn sign_in(&self, status: &mut crate::provider::AgentStatus) {
        super::auth(status);
    }

    fn models(&self) -> (Option<String>, Vec<crate::provider::ModelOption>) {
        (super::default_model(), super::models())
    }

    fn turn_args(&self, _spec: &TurnSpec) -> Vec<String> {
        super::turn_args()
    }

    fn turn_input(&self, spec: &TurnSpec) -> Vec<String> {
        vec![super::bridge_request(spec)]
    }

    fn parse(&self, value: &Value, _spec: &TurnSpec, message_id: &str) -> Vec<Parsed> {
        super::parse(value, message_id)
    }

    fn steer_input(
        &self,
        prompt: Prompt<'_>,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
    ) -> Option<String> {
        Some(super::steer_line(
            &prompt_text(prompt.text, prompt.attachments),
            prompt.images,
        ))
    }

    fn interrupt(&self, _session_id: Option<&str>, _turn_id: Option<&str>) -> Option<String> {
        Some(super::interrupt())
    }

    fn approval_reply(
        &self,
        reply: &Value,
        allow: bool,
        answers: &HashMap<String, Vec<String>>,
    ) -> String {
        super::approval_reply(reply, allow, answers)
    }
}
