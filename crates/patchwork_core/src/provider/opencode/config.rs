//! Turn-specific OpenCode configuration (`OPENCODE_CONFIG_CONTENT`) for permissions, instructions,
//! skills, todos, and subagents.

// Rust guideline compliant 2026-10-09

use serde_json::{Value, json};

use crate::chats::AccessMode;
use crate::provider::TurnSpec;

use super::{action, models};

/// The tool kinds OpenCode 1.x checks before running; the read-only tools always run.
const GUARDED: [&str; 6] = [
    "bash",
    "webfetch",
    "websearch",
    "codesearch",
    "external_directory",
    "doom_loop",
];

/// Permission rules for the access mode, as `OPENCODE_CONFIG_CONTENT`.
///
/// A run cannot answer a prompt, so an "ask" rule refuses the call and the run goes on. Without
/// rules OpenCode keeps its own: full access approves them with `--auto`, and auto lets OpenCode
/// decide. Patchwork's deny rules are added as they are, since a run cannot ask Patchwork either.
#[must_use]
pub fn permission_config(spec: &TurnSpec) -> Option<Value> {
    let mut rules = serde_json::Map::new();
    let edit = match spec.mode {
        AccessMode::FullAccess | AccessMode::Auto => None,
        AccessMode::AutoAcceptEdits => Some("allow"),
        AccessMode::Supervised => Some("ask"),
    };
    if let Some(edit) = edit {
        rules.insert("edit".into(), json!(edit));
        rules.extend(GUARDED.map(|kind| (kind.to_string(), json!("ask"))));
    }
    for guarded in spec.guarded.iter().filter(|g| g.refuses()) {
        let Some(action) = action(guarded.kind) else {
            continue;
        };
        let action = if action == "shell" { "bash" } else { action };
        if guarded.denied || guarded.deny.iter().any(|p| p == "*") {
            rules.insert(action.into(), json!("deny"));
        } else if action == "bash" {
            // Commands take patterns; the rest of the kinds only a whole answer.
            let mut patterns = match rules.remove(action) {
                Some(Value::String(rest)) => {
                    serde_json::Map::from_iter([("*".to_string(), json!(rest))])
                }
                _ => serde_json::Map::new(),
            };
            for p in &guarded.deny {
                patterns.insert(p.clone(), json!("deny"));
                patterns.insert(format!("{p} *"), json!("deny"));
            }
            rules.insert(action.into(), Value::Object(patterns));
        }
    }
    (!rules.is_empty()).then(|| json!({ "permission": rules }))
}

/// The user's system prompt for `opencode run`, in a file its `instructions` loads into the system
/// prompt. 2.x gets it from the bridge instead (see `bridge/`).
#[must_use]
pub fn instructions_config(spec: &TurnSpec) -> Option<Value> {
    let text = spec.instructions?;
    match crate::extensions::agents::launch_file("opencode", spec.root, "system-prompt.md", text) {
        Ok(file) => Some(json!({ "instructions": [file] })),
        Err(e) => {
            log::warn!(error:% = e; "could not write OpenCode's system prompt: {{error}}");
            None
        }
    }
}

/// 2.x leaves out its `todowrite` tool unless asked, and that tool is what fills the plan above
/// the chat box.
#[must_use]
pub fn todo_config() -> Option<Value> {
    Some(json!({ "experimental": { "todo": true } }))
}

/// The skills folder for 2.x's `skills.paths`.
#[must_use]
pub fn skills_config(spec: &TurnSpec) -> Option<Value> {
    spec.skills_dir
        .map(|dir| json!({ "skills": { "paths": [dir] } }))
}

/// The custom agents as OpenCode subagents, which it can start by name. `build` and
/// `plan` are left to OpenCode, since every turn runs as one of them.
#[must_use]
pub fn agents_config(spec: &TurnSpec) -> Option<Value> {
    // Listing the models runs OpenCode, so only when an agent names one.
    let models = if spec.agents.iter().any(|a| a.model.is_some()) {
        models()
    } else {
        Vec::new()
    };
    let agents: serde_json::Map<String, Value> = spec
        .agents
        .iter()
        .filter(|agent| agent.name != "build" && agent.name != "plan")
        .map(|agent| {
            let mut entry = json!({ "mode": "subagent", "description": agent.about(), "prompt": agent.prompt.trim() });
            // A model OpenCode doesn't have fails the subagent, so it takes the chat's instead.
            if let Some(model) = agent.model.as_deref().and_then(|m| {
                crate::extensions::agents::offered(m, models.iter().filter_map(|o| o.id.as_deref()))
            }) {
                entry["model"] = model.into();
            }
            (agent.name.clone(), entry)
        })
        .collect();
    (!agents.is_empty()).then(|| json!({ "agent": agents }))
}

/// Config parts, each an object of its own keys, as one `OPENCODE_CONFIG_CONTENT`.
pub fn config_content(parts: impl IntoIterator<Item = Option<Value>>) -> Option<String> {
    let mut config = serde_json::Map::new();
    for part in parts.into_iter().flatten() {
        if let Value::Object(part) = part {
            config.extend(part);
        }
    }
    (!config.is_empty()).then(|| Value::Object(config).to_string())
}
