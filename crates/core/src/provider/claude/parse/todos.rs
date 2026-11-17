//! Claude Code's task list, which it keeps with `TaskCreate` and `TaskUpdate` and Patchwork shows as
//! the reply's plan.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use serde_json::Value;

use crate::threads::PlanStep;
use crate::threads::StepStatus;

use crate::provider::step_status;

/// One task: the id Claude gave it, once its `TaskCreate` result names it, and its step.
#[derive(Debug)]
struct Todo {
    id: Option<String>,
    step: PlanStep,
}

/// Every task Claude has made in this process. Each call names one task, and a new task's id only
/// comes back in its call's result, so the whole list is kept here to show it as one plan.
#[derive(Debug, Default)]
pub struct TaskList {
    todos: Vec<Todo>,
    /// A `TaskCreate` call waiting for its result, by call id, with its task's index.
    creating: HashMap<String, usize>,
}

impl TaskList {
    /// The list after one of Claude's lines, when the line changed it. A subagent's lines are its own.
    pub fn read(&mut self, v: &Value) -> Option<Vec<PlanStep>> {
        if !v["parent_tool_use_id"].is_null() {
            return None;
        }
        let blocks = v["message"]["content"].as_array()?;
        let mut changed = false;
        for block in blocks {
            match (v["type"].as_str(), block["type"].as_str()) {
                (Some("assistant"), Some("tool_use")) => changed |= self.call(block),
                (Some("user"), Some("tool_result")) => changed |= self.result(block, v),
                _ => {}
            }
        }
        changed.then(|| self.todos.iter().map(|todo| todo.step.clone()).collect())
    }

    fn call(&mut self, block: &Value) -> bool {
        let input = &block["input"];
        match block["name"].as_str() {
            Some("TaskCreate") => {
                // A list Claude finished is done with, so a new task starts a new one.
                if self.todos.iter().all(|t| t.step.status == StepStatus::Done) {
                    self.todos.clear();
                    self.creating.clear();
                }
                if let Some(call) = block["id"].as_str() {
                    self.creating.insert(call.into(), self.todos.len());
                }
                self.todos.push(Todo {
                    id: None,
                    step: PlanStep {
                        text: input["subject"].as_str().unwrap_or_default().into(),
                        status: step_status(None),
                    },
                });
                true
            }
            Some("TaskUpdate") => {
                let Some(index) = input["taskId"]
                    .as_str()
                    .and_then(|id| self.todos.iter().position(|t| t.id.as_deref() == Some(id)))
                else {
                    return false;
                };
                if input["status"] == "deleted" {
                    self.todos.remove(index);
                    self.creating
                        .values_mut()
                        .for_each(|i| *i -= usize::from(*i > index));
                    return true;
                }
                let step = &mut self.todos[index].step;
                if let Some(status) = input["status"].as_str() {
                    step.status = step_status(Some(status));
                }
                if let Some(subject) = input["subject"].as_str() {
                    step.text = subject.into();
                }
                true
            }
            _ => false,
        }
    }

    /// A `TaskCreate` result names its task's id; a failed one made no task.
    fn result(&mut self, block: &Value, line: &Value) -> bool {
        let Some(index) = block["tool_use_id"]
            .as_str()
            .and_then(|call| self.creating.remove(call))
        else {
            return false;
        };
        if index >= self.todos.len() {
            return false;
        }
        if block["is_error"] == true {
            self.todos.remove(index);
            self.creating
                .values_mut()
                .for_each(|i| *i -= usize::from(*i > index));
            return true;
        }
        self.todos[index].id = line["tool_use_result"]["task"]["id"]
            .as_str()
            .map(String::from);
        false
    }
}
