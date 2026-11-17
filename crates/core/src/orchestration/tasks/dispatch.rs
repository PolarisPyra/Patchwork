//! What only a chat app's dispatcher does with the chat tools (see `crate::threads::chat_tools`):
//! naming the projects, picking the one a chat starts in, and having its conversations told
//! about the chats it starts or messages.

// Rust guideline compliant 2026-10-09

use serde_json::{Value, json};

use super::chats::busy;
use crate::orchestration::Runner;
use crate::settings::Settings;
use crate::threads::Thread;
use crate::threads::chat_tools::{fit, named};
use crate::{Core, Error};

/// The projects, each with how many chats it has and how many are replying.
pub(super) fn list_projects(runner: &Runner, core: &Core) -> String {
    let chats: Vec<Thread> = core
        .store
        .threads()
        .into_iter()
        .filter(|t| t.parent.is_none() && t.archived_at.is_none())
        .collect();
    let settings = core.store.settings();
    if settings.projects.is_empty() {
        return "Patchwork has no projects yet. A chat started without one works in a folder of \
                its own."
            .into();
    }
    let projects: Vec<Value> = settings
        .projects
        .iter()
        .map(|p| {
            let own: Vec<&Thread> = chats
                .iter()
                .filter(|t| t.project_id.as_ref() == Some(&p.id))
                .collect();
            json!({
                "name": p.name,
                "chats": own.len(),
                "replying": own.iter().filter(|t| busy(runner, &t.id)).count(),
                "usual": settings.gateway.project_id.as_ref() == Some(&p.id),
            })
        })
        .collect();
    Value::Array(projects).to_string()
}

/// The id of the project a dispatcher's chat starts in: the one `name` fits best, or without a
/// name the one Settings → Chat apps picks, which may be none.
///
/// # Errors
///
/// Returns an error naming the projects when none is called `name`, or when it fits several
/// equally.
pub(in crate::orchestration) fn project(
    settings: &Settings,
    name: &str,
) -> Result<Option<String>, Error> {
    let known = |id: &String| settings.projects.iter().any(|p| &p.id == id);
    if name.is_empty() {
        return Ok(settings.gateway.project_id.clone().filter(known));
    }
    let lower = name.to_lowercase();
    if let Some(exact) = settings
        .projects
        .iter()
        .find(|p| p.name.to_lowercase() == lower)
    {
        return Ok(Some(exact.id.clone()));
    }
    let wanted = named(name);
    let best = settings
        .projects
        .iter()
        .map(|p| fit(&wanted, &p.name))
        .max()
        .unwrap_or(0);
    let fits: Vec<_> = settings
        .projects
        .iter()
        .filter(|p| best > 0 && fit(&wanted, &p.name) == best)
        .collect();
    match fits.as_slice() {
        [only] => Ok(Some(only.id.clone())),
        [] => {
            let names: Vec<&str> = settings.projects.iter().map(|p| p.name.as_str()).collect();
            Err(format!(
                "No project is called that. The projects are: {}",
                names.join(", ")
            )
            .into())
        }
        several => {
            let names: Vec<&str> = several.iter().map(|p| p.name.as_str()).collect();
            Err(format!(
                "Several projects fit that: {}. Ask the person which.",
                names.join(", ")
            )
            .into())
        }
    }
}

/// Has the conversations of the dispatcher `caller` told when the chat `thread_id` finishes or
/// asks something, and says whether there was one to tell. Other chats have no one told.
pub(super) fn watch(core: &Core, caller: &Thread, thread_id: &str) -> bool {
    if !core.is_dispatcher(&caller.id) {
        return false;
    }
    let chats = core.bound_chats(&caller.id);
    for b in &chats {
        core.watch_chat(thread_id, b.platform, &b.chat);
    }
    !chats.is_empty()
}
