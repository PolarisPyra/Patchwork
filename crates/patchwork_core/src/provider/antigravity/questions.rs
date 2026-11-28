//! Questions agy asks the user. It sends them as permission requests whose call id starts with
//! `interaction_`, with one option per choice, and takes the chosen option's id as the answer.
//! agy 1.5.16 also asks with its `ask_question` tool: it ends its reply on the pending call and
//! takes the next prompt's text as the answer.

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

/// The questions of agy's `ask_question` call in `update`, once it waits on them. agy ends its
/// reply there and takes the next prompt as the answer, so the request is answered by a message
/// (see [`BY_MESSAGE`]). None for any other call, or one with no question to ask.
pub(super) fn asked(update: &Value, name: &str) -> Option<Parsed> {
    if name != "ask_question" || update["status"].as_str() != Some("pending") {
        return None;
    }
    let call = update["toolCallId"].as_str()?;
    let args = if update["rawInput"]["questions"].is_array() {
        &update["rawInput"]
    } else {
        &update["args"]
    };
    let questions: Vec<Question> = args["questions"]
        .as_array()?
        .iter()
        .enumerate()
        .filter_map(|(at, q)| {
            let question = q["question"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())?;
            let options = q["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| o.as_str().or_else(|| o["label"].as_str()))
                .map(str::trim)
                .filter(|o| !o.is_empty())
                .map(|label| QuestionOption {
                    label: label.into(),
                    description: String::new(),
                })
                .collect();
            let multi_select = ["isMultiSelect", "is_multi_select", "multiSelect"]
                .iter()
                .any(|key| q[*key] == true);
            Some(Question {
                id: format!("{call}-{at}"),
                header: "Question".into(),
                question: question.into(),
                options,
                multi_select,
            })
        })
        .collect();
    let first = questions.first()?.question.clone();
    Some(Parsed::Approval {
        title: "Question".into(),
        detail: first,
        reply: json!({ BY_MESSAGE: true }),
        questions,
        target: None,
    })
}

/// The key in an approval's reply that marks a question answered by the user's next message
/// rather than by a reply to a request, for agy's `ask_question`.
pub const BY_MESSAGE: &str = "by_message";

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
