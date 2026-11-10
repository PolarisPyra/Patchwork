//! The built-in hooks by name, and running one as its own process.

// Rust guideline compliant 2026-10-09

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::provider::AgentKind;

use super::bounded_waits;

/// The argument that starts Patchwork's binary as a built-in hook.
pub const HOOK_ARG: &str = "--hook";

/// A built-in hook, by the name a plugin gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BuiltinHook {
    /// Refuses a shell command that polls in a loop with no time limit, or waits for a word cargo
    /// prints in colour.
    BoundedWaits,
}

impl BuiltinHook {
    #[must_use]
    pub fn from_name(name: &str) -> Option<BuiltinHook> {
        match name {
            "boundedWaits" => Some(BuiltinHook::BoundedWaits),
            _ => None,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            BuiltinHook::BoundedWaits => "boundedWaits",
        }
    }

    /// The tools the hook is for when its plugin names none, as a matcher.
    #[must_use]
    pub fn matcher(self) -> &'static str {
        match self {
            BuiltinHook::BoundedWaits => crate::provider::hooks::SHELL_TOOLS,
        }
    }

    /// What the hook does, as Settings → Extensions → Hooks shows it.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            BuiltinHook::BoundedWaits => "Refuses waits with no time limit",
        }
    }

    /// Why the call `input` describes is refused, or None to let it run. `input` is the call as
    /// the agent gives it to its hooks.
    #[must_use]
    pub fn check(self, input: &Value) -> Option<String> {
        match self {
            BuiltinHook::BoundedWaits => {
                bounded_waits::check(&crate::provider::hooks::shell_command(input)?)
            }
        }
    }
}

/// Runs a built-in hook as an agent CLI starts it: reads the call on stdin and writes the answer
/// that agent reads. An unknown hook or agent lets the call run.
pub fn run() -> i32 {
    let mut args = std::env::args().skip(2);
    let hook = args.next().as_deref().and_then(BuiltinHook::from_name);
    let agent: Option<AgentKind> = args
        .next()
        .and_then(|agent| serde_json::from_value(Value::String(agent)).ok());
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let (Some(hook), Some(agent)) = (hook, agent) else {
        return 0;
    };
    let call: Value = serde_json::from_str(&input).unwrap_or_default();
    let answer = crate::provider::hooks::answer(agent, hook.check(&call));
    let mut stdout = std::io::stdout();
    let _ = stdout
        .write_all(answer.as_bytes())
        .and_then(|()| stdout.flush());
    0
}
