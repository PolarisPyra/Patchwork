//! What a tool call asks to do, in terms a permission rule can match.

// Rust guideline compliant 2026-11-27

use patchwork_model::permissions::PermissionKind;
use serde_json::Value;

/// What an approval request asks to do, in terms a rule can match.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub kind: PermissionKind,
    /// Paths, command lines, URLs, or `server/tool` names.
    pub resources: Vec<String>,
}

impl Target {
    /// A request of `kind` for `resources`, without the empty ones.
    pub fn new<S: Into<String>>(
        kind: PermissionKind,
        resources: impl IntoIterator<Item = S>,
    ) -> Self {
        let resources = resources
            .into_iter()
            .map(Into::into)
            .filter(|r: &String| !r.trim().is_empty())
            .collect();
        Self { kind, resources }
    }
}

/// What a tool call asks to do, for the permission rules. The permission hook reads the same
/// mapping, so a call the rules can answer is the same call whether Claude asked or the hook did.
#[must_use]
pub fn claude_target(tool: &str, input: &Value, root: &str) -> Option<Target> {
    let text = |key: &str| input[key].as_str().unwrap_or_default();
    let path = input["file_path"]
        .as_str()
        .or(input["notebook_path"].as_str());
    match tool {
        "Bash" => Some(Target::new(PermissionKind::Command, [text("command")])),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            Some(Target::new(PermissionKind::Edit, path))
        }
        // Reading outside the folder: the file read, which a rule for its folder covers, or the
        // folder searched.
        "Read" => Some(Target::new(PermissionKind::Folder, [path?])),
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
