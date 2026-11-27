//! The capabilities that show things in Patchwork's window, send a project's files, pictures, a
//! screenshot and a chat's changes to the chat app, and bring conversations in and out of
//! Patchwork.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

use serde_json::{Value, json};

use super::chats::chat;
use crate::orchestration::tasks::chats::text;
use super::service::{changed, project};
use super::types::Ctx;
use crate::Error;
use crate::events::{Place, WindowCommand};
use crate::import::ImportSource;
use crate::projects::Project;
use crate::chats::Role;
use crate::gateway::{FILE_LIMIT, is_picture};
use crate::projects::read_bytes;

/// The most files offered when a name fits several.
const CANDIDATES: usize = 8;

/// The largest picture read to send, in bytes. Larger than what chat apps take, since a picture
/// over that is made smaller before it goes; small enough that a stray huge file is not read
/// whole.
const PICTURE_READ_LIMIT: u64 = 64 * 1024 * 1024;

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
    ctx.core.command(WindowCommand::Show { place });
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

/// Sends a project's file, text or not, to the conversation.
pub(super) fn send_file(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = project(ctx, args)?;
    let path = file(ctx, &found, args)?;
    if holds_secrets(&path) {
        // A file of keys leaves the computer for good once it is in a chat app.
        ctx.escalate(
            "Patchwork: send a file that may hold keys or passwords to the chat app",
            &format!("{path} of {}", found.name),
        )?;
    }
    let root = ctx.core.project_root(&found.id)?;
    let contents = read_bytes(&root, &path, FILE_LIMIT as u64)?;
    if contents.len() > FILE_LIMIT {
        return Err(format!(
            "{path} is larger than the {} MB chat apps take",
            FILE_LIMIT / (1024 * 1024)
        )
        .into());
    }
    let caption = format!("{path} of {}", found.name);
    let took =
        ctx.core
            .send_file_to_chats(&ctx.caller.id, &upload_name(&path), &contents, &caption)?;
    Ok(json!({
        "sent": path,
        "project": found.name,
        "bytes": contents.len(),
        "conversations": took,
    })
    .to_string())
}

/// Sends a picture to the conversation: a project's, or one anywhere on the computer by its
/// whole path, which the person allows first.
pub(super) fn send_picture(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let caption = text(args, "caption").trim();
    let (path, bytes, outside) = if text(args, "project").is_empty() {
        let (path, bytes) = outside_picture(text(args, "file"))?;
        let outside = !in_a_project(ctx, &path);
        (path, bytes, outside)
    } else {
        let found = project(ctx, args)?;
        let path = file(ctx, &found, args)?;
        let root = ctx.core.project_root(&found.id)?;
        let bytes = read_bytes(&root, &path, PICTURE_READ_LIMIT)?;
        (format!("{path} of {}", found.name), bytes, false)
    };
    if bytes.len() as u64 > PICTURE_READ_LIMIT {
        return Err("The picture is larger than 64 MB".into());
    }
    // Only a picture goes, which the bytes show; so no file of keys goes out under a picture's
    // name.
    if !is_picture(&bytes) {
        return Err(format!("{path} is not a PNG, JPEG, GIF or WebP picture").into());
    }
    if outside {
        // A picture outside the projects may be anything on the computer, such as a photo or a
        // scan of a document, and it leaves the computer for good once it is in a chat app.
        ctx.escalate(
            "Patchwork: send a picture from outside your projects to the chat app",
            &path,
        )?;
    }
    let stem = Path::new(&upload_name(&path))
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("picture")
        .to_string();
    let caption = if caption.is_empty() {
        path.as_str()
    } else {
        caption
    };
    let (name, took) = ctx
        .core
        .send_picture_to_chats(&ctx.caller.id, &stem, bytes, caption)?;
    Ok(json!({"sent": name, "from": path, "conversations": took}).to_string())
}

/// Takes a screenshot of the screen and sends it to the conversation.
pub(super) fn send_screenshot(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    // Nothing is captured that has nowhere to go.
    if ctx.core.bound_conversations(&ctx.caller.id).is_empty() {
        return Err("No chat app conversation talks to this chat".into());
    }
    let shot = ctx.core.agent_screenshot()?;
    let caption = match text(args, "caption").trim() {
        "" => "The screen now",
        given => given,
    };
    let (name, took) =
        ctx.core
            .send_picture_to_chats(&ctx.caller.id, "screenshot", shot.png, caption)?;
    Ok(json!({
        "sent": name,
        "width": shot.width,
        "height": shot.height,
        "conversations": took,
    })
    .to_string())
}

/// The picture at the whole path `given` on the computer: its path with links followed, and its
/// bytes.
///
/// # Errors
///
/// Returns an error when `given` is not a whole path, names no file, or is where keys or
/// passwords are kept.
fn outside_picture(given: &str) -> Result<(String, Vec<u8>), Error> {
    let given = given.trim();
    if given.is_empty() || !Path::new(given).is_absolute() {
        return Err(
            "Give the picture's whole path on the computer, or the project it is in and its path there"
                .into(),
        );
    }
    let real =
        crate::system::files::paths::canonicalize(given).map_err(|e| format!("{given}: {e}"))?;
    if !real.is_file() {
        return Err(format!("{given} is not a file").into());
    }
    let path = real.to_string_lossy().into_owned();
    if holds_secrets(&path) {
        return Err(format!(
            "{path} is where keys or passwords are kept, so Patchwork does not send it"
        )
        .into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&real)
        .and_then(|f| f.take(PICTURE_READ_LIMIT + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("{path}: {e}"))?;
    Ok((path, bytes))
}

/// Whether the whole path `path`, with links followed, is in a project on this computer.
fn in_a_project(ctx: &Ctx<'_>, path: &str) -> bool {
    ctx.core.settings().projects.iter().any(|p| {
        crate::system::files::paths::canonicalize(&p.path)
            .is_ok_and(|root| Path::new(path).starts_with(root))
    })
}

/// Whether the path `path` looks like it holds keys or passwords.
fn holds_secrets(path: &str) -> bool {
    let lower = path.to_lowercase();
    SECRETS.iter().any(|s| lower.contains(s))
}

/// The last part of `path`, as a file name a chat app's upload takes: a quote or a line break
/// would end its field in the request.
fn upload_name(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect()
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
