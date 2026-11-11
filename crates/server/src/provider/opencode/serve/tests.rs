//! What the permissions sent to the server allow.

// Rust guideline compliant 2026-10-09

use crate::threads::{AccessMode};
use crate::provider::TurnSpec;

use super::request::serve_permissions;

fn spec(mode: AccessMode, plan: bool) -> TurnSpec<'static> {
    TurnSpec {
        root: "/w",
        thread_id: "thread",
        session_id: None,
        model: None,
        vision_model: None,
        text: "go",
        mode,
        plan,
        effort: None,
        compact_window: 0,
        attachments: &[],
        images: &[],
        instructions: None,
        skills_dir: None,
        guarded: &[],
        agents: &[],
        hooks: &[],
    }
}

/// The rule an action ends up with, which is its last matching one.
fn effect_rules(rules: &serde_json::Value) -> Vec<(String, String)> {
    rules
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some((
                r["action"].as_str()?.to_string(),
                r["effect"].as_str()?.to_string(),
            ))
        })
        .collect()
}

fn effect_of(rules: &serde_json::Value, action: &str) -> Option<String> {
    effect_rules(rules)
        .into_iter()
        .rfind(|(a, _)| a == action)
        .map(|(_, effect)| effect)
}

/// Every action a rule can answer asks Patchwork, in every mode, so a rule saved mid-turn
/// decides the next call. OpenCode's own loop guard keeps its own call.
#[test]
fn every_action_a_rule_can_answer_asks_patchwork() {
    for mode in [
        AccessMode::Supervised,
        AccessMode::AutoAcceptEdits,
        AccessMode::Auto,
        AccessMode::FullAccess,
    ] {
        let rules = serve_permissions(&spec(mode, false));
        for action in ["shell", "edit", "external_directory", "webfetch"] {
            assert_eq!(
                effect_of(&rules, action).as_deref(),
                Some("ask"),
                "{mode:?} asks Patchwork about {action}: {rules}"
            );
        }
        // MCP tools ask under their id, `<server>_<tool>`.
        assert_eq!(effect_of(&rules, "*_*").as_deref(), Some("ask"), "{mode:?}");
        assert_eq!(
            effect_of(&rules, "doom_loop").as_deref(),
            Some("allow"),
            "{mode:?}"
        );
    }
}

#[test]
fn plan_mode_still_refuses_edits() {
    let rules = serve_permissions(&spec(AccessMode::Auto, true));
    assert_eq!(
        effect_of(&rules, "edit").as_deref(),
        Some("deny"),
        "{rules}"
    );
}

mod dump_rules {
    #[test]
    fn print_rules() {
        for (mode, plan) in [
            (crate::threads::AccessMode::Supervised, false),
            (crate::threads::AccessMode::AutoAcceptEdits, false),
            (crate::threads::AccessMode::Auto, false),
            (crate::threads::AccessMode::FullAccess, false),
            (crate::threads::AccessMode::Supervised, true),
        ] {
            let spec = crate::provider::TurnSpec {
                root: "/w",
                thread_id: "thread",
                session_id: None,
                model: None,
                vision_model: None,
                text: "go",
                mode,
                plan,
                effort: None,
                compact_window: 0,
                attachments: &[],
                images: &[],
                instructions: None,
                skills_dir: None,
                guarded: &[],
                agents: &[],
                hooks: &[],
            };
            println!(
                "RULES {mode:?} plan={plan} {}",
                super::super::request::serve_permissions(&spec)
            );
        }
    }
}
