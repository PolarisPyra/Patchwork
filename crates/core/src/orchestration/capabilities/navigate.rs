//! The capabilities that show things in Patchwork's window, send a project's files and a
//! chat's changes to the chat app, and bring conversations in and out of Patchwork.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;

use serde_json::{Value, json};

use super::chats::{chat, text};
use super::service::{changed, project};
use super::types::Ctx;
use crate::Error;
use crate::events::{AgentEvent, AgentEventEnvelope, Place};
use crate::import::ImportSource;
use crate::projects::Project;
use crate::chats::Role;
use crate::projects::read_file;

/// The most files offered when a name fits several.
const CANDIDATES: usize = 8;

/// The sections of Settings, by the id the window opens them with.
const SECTIONS: &[&str] = &[
    "chat",
    "logs",
    "notifications",
    "updates",
    "themes",
    "code-theme",
    "orchestrator-background",
    "chat-background",
    "interface",
    "shortcuts",
    "providers",
    "tokens",
    "extensions",
    "permissions",
    "security",
    "projects",
    "scheduled",
    "terminal",
    "ssh",
    "gateway",
];

/// What a file's name holds when it is likely to keep keys or passwords.
const SECRETS: &[&str] = &[
    ".env",
    ".pem",
    ".key",
    ".p12",
    ".pfx",
    ".keystore",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "credentials",
    "secret",
    "api-keys",
    ".netrc",
    ".npmrc",
    "token",
    "password",
    "passwd",
    ".ssh/",
    ".gnupg/",
    ".aws/",
    ".kube/",
    "kubeconfig",
    ".docker/config",
    ".git-credentials",
    ".pgpass",
    ".gpg",
    ".asc",
    ".kdbx",
    "history",
    "known_hosts",
    "authorized_keys",
    "cookies",
    "wallet",
];

/// Asks the window to show `place`.
fn show(ctx: &Ctx<'_>, place: Place) {
    ctx.core.emit(AgentEventEnvelope {
        chat_id: String::new(),
        event: AgentEvent::Show { place },
    });
}

/// The file of `project` that `args` names: its path, or a name or part of one that fits one
/// file.
///
/// # Errors
///
/// Returns an error when no file fits or several do; that error lists them, to ask about.
fn file(ctx: &Ctx<'_>, project: &Project, args: &Value) -> Result<String, Error> {
    let wanted = text(args, "file").trim_start_matches("./");
    let files = ctx.core.list_files(&project.id)?;
    if files.iter().any(|f| f == wanted) {
        return Ok(wanted.to_string());
    }
    let lower = wanted.to_lowercase();
    // A whole file name beats a part of a path.
    let named: Vec<&String> = files
        .iter()
        .filter(|f| {
            let path = f.to_lowercase();
            path == lower || path.ends_with(&format!("/{lower}"))
        })
        .collect();
    let found = if named.is_empty() {
        files
            .iter()
            .filter(|f| f.to_lowercase().contains(&lower))
            .collect()
    } else {
        named
    };
    match found.as_slice() {
        [] => Err(format!(
            "No file of {} has a path like that; list_project_files lists them",
            project.name
        )
        .into()),
        [only] => Ok((*only).clone()),
        several => Err(format!(
            "{} files of {} fit that. Ask the person which, then pass its path: {}",
            several.len(),
            project.name,
            json!(several.iter().take(CANDIDATES).collect::<Vec<_>>())
        )
        .into()),
    }
}

/// Opens a project in the window.
pub(super) fn open_project(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = project(ctx, args)?;
    show(
        ctx,
        Place::Project {
            id: found.id.clone(),
        },
    );
    Ok(format!(
        "Asked Patchwork's window to open {}. It shows there when the window is running.",
        found.name
    ))
}

/// Opens a project's file in the window's file panel.
pub(super) fn open_file(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = project(ctx, args)?;
    let path = file(ctx, &found, args)?;
    show(
        ctx,
        Place::File {
            project_id: found.id.clone(),
            path: path.clone(),
        },
    );
    Ok(format!(
        "Asked Patchwork's window to open {path} of {}. It shows there when the window is \
         running.",
        found.name
    ))
}

/// Opens Settings in the window, on a section.
pub(super) fn open_settings(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let section = text(args, "section").to_lowercase();
    if !section.is_empty() && !SECTIONS.contains(&section.as_str()) {
        return Err(format!(
            "Settings has no section {section}. Its sections are: {}",
            SECTIONS.join(", ")
        )
        .into());
    }
    show(
        ctx,
        Place::Settings {
            section: (!section.is_empty()).then_some(section),
        },
    );
    Ok(
        "Asked Patchwork's window to open Settings. It shows there when the window is running."
            .into(),
    )
}

/// Sends a project's text file to the conversation.
pub(super) fn send_file(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = project(ctx, args)?;
    let path = file(ctx, &found, args)?;
    let lower = path.to_lowercase();
    if SECRETS.iter().any(|s| lower.contains(s)) {
        // A file of keys leaves the computer for good once it is in a chat app.
        ctx.escalate(
            "Patchwork: send a file that may hold keys or passwords to the chat app",
            &format!("{path} of {}", found.name),
        )?;
    }
    let root = ctx.core.project_root(&found.id)?;
    let contents = read_file(&root, &path)?;
    // The name goes into the chat app's upload request, where a quote or a line break would
    // end its field.
    let name: String = path
        .rsplit('/')
        .next()
        .unwrap_or(&path)
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let caption = format!("{path} of {}", found.name);
    let took = ctx
        .core
        .send_file_to_chats(&ctx.caller.id, &name, contents.as_bytes(), &caption)?;
    Ok(json!({"sent": path, "project": found.name, "conversations": took}).to_string())
}

/// Sends what a chat's latest reply changed to the conversation, as a patch.
pub(super) fn send_changes(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let chat = chat(ctx, args)?;
    let chats = ctx.core.bound_conversations(&ctx.caller.id);
    if chats.is_empty() {
        return Err("No chat app conversation talks to this chat".into());
    }
    for bound in &chats {
        if let Some(nothing) =
            ctx.core
                .send_diff(bound.platform, &bound.conversation, Some(&chat.id))
        {
            return Err(nothing.into());
        }
    }
    Ok(format!(
        "Sent what the latest reply of \u{201c}{}\u{201d} changed.",
        chat.title
    ))
}

/// Sends a chat's whole conversation to the chat app, as a Markdown file.
pub(super) fn export_chat(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let chat = chat(ctx, args)?;
    let mut page = format!("# {}\n", chat.title);
    let mut messages = 0;
    for message in chat.messages.iter().filter(|m| !m.text.trim().is_empty()) {
        let who = if message.role == Role::User {
            "You"
        } else {
            "Agent"
        };
        // Writing to a string cannot fail.
        let _ = write!(page, "\n## {who}\n\n{}\n", message.text.trim());
        messages += 1;
    }
    if messages == 0 {
        return Err("That chat has nothing written in it yet".into());
    }
    let name: String = chat
        .title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    let name = format!("{}.md", name.trim_matches('-'));
    let took = ctx.core.send_file_to_chats(
        &ctx.caller.id,
        &name,
        page.as_bytes(),
        &format!("{}: {messages} messages", chat.title),
    )?;
    Ok(json!({"sent": name, "messages": messages, "conversations": took}).to_string())
}

/// The name a source of conversations goes by.
fn source_name(source: ImportSource) -> String {
    serde_json::to_value(source)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// How many conversations each agent saved on the computer, and how many are not in Patchwork.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn scan_imports(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let sources: Vec<Value> = ctx
        .core
        .import_sources()
        .into_iter()
        .map(|source| {
            let scan = ctx.core.scan_imports(source);
            json!({
                "source": source_name(source),
                "found": scan.found,
                "not_in_patchwork": scan.new,
            })
        })
        .collect();
    Ok(json!({"sources": sources, "next": "import_chats brings them in"}).to_string())
}

/// Brings the conversations agents saved on the computer into Patchwork as chats.
pub(super) fn import_chats(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let offered = ctx.core.import_sources();
    let sources = match text(args, "source").to_lowercase().as_str() {
        "" => offered,
        name => {
            let one = offered
                .into_iter()
                .find(|s| source_name(*s) == name)
                .ok_or("No source is called that; scan_imports lists them")?;
            vec![one]
        }
    };
    let imported = ctx.core.import_chats(&sources)?;
    changed(ctx);
    Ok(json!({"imported": imported}).to_string())
}
