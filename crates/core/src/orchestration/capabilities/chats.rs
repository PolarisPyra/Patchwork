//! The capabilities that act on chats: the chat tools every chat has, and what a person does
//! from the chat list: stopping, answering, renaming, archiving, deleting and opening one, and
//! changing what it runs with.

// Rust guideline compliant 2026-10-09

use serde_json::{Value, json};

use super::types::Ctx;
use crate::Error;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::gateway::options;
use crate::notifications::{Notices, resets_in};
use crate::system::now_ms;
use crate::threads::{Thread, chat_tools};

/// The most chats offered when a name fits several.
const CANDIDATES: usize = 8;

/// A text argument, trimmed, or empty when it is missing.
pub(super) fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args[key].as_str().map(str::trim).unwrap_or_default()
}

/// Runs one of the chat tools as the dispatcher's own, which checks and words it.
fn chat_tool(ctx: &Ctx<'_>, tool: &str, args: &Value) -> Result<String, Error> {
    ctx.core
        .runner
        .run_chat_tool(ctx.core, &ctx.caller.id, tool, args, ctx.stopped)
}

/// Lists the chats.
pub(super) fn list(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    chat_tool(ctx, chat_tools::LIST, args)
}

/// Reads how far a chat has got.
pub(super) fn read(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    chat_tool(ctx, chat_tools::READ, args)
}

/// Lists the projects.
pub(super) fn projects(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    chat_tool(ctx, chat_tools::PROJECTS, args)
}

/// Starts a chat.
pub(super) fn start(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    chat_tool(ctx, chat_tools::START, args)
}

/// Messages a chat.
pub(super) fn send(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    chat_tool(ctx, chat_tools::SEND, args)
}

/// Waits for a chat's reply.
pub(super) fn wait(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    chat_tool(ctx, chat_tools::WAIT, args)
}

/// Stops a chat.
pub(super) fn stop(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    chat_tool(ctx, chat_tools::STOP, args)
}

/// Passes the person's answer on to a chat.
pub(super) fn answer(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    chat_tool(ctx, chat_tools::ANSWER, args)
}

/// Has the person told when a chat finishes.
pub(super) fn notify(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    chat_tool(ctx, chat_tools::NOTIFY, args)
}

/// The chat `args` names: by `chat_id`, or by `name` in the person's words when one chat fits
/// it best.
///
/// # Errors
///
/// Returns an error when neither is given, no chat fits, or several fit equally; that error
/// lists them with their ids, to pick from or ask about.
pub(super) fn chat(ctx: &Ctx<'_>, args: &Value) -> Result<Thread, Error> {
    let core = ctx.core;
    let own = ctx.caller.id.as_str();
    let id = text(args, "chat_id");
    if !id.is_empty() {
        if id == own {
            return Err("That is this conversation's own chat".into());
        }
        return core
            .store
            .thread(id)
            .filter(|t| t.parent.is_none())
            .ok_or_else(|| "No chat with that id; list_chats lists them".into());
    }
    let name = text(args, "name");
    if name.is_empty() {
        return Err("Give the chat_id, or the name the person called the chat".into());
    }
    let chats: Vec<Thread> = core
        .store
        .threads()
        .into_iter()
        .filter(|t| t.parent.is_none() && t.archived_at.is_none() && t.id != own)
        .collect();
    let found = chat_tools::matching(core, &chats, name);
    match found.as_slice() {
        [] => Err("No chat has a title or project like that; list_chats lists them".into()),
        [only] => Ok((*only).clone()),
        several => {
            let listed: Vec<Value> = several
                .iter()
                .take(CANDIDATES)
                .map(|t| json!({"chat_id": t.id, "title": t.title}))
                .collect();
            Err(format!(
                "Several chats fit that. Ask the person which, then pass its chat_id: {}",
                Value::Array(listed)
            )
            .into())
        }
    }
}

/// Tells the window that what the chat runs with, or its place in the list, changed.
fn changed(ctx: &Ctx<'_>, thread_id: &str) {
    ctx.core.emit(AgentEventEnvelope {
        thread_id: thread_id.to_string(),
        event: AgentEvent::OptionsChanged,
    });
}

/// Changes what a chat runs with: its agent, model and access.
pub(super) fn set_options(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let thread = chat(ctx, args)?;
    let access = text(args, "access");
    if !access.is_empty() {
        // More access is more the chat may do unasked, which only the person grants.
        ctx.escalate(
            "Patchwork: change a chat's access",
            &format!("{} to {access}", thread.title),
        )?;
    }
    let mut any = false;
    // The agent first: a model named with it belongs to the new agent.
    for word in ["agent", "model", "access"] {
        let value = text(args, word);
        if !value.is_empty() {
            let now = ctx.core.store.thread(&thread.id).unwrap_or(thread.clone());
            ctx.core.set_chat_option(&now, word, value)?;
            any = true;
        }
    }
    let now = ctx.core.store.thread(&thread.id).unwrap_or(thread);
    Ok(if any {
        format!("Changed. {}", options::options(&now))
    } else {
        options::options(&now)
    })
}

/// Renames a chat.
pub(super) fn rename(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let thread = chat(ctx, args)?;
    let title = text(args, "title");
    ctx.core.rename_thread(&thread.id, title)?;
    changed(ctx, &thread.id);
    Ok(format!(
        "Renamed \u{201c}{}\u{201d} to \u{201c}{title}\u{201d}.",
        thread.title
    ))
}

/// Archives a chat, or brings it back.
pub(super) fn archive(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let thread = chat(ctx, args)?;
    let archived = args["archived"].as_bool().unwrap_or(true);
    ctx.core.set_thread_archived(&thread.id, archived)?;
    changed(ctx, &thread.id);
    Ok(format!(
        "\u{201c}{}\u{201d} is {}.",
        thread.title,
        if archived {
            "archived"
        } else {
            "back in the chat list"
        }
    ))
}

/// Deletes a chat for good.
pub(super) fn delete(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let thread = chat(ctx, args)?;
    ctx.core.delete_thread(&thread.id);
    if ctx.core.store.thread(&thread.id).is_some() {
        return Err("Patchwork could not delete that chat; its log says why".into());
    }
    ctx.core.emit(AgentEventEnvelope {
        thread_id: thread.id.clone(),
        event: AgentEvent::ThreadDeleted,
    });
    Ok(format!("Deleted \u{201c}{}\u{201d}.", thread.title))
}

/// Opens a chat in Patchwork's window on the computer.
pub(super) fn open(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let thread = chat(ctx, args)?;
    ctx.core.emit(AgentEventEnvelope {
        thread_id: thread.id.clone(),
        event: AgentEvent::OpenChat,
    });
    Ok(format!(
        "Asked Patchwork's window to open \u{201c}{}\u{201d}. It shows there when the window \
         is running.",
        thread.title
    ))
}

/// Sets which of a chat's events the person is told about.
pub(super) fn notices(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let thread = chat(ctx, args)?;
    let wanted = text(args, "notices").to_lowercase();
    let notices = match wanted.as_str() {
        "all" => Notices::All,
        "asks" => Notices::Asks,
        "finished" => Notices::Finished,
        "off" => Notices::Off,
        _ => return Err("Notices are one of: all, asks, finished, off".into()),
    };
    ctx.core.set_thread_notices(&thread.id, Some(notices))?;
    Ok(format!(
        "\u{201c}{}\u{201d} now sends: {wanted}.",
        thread.title
    ))
}

/// Has every muted chat and project send its notices again.
pub(super) fn unmute_all(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let unmuted = ctx.core.unmute_all()?;
    Ok(json!({"unmuted": unmuted}).to_string())
}

/// Sets a chat that stopped at its agent's usage limit to continue when the limit resets.
pub(super) fn resume_at_reset(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let thread = chat(ctx, args)?;
    let at = ctx.core.resume_at_limit_reset(&thread.id)?;
    changed(ctx, &thread.id);
    Ok(json!({
        "chat": thread.title,
        "continues_at_epoch_ms": at,
        "continues": resets_in(at, now_ms()),
    })
    .to_string())
}
