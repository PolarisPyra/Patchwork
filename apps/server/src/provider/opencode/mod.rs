//! OpenCode CLI (`opencode run --format json`), versions 1.x and 2.x. Each turn is one run that
//! prints a JSON event per line and exits; nothing is written to its stdin.

mod config;
mod run;
mod serve;

pub use config::{
    opencode_bin, auth, models, permission_config, skills_config, agents_config, config_content,
    instructions_config,
};
use config::major_version;
pub use run::{turn_args, parse};
use run::{todo_plan, tool_name};
pub use serve::{uses_serve, bridge_request, mode_asks, steer_line, approval_reply};
use serve::{action, parse_serve};

pub mod bridge;

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::run::tool;
    use crate::orchestration::AgentEvent;
    use crate::provider::Parsed;
    use crate::threads::StepStatus;

    fn todowrite(status: &str) -> Value {
        json!({
            "callID": "call-1",
            "tool": "todowrite",
            "state": {
                "status": status,
                "input": { "todos": [
                    { "content": "Read the code", "status": "completed" },
                    { "content": "Fix the bug", "status": "in_progress" },
                    { "content": "Run the tests", "status": "pending" }
                ] },
                "output": "",
                "error": "no"
            }
        })
    }

    #[test]
    fn a_todowrite_call_reports_its_plan() {
        let out = tool(&todowrite("completed"), "/project", "m1");
        let Some(Parsed::Event(AgentEvent::PlanUpdated { message_id, steps })) = out.last() else {
            panic!("expected a plan, got {} events", out.len());
        };
        assert_eq!(message_id, "m1");
        let got: Vec<(&str, StepStatus)> = steps
            .iter()
            .map(|step| (step.text.as_str(), step.status))
            .collect();
        assert_eq!(
            got,
            [
                ("Read the code", StepStatus::Done),
                ("Fix the bug", StepStatus::Running),
                ("Run the tests", StepStatus::Pending),
            ]
        );
    }

    #[test]
    fn a_failed_todowrite_call_reports_no_plan() {
        let out = tool(&todowrite("error"), "/project", "m1");
        assert!(!out.is_empty(), "the call itself is still shown");
        assert!(
            !out.iter()
                .any(|parsed| matches!(parsed, Parsed::Event(AgentEvent::PlanUpdated { .. }))),
            "a failed call does not replace the plan"
        );
    }
}
