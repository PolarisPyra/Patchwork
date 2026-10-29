//! What the model is told about each tool, and what the user sees of a call: its name, summary, and whether it needs approval.

use std::path::Path;
use serde_json::{Value, json};
use crate::system::fs::resolve;
use crate::permissions::Target;
use crate::permissions::PermissionKind;
use crate::threads::AccessMode;

/// The tools offered to the model. Plan mode offers only those that change nothing.
pub fn definitions(plan: bool) -> Value {
    let tool = |name: &str, description: &str, parameters: Value| json!({"type": "function", "function": {"name": name, "description": description, "parameters": parameters}});
    let path = json!({"type": "string", "description": "A path relative to the project root, or absolute"});
    let mut tools = vec![
        tool(
            "read_file",
            "Read a text file, or look at an image (PNG, JPEG, GIF, WebP). Returns numbered lines. Use offset and limit for long files.",
            json!({"type": "object", "properties": {
                "path": path,
                "offset": {"type": "integer", "description": "First line to read, from 1"},
                "limit": {"type": "integer", "description": "How many lines to read"}
            }, "required": ["path"]}),
        ),
        tool(
            "list_dir",
            "List a directory's entries. Directories end with /.",
            json!({"type": "object", "properties": {"path": path}}),
        ),
        tool(
            "glob",
            "Find files whose path matches a glob such as **/*.rs, newest first. Skips ignored files.",
            json!({"type": "object", "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string", "description": "Directory to search in; the project root by default"}
            }, "required": ["pattern"]}),
        ),
        tool(
            "grep",
            "Search file contents with a regular expression. Returns path:line: text. Skips ignored files.",
            json!({"type": "object", "properties": {
                "pattern": {"type": "string", "description": "A Rust regular expression"},
                "path": {"type": "string", "description": "File or directory to search; the project root by default"},
                "glob": {"type": "string", "description": "Only search files matching this glob, e.g. *.ts"},
                "case_insensitive": {"type": "boolean"}
            }, "required": ["pattern"]}),
        ),
        tool(
            "fetch_url",
            "Fetch a web page or file over HTTP(S) and return its text.",
            json!({"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}),
        ),
        tool(
            "ask_user",
            "Ask the user one or more multiple-choice questions when a decision is genuinely theirs. The user can also type their own answer.",
            json!({"type": "object", "properties": {"questions": {"type": "array", "items": {
                "type": "object",
                "properties": {
                    "header": {"type": "string", "description": "A label of a few words"},
                    "question": {"type": "string"},
                    "options": {"type": "array", "items": {"type": "object", "properties": {
                        "label": {"type": "string"},
                        "description": {"type": "string"}
                    }, "required": ["label"]}},
                    "multi_select": {"type": "boolean"}
                },
                "required": ["question", "options"]
            }}}, "required": ["questions"]}),
        ),
        tool(
            "update_plan",
            "Show the user a checklist of the steps of a multi-step task, and keep it current as steps finish.",
            json!({"type": "object", "properties": {"steps": {"type": "array", "items": {
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "status": {"type": "string", "enum": ["pending", "running", "done"]}
                },
                "required": ["text", "status"]
            }}}, "required": ["steps"]}),
        ),
    ];
    if plan {
        tools.push(tool(
            "propose_plan",
            "Present the finished implementation plan to the user, as Markdown. Call this once, when the plan is ready.",
            json!({"type": "object", "properties": {"plan": {"type": "string"}}, "required": ["plan"]}),
        ));
        return Value::Array(tools);
    }
    tools.extend([
        tool(
            "write_file",
            "Create or overwrite a file with the given content. Prefer edit_file for changes to an existing file.",
            json!({"type": "object", "properties": {
                "path": path,
                "content": {"type": "string"}
            }, "required": ["path", "content"]}),
        ),
        tool(
            "edit_file",
            "Replace old_string with new_string in a file. old_string must match exactly, including indentation, and be unique unless replace_all is set. Read the file first.",
            json!({"type": "object", "properties": {
                "path": path,
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "replace_all": {"type": "boolean"}
            }, "required": ["path", "old_string", "new_string"]}),
        ),
        tool(
            "bash",
            "Run a shell command in the project root and return its output and exit code.",
            json!({"type": "object", "properties": {
                "command": {"type": "string"},
                "timeout": {"type": "integer", "description": "Seconds before the command is stopped; 120 by default, at most 600"}
            }, "required": ["command"]}),
        ),
    ]);
    // Computer use is only offered once Settings → Permissions has turned it on.
    if !plan && crate::computer::enabled() {
        tools.extend([
            tool(
                "computer_screenshot",
                "Look at the screen. Returns the path of a PNG screenshot, which read_file can show you. Take one before and after each group of actions, since the screen changes only when you act.",
                json!({"type": "object", "properties": {}}),
            ),
            tool(
                "computer_move",
                "Move the pointer, in screen pixels, without clicking.",
                json!({"type": "object", "properties": {
                    "x": {"type": "integer"},
                    "y": {"type": "integer"}
                }, "required": ["x", "y"]}),
            ),
            tool(
                "computer_click",
                "Click the pointer at screen pixels, where you see the spot in the screenshot.",
                json!({"type": "object", "properties": {
                    "x": {"type": "integer"},
                    "y": {"type": "integer"},
                    "button": {"type": "string", "enum": ["left", "middle", "right"]},
                    "double": {"type": "boolean"}
                }, "required": ["x", "y"]}),
            ),
            tool(
                "computer_type",
                "Type text into the focused field, as the keyboard would.",
                json!({"type": "object", "properties": {
                    "text": {"type": "string"}
                }, "required": ["text"]}),
            ),
            tool(
                "computer_key",
                "Press one key, with modifiers held. Keys are named as on the keyboard: enter, tab, escape, space, backspace, delete, up, down, left, right, home, end, pageup, pagedown, or a single character.",
                json!({"type": "object", "properties": {
                    "key": {"type": "string"},
                    "modifiers": {"type": "array", "items": {"type": "string", "enum": ["ctrl", "shift", "alt", "meta"]}}
                }, "required": ["key"]}),
            ),
            tool(
                "computer_scroll",
                "Scroll the window under the pointer by notches; positive is down, or right when horizontal.",
                json!({"type": "object", "properties": {
                    "amount": {"type": "integer"},
                    "horizontal": {"type": "boolean"}
                }, "required": ["amount"]}),
            ),
        ]);
    }
    Value::Array(tools)
}

/// The display name of a tool, as the chat shows tool calls.
pub fn display_name(name: &str) -> &str {
    match name {
        "read_file" => "Read",
        "write_file" => "Write",
        "edit_file" => "Edit",
        "list_dir" => "LS",
        "glob" => "Glob",
        "grep" => "Grep",
        "bash" => "Bash",
        "fetch_url" => "WebFetch",
        "ask_user" => "AskUserQuestion",
        "update_plan" => "TodoWrite",
        "propose_plan" => "ExitPlanMode",
        "computer_screenshot" => "Screenshot",
        "computer_move" => "MoveMouse",
        "computer_click" => "Click",
        "computer_type" => "Type",
        "computer_key" => "PressKey",
        "computer_scroll" => "Scroll",
        other => other,
    }
}

/// A one-line summary of a call's arguments.
pub fn summary(name: &str, args: &Value, root: &Path) -> String {
    let field = |key: &str| args[key].as_str().unwrap_or_default().to_string();
    let summary = match name {
        "bash" => field("command"),
        "fetch_url" => field("url"),
        "glob" | "grep" => field("pattern"),
        "ask_user" => args["questions"][0]["question"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        "update_plan" | "propose_plan" | "computer_screenshot" => String::new(),
        "computer_move" | "computer_click" => format!("{}, {}", field("x"), field("y")),
        "computer_type" => field("text"),
        "computer_key" => {
            let mut parts: Vec<String> = args["modifiers"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .filter_map(|m| m.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            parts.push(field("key"));
            parts.join("+")
        }
        "computer_scroll" => format!("{} {}", field("amount"), args["horizontal"]),
        _ if name.starts_with("mcp__") => args.to_string(),
        _ => args["path"]
            .as_str()
            .map(|p| shown(&resolve(root, p), root))
            .unwrap_or_default(),
    };
    summary
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect()
}

/// A path as the user sees it: relative to the project when inside it.
pub fn shown(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// What a call asks to do, as its title, its detail, what it asks to do for the permission rules,
/// and whether the access mode asks the user about it; otherwise only a rule may stop it. None
/// when no rule can answer it, so it runs at once.
pub fn approval(
    name: &str,
    args: &Value,
    mode: AccessMode,
    root: &Path,
) -> Option<(String, String, Target, bool)> {
    let path = args["path"].as_str().map(|p| resolve(root, p));
    if let Some(path) = &path
        && !path.starts_with(root)
    {
        let asks = mode != AccessMode::FullAccess;
        let shown = path.display().to_string();
        if writes(name) {
            let target = Target::new(PermissionKind::Edit, [shown.clone()]);
            return Some((
                "Change a file outside the project".into(),
                shown,
                target,
                asks,
            ));
        }
        // Reading a file asks for its folder.
        let folder = if name == "read_file" {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        let target = Target::new(PermissionKind::Folder, [folder.display().to_string()]);
        return Some(("Read outside the project".into(), shown, target, asks));
    }
    match name {
        "write_file" | "edit_file" => {
            let title = if name == "write_file" {
                "Write a file"
            } else {
                "Edit a file"
            };
            let target = Target::new(
                PermissionKind::Edit,
                path.iter().map(|p| p.display().to_string()),
            );
            let asks = mode == AccessMode::Supervised;
            Some((
                title.into(),
                path.map(|p| shown(&p, root)).unwrap_or_default(),
                target,
                asks,
            ))
        }
        "bash" => {
            let command = args["command"].as_str().unwrap_or_default();
            let target = Target::new(PermissionKind::Command, [command]);
            let asks = matches!(mode, AccessMode::Supervised | AccessMode::AutoAcceptEdits);
            Some(("Run a command".into(), command.into(), target, asks))
        }
        "fetch_url" => {
            let url = args["url"].as_str().unwrap_or_default();
            let target = Target::new(PermissionKind::Web, [url]);
            Some((
                "Fetch a web page".into(),
                url.into(),
                target,
                mode == AccessMode::Supervised,
            ))
        }
        // Looking at the screen and driving it are the same permission, named for the screen.
        "computer_screenshot"
        | "computer_move"
        | "computer_click"
        | "computer_type"
        | "computer_key"
        | "computer_scroll" => {
            let verb = if name == "computer_screenshot" {
                "Look at"
            } else {
                "Use"
            };
            let target = Target::new(PermissionKind::Screen, ["screen"]);
            Some((
                format!("{verb} the screen"),
                "the whole screen".into(),
                target,
                matches!(mode, AccessMode::Supervised | AccessMode::AutoAcceptEdits),
            ))
        }
        _ => None,
    }
}

pub fn writes(name: &str) -> bool {
    matches!(name, "write_file" | "edit_file")
}
