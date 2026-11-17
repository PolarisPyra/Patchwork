//! Tests for reading plans from OpenCode's todo tool calls.

// Rust guideline compliant 2026-10-09

use serde_json::{Value, json};

use super::run::tool;
use crate::events::AgentEvent;
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
