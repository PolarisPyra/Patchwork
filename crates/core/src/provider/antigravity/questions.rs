//! Questions agy asks the user. It sends them as permission requests whose call id starts with
//! `interaction_`, with one option per choice, and takes the chosen option's id as the answer.

// Rust guideline compliant 2026-11-23

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::chats::{Question, QuestionOption};
use crate::provider::Parsed;

/// The call id prefix agy gives a question it asks through a permission request.
const QUESTION_PREFIX: &str = "interaction_";

/// The permission request `v` as a question for the user, when agy asks one. None for a
/// request for permission, or a question whose choices cannot be told apart.
pub(super) fn question(v: &Value) -> Option<Parsed> {
    let params = &v["params"];
    let id = params["toolCall"]["toolCallId"]
        .as_str()
        .filter(|id| id.starts_with(QUESTION_PREFIX))?;
    let options = params["options"].as_array().filter(|o| !o.is_empty())?;
    // Each choice is answered by its label, so the labels lead back to agy's option ids.
    let mut picks = Map::new();
    let mut choices = Vec::with_capacity(options.len());
    for option in options {
        let option_id = option["optionId"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        let label = option["name"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(option_id);
        if picks.insert(label.to_string(), json!(option_id)).is_some() {
            return None;
        }
        choices.push(QuestionOption {
            label: label.to_string(),
            description: String::new(),
        });
    }
    let text = params["toolCall"]["title"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Choose an option.");
    Some(Parsed::Approval {
        title: "Question".into(),
        detail: text.into(),
        reply: json!({"id": v["id"], "question": id, "picks": picks}),
        questions: vec![Question {
            id: id.into(),
            header: "Question".into(),
            question: text.into(),
            options: choices,
            multi_select: false,
        }],
        target: None,
    })
}

/// The option agy offered for the user's answer to the question in `reply`. None when the
/// question was declined, or answered with something agy did not offer.
pub(super) fn picked<'a>(
    reply: &'a Value,
    allow: bool,
    answers: &HashMap<String, Vec<String>>,
) -> Option<&'a str> {
    let id = reply["question"].as_str()?;
    let answer = answers.get(id).filter(|_| allow)?.first()?;
    reply["picks"][answer.as_str()].as_str()
}
