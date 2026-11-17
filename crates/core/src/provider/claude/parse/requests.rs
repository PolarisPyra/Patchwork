//! The control requests Claude Code sends while a turn runs: permission prompts and questions, read into approvals.

// Rust guideline compliant 2026-10-09

use serde_json::Value;
use crate::permissions::Target;
use crate::permissions::PermissionKind;
use crate::threads::Question;
use crate::threads::QuestionOption;
use crate::provider::one_line;
use crate::provider::Parsed;
use crate::provider::summarize;

/// A `can_use_tool` request becomes an approval; other control requests are refused.
pub(super) fn claude_control_request(v: &Value, root: &str) -> Parsed {
    let request = &v["request"];
    if request["subtype"] != "can_use_tool" {
        return Parsed::Send(
            serde_json::json!({
                "type": "control_response",
                "response": {
                    "subtype": "error",
                    "request_id": v["request_id"],
                    "error": "Not supported by Patchwork",
                },
            })
            .to_string(),
        );
    }
    let tool = request["tool_name"].as_str().unwrap_or("a tool");
    let reply = serde_json::json!({"request_id": v["request_id"], "input": request["input"]});
    if tool == "AskUserQuestion" {
        let questions = claude_questions(&request["input"]);
        return Parsed::Approval {
            title: "Question".into(),
            detail: questions
                .first()
                .map(|q| q.question.clone())
                .unwrap_or_default(),
            reply,
            questions,
            target: None,
        };
    }
    let title = match tool {
        "Bash" => "Run command".to_string(),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => "Edit file".to_string(),
        _ => format!("Use {}", request["display_name"].as_str().unwrap_or(tool)),
    };
    let mut detail = summarize(&request["input"], root);
    if detail.is_empty() {
        detail = one_line(request["description"].as_str().unwrap_or_default());
    }
    Parsed::Approval {
        title,
        detail,
        reply,
        questions: vec![],
        target: claude_target(tool, &request["input"], root),
    }
}

/// What a tool call asks to do, for the permission rules. The permission hook reads the same
/// mapping, so a call the rules can answer is the same call whether Claude asked or the hook did.
pub(crate) fn claude_target(tool: &str, input: &Value, root: &str) -> Option<Target> {
    let text = |key: &str| input[key].as_str().unwrap_or_default();
    let path = input["file_path"]
        .as_str()
        .or(input["notebook_path"].as_str());
    match tool {
        "Bash" => Some(Target::new(PermissionKind::Command, [text("command")])),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            Some(Target::new(PermissionKind::Edit, path))
        }
        // Reading outside the folder: the file's folder, or the folder searched.
        "Read" => {
            let path = std::path::Path::new(path?);
            let folder = path.parent().unwrap_or(path).to_string_lossy().into_owned();
            Some(Target::new(PermissionKind::Folder, [folder]))
        }
        "Glob" | "Grep" | "LS" => Some(Target::new(
            PermissionKind::Folder,
            [input["path"].as_str().unwrap_or(root)],
        )),
        "WebFetch" => Some(Target::new(PermissionKind::Web, [text("url")])),
        _ => {
            let (server, tool) = tool.strip_prefix("mcp__")?.split_once("__")?;
            Some(Target::new(
                PermissionKind::Mcp,
                [format!("{server}/{tool}")],
            ))
        }
    }
}

/// `AskUserQuestion`'s questions. Claude takes each answer back under the question's text.
fn claude_questions(input: &Value) -> Vec<Question> {
    let questions = input["questions"].as_array().map_or(&[][..], Vec::as_slice);
    questions
        .iter()
        .filter_map(|q| {
            let question = q["question"].as_str()?.to_string();
            let options = q["options"].as_array().map_or(&[][..], Vec::as_slice);
            Some(Question {
                id: question.clone(),
                header: q["header"].as_str().unwrap_or_default().into(),
                question,
                options: options
                    .iter()
                    .filter_map(|o| {
                        Some(QuestionOption {
                            label: o["label"].as_str()?.into(),
                            description: o["description"].as_str().unwrap_or_default().into(),
                        })
                    })
                    .collect(),
                multi_select: q["multiSelect"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}
