//! The `artifact` tool, which a chat's agent makes and changes artifacts with, the way Claude's
//! own `artifacts` tool works: `create` saves the first version, `update` replaces one exact
//! piece of the newest version, and `rewrite` replaces all of it. Each saves a new version.

// Rust guideline compliant 2026-10-09

use serde_json::{Value, json};

use super::types::{Artifact, ArtifactKind};
use crate::system::now_ms;
use crate::{Core, Error};

/// The tool's name on Patchwork's own server.
pub const TOOL: &str = "artifact";

/// What the tool is for, as its agent reads it.
const DESCRIPTION: &str = "\
Show the person something to look at, drawn live on Patchwork's Artifacts page beside the chat: \
a doc, a slide deck, a sheet, a design, a data dashboard, a whiteboard, a web page, an \
interactive tool, a chart, a diagram, or a React component. Use it for substantial, \
self-contained content the person will view, use, edit or keep, usually over 15 lines; not for \
short code, answers or snippets that read fine in the reply. Make one artifact per thing and \
change it with update or rewrite instead of making another. Every change is kept as a version \
the person can go back to. Say in the reply what you made; do not repeat its content there.

Types:
- doc: a document to read or print, as Markdown. It is set on a page of paper; use headings, \
lists and tables, not HTML.
- slides: a deck, as HTML in which each top-level <section> is one 1280x720 slide. Style each \
section yourself (inline <style> is fine) with large type, one idea per slide, and absolute or \
flex layout inside the fixed size. Patchwork scales it, steps through it, and presents it.
- sheet: a spreadsheet, as CSV with a header row. A cell starting with = is a formula: numbers, \
cells such as B2, + - * / ^, brackets, and SUM, AVERAGE, MIN, MAX, COUNT, ROUND and ABS over \
cells and ranges such as B2:B9. Quote fields that hold commas.
- design: a visual design at a fixed size, such as a screen, poster, card or icon, as SVG with \
width and height, or as a whole HTML page with <meta name=\"frame\" content=\"390x844\"> giving \
its size. It is shown as a frame on a canvas.
- data: a dashboard of numbers, charts and tables, as a whole HTML page; draw charts with inline \
SVG or a chart library from cdnjs.cloudflare.com.
- whiteboard: a board of notes, shapes, arrows and labels, as one SVG with a viewBox. It is shown \
on a board the person pans and zooms.
- html: any other web page or interactive tool. svg: a picture. markdown: plain text with \
formatting. mermaid: a Mermaid diagram's source. code: source code shown as text.
- react: one component file whose default export takes no props; it may use Tailwind classes \
and import react and npm packages such as recharts, lucide-react, d3 or three.

Pages must be complete and run on their own: inline styles and scripts, and load scripts only \
from cdnjs.cloudflare.com. They are drawn by Chrome exactly as written, with its default styles: \
a styled <button>, <input> or <select> must set its own border, padding, background, font and \
appearance, or Chrome's default border and padding squeeze it.";

/// The tool's arguments.
fn input_schema() -> Value {
    let kinds: Vec<&str> = ArtifactKind::ALL.iter().map(|k| k.name()).collect();
    json!({"type": "object", "properties": {
        "command": {"type": "string", "enum": ["create", "update", "rewrite"], "description": "create a new artifact, update one exact piece of its newest version, or rewrite all of it."},
        "id": {"type": "string", "description": "A short name for the artifact in kebab-case, such as \"sales-chart\". It names the same artifact for update and rewrite."},
        "type": {"type": "string", "enum": kinds, "description": "What it holds, for create."},
        "title": {"type": "string", "description": "What the person sees it called. Kept when left out."},
        "language": {"type": "string", "description": "For code: its language, such as python."},
        "content": {"type": "string", "description": "The whole content, for create and rewrite."},
        "old_str": {"type": "string", "description": "For update: text that appears exactly once in the newest version."},
        "new_str": {"type": "string", "description": "For update: what replaces old_str."}
    }, "required": ["command", "id"]})
}

/// A text argument, or None when it is missing or empty.
fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args[key].as_str().filter(|s| !s.is_empty())
}

impl Core {
    /// The tool as Patchwork's own server lists it.
    pub(crate) fn artifact_tools(&self, thread_id: &str) -> Vec<Value> {
        if self.store.thread(thread_id).is_none() {
            return Vec::new();
        }
        vec![json!({"name": TOOL, "description": DESCRIPTION, "inputSchema": input_schema()})]
    }

    /// Runs the tool for the agent of `thread_id`.
    ///
    /// # Errors
    ///
    /// Returns an error when an argument is missing or wrong, the artifact to change does not
    /// exist, `old_str` does not appear exactly once, or the database write fails.
    pub(crate) fn run_artifact_tool(&self, thread_id: &str, args: &Value) -> Result<String, Error> {
        let key = text(args, "id").ok_or("Give the artifact's id")?.trim();
        let existing = self.store.artifact_by_key(thread_id, key);
        let command = text(args, "command").unwrap_or_default();
        let (artifact, content) = match (command, existing) {
            ("create", existing) => {
                let kind = text(args, "type")
                    .and_then(ArtifactKind::parse)
                    .ok_or_else(|| {
                        let kinds: Vec<&str> = ArtifactKind::ALL.iter().map(|k| k.name()).collect();
                        format!("Give the type: {}", kinds.join(", "))
                    })?;
                let content = text(args, "content").ok_or("Give the content")?;
                let now = now_ms();
                let mut artifact = existing.unwrap_or_else(|| Artifact {
                    id: uuid::Uuid::new_v4().to_string(),
                    thread_id: thread_id.to_owned(),
                    key: key.to_owned(),
                    kind,
                    title: key.to_owned(),
                    language: None,
                    created_at: now,
                    updated_at: now,
                    pinned: false,
                    versions: 0,
                });
                artifact.kind = kind;
                (artifact, content.to_owned())
            }
            ("update" | "rewrite", None) => {
                return Err(format!("There is no artifact {key}; create it first").into());
            }
            ("rewrite", Some(artifact)) => {
                let content = text(args, "content").ok_or("Give the content")?;
                (artifact, content.to_owned())
            }
            ("update", Some(artifact)) => {
                let old = text(args, "old_str").ok_or("Give old_str")?;
                let new = args["new_str"].as_str().unwrap_or_default();
                let newest = self
                    .store
                    .artifact_versions(&artifact.id)
                    .pop()
                    .ok_or("The artifact has no version to update")?;
                match newest.content.matches(old).count() {
                    1 => {}
                    0 => return Err("old_str is not in the newest version".into()),
                    n => {
                        return Err(format!(
                            "old_str appears {n} times; give more of it so it appears once"
                        )
                        .into());
                    }
                }
                let content = newest.content.replacen(old, new, 1);
                (artifact, content)
            }
            _ => return Err("command is create, update or rewrite".into()),
        };
        let mut artifact = artifact;
        if let Some(title) = text(args, "title") {
            title.clone_into(&mut artifact.title);
        }
        if let Some(language) = text(args, "language") {
            artifact.language = Some(language.to_owned());
        }
        let (title, key) = (artifact.title.clone(), artifact.key.clone());
        let n = self.save_artifact(artifact, &content)?;
        Ok(super::calls::saved(&title, &key, n))
    }
}
