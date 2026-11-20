//! Running a capability: its arguments are checked against what it takes, the access mode
//! decides whether the person is asked first, and what it came to is written to the log.
//!
//! This is the only way from a chat app's agent into Patchwork: the dispatcher's tools are the
//! capabilities in `table`.

// Rust guideline compliant 2026-10-09

use crate::system::Locked;
use std::collections::HashSet;
use std::sync::Arc;

use serde_json::{Map, Value, json};

use super::table::all;
use super::types::{Capability, Ctx, Kind, Permission};
use crate::threads::{AccessMode, Role, Thread};
use crate::{Core, Error};

/// The most characters of an argument the person is shown when asked to allow a capability.
const SHOWN: usize = 200;

/// What a dispatcher's chat had Patchwork do in the turn it is in, so a call that arrives twice
/// runs once.
#[derive(Debug, Default)]
pub(crate) struct Ran {
    /// The message the turn answers.
    turn: String,
    /// The calls running now.
    running: HashSet<String>,
    /// The last call that changed something and what it returned, until another call follows.
    last: Option<(String, String)>,
}

/// How a call stands against what its chat already had Patchwork do.
enum Standing {
    /// It has not run; it is noted as running.
    New,
    /// The same call is running now.
    Running,
    /// The same call just ran and returned this.
    Done(String),
}

/// The message the caller's turn answers.
fn turn(caller: &Thread) -> String {
    caller
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User && !m.queued)
        .map(|m| m.id.clone())
        .unwrap_or_default()
}

/// Notes the call `key` as running for `ctx`, unless the same call runs now or was the last to
/// run in this turn.
fn begin(ctx: &Ctx<'_>, key: &str) -> Standing {
    let runner = &ctx.core.runner;
    let mut all = runner.ran.locked();
    let ran = all.entry(ctx.caller.id.clone()).or_default();
    let turn = turn(ctx.caller);
    if ran.turn != turn {
        *ran = Ran {
            turn,
            ..Ran::default()
        };
    }
    if ran.running.contains(key) {
        return Standing::Running;
    }
    if let Some((last, result)) = &ran.last
        && last == key
    {
        return Standing::Done(result.clone());
    }
    ran.running.insert(key.to_string());
    Standing::New
}

/// Notes that the call `key` ended. `kept` is what it returned when it changed something, which
/// answers the same call if it comes next.
fn end(ctx: &Ctx<'_>, key: &str, kept: Option<&str>) {
    let runner = &ctx.core.runner;
    let mut all = runner.ran.locked();
    if let Some(ran) = all.get_mut(&ctx.caller.id) {
        ran.running.remove(key);
        ran.last = kept.map(|result| (key.to_string(), result.to_string()));
    }
}

/// The capability called `id`.
fn find(id: &str) -> Option<&'static Capability> {
    all().find(|c| c.id == id)
}

/// The capabilities as Patchwork's own server lists them to a dispatcher, one tool each.
pub(crate) fn tools() -> Vec<Value> {
    all()
        .map(|c| {
            let properties: Map<String, Value> = c
                .args
                .iter()
                .map(|a| {
                    let mut schema = json!({"description": a.about});
                    match a.kind {
                        Kind::Text => schema["type"] = json!("string"),
                        Kind::Flag => schema["type"] = json!("boolean"),
                        Kind::Json => {}
                    }
                    (a.name.to_string(), schema)
                })
                .collect();
            let required: Vec<&str> = c
                .args
                .iter()
                .filter(|a| a.required)
                .map(|a| a.name)
                .collect();
            json!({
                "name": c.id,
                "description": c.about,
                "inputSchema": {
                    "type": "object",
                    "properties": properties,
                    "required": required,
                    "additionalProperties": false,
                },
            })
        })
        .collect()
}

/// Every capability as `list_capabilities` tells it: what it is, what it takes and gives back,
/// and what it needs before it runs.
pub(super) fn describe(_: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let area = args["area"].as_str().unwrap_or_default();
    let listed: Vec<Value> = all()
        .filter(|c| area.is_empty() || c.area == area)
        .map(|c| {
            let args: Vec<Value> = c
                .args
                .iter()
                .map(|a| {
                    json!({
                        "name": a.name,
                        "type": match a.kind {
                            Kind::Text => "text",
                            Kind::Flag => "on or off",
                            Kind::Json => "json",
                        },
                        "required": a.required,
                        "about": a.about,
                    })
                })
                .collect();
            json!({
                "id": c.id,
                "application": "patchwork",
                "area": c.area,
                "about": c.about,
                "arguments": args,
                "returns": c.returns,
                "resources": c.resources,
                "permission": c.permission.name(),
                "asks_first": match c.permission {
                    Permission::Read | Permission::Act => "never",
                    Permission::Change => "in supervised access",
                    Permission::Confirm => "always",
                },
                "reversible": c.reversible,
                "runs_on_after_returning": c.asynchronous,
                "events": c.events,
            })
        })
        .collect();
    if listed.is_empty() {
        let mut areas: Vec<&str> = all().map(|c| c.area).collect();
        areas.sort_unstable();
        areas.dedup();
        return Err(format!(
            "No area is called that. The areas are: {}",
            areas.join(", ")
        )
        .into());
    }
    Ok(Value::Array(listed).to_string())
}

/// Checks `args` against what `capability` takes: an object with only its arguments, each of
/// its type, and every required one given.
fn check(capability: &Capability, args: &Value) -> Result<(), Error> {
    let empty = Map::new();
    let given = match args {
        Value::Null => &empty,
        Value::Object(given) => given,
        _ => return Err("The arguments must be an object".into()),
    };
    for (name, value) in given {
        let Some(arg) = capability.args.iter().find(|a| a.name == name) else {
            let names: Vec<&str> = capability.args.iter().map(|a| a.name).collect();
            return Err(format!(
                "{} takes no argument {name}. It takes: {}",
                capability.id,
                names.join(", ")
            )
            .into());
        };
        let fits = match arg.kind {
            Kind::Text => value.is_string() || value.is_null(),
            Kind::Flag => value.is_boolean() || value.is_null(),
            Kind::Json => true,
        };
        if !fits {
            return Err(format!(
                "The argument {name} must be {}",
                if arg.kind == Kind::Flag {
                    "true or false"
                } else {
                    "text"
                }
            )
            .into());
        }
    }
    for arg in capability.args.iter().filter(|a| a.required) {
        let missing = match given.get(arg.name) {
            None | Some(Value::Null) => true,
            Some(Value::String(text)) => text.trim().is_empty(),
            Some(_) => false,
        };
        if missing {
            return Err(format!("Give the {}", arg.name).into());
        }
    }
    Ok(())
}

/// The arguments as the person is shown them when asked to allow a capability.
fn shown(args: &Value) -> String {
    let Some(given) = args.as_object() else {
        return String::new();
    };
    given
        .iter()
        .map(|(name, value)| {
            let text = match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            let clipped: String = text.chars().take(SHOWN).collect();
            let more = if text.chars().count() > SHOWN {
                "…"
            } else {
                ""
            };
            format!("{name}: {clipped}{more}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Asks the person before `capability` runs, when its permission and the caller's access mode
/// call for it.
fn permit(ctx: &Ctx<'_>, capability: &Capability, args: &Value) -> Result<(), Error> {
    let asks = match capability.permission {
        Permission::Read | Permission::Act => false,
        Permission::Change => ctx.caller.access_mode == AccessMode::Supervised,
        Permission::Confirm => true,
    };
    if !asks {
        return Ok(());
    }
    ctx.confirm(&format!("Patchwork: {}", capability.id), &shown(args))
}

/// Runs the capability `id` for `ctx` and returns what to tell the caller.
///
/// # Errors
///
/// Returns an error when no capability has that name, the arguments do not fit it, the person
/// declines it, or it fails. The error says what went wrong, as the operation under it did.
pub(crate) fn invoke(ctx: &Ctx<'_>, id: &str, args: &Value) -> Result<String, Error> {
    let capability = find(id).ok_or_else(|| {
        Error::from(format!(
            "Patchwork has no capability {id}; list_capabilities lists them"
        ))
    })?;
    let from = ctx.caller.id.as_str();
    let key = format!("{id} {args}");
    // Reading twice is harmless, so only a call that changes something is held to once.
    let guarded = capability.permission != Permission::Read;
    match if guarded {
        begin(ctx, &key)
    } else {
        Standing::New
    } {
        Standing::New => {}
        Standing::Running => {
            log::warn!(capability = id, from; "chat app asked for {{capability}} twice at once");
            return Err(format!(
                "{id} is already running with the same arguments. It was not started a second \
                 time; wait for its result."
            )
            .into());
        }
        Standing::Done(result) => {
            log::warn!(capability = id, from; "chat app asked for {{capability}} twice in a row");
            return Ok(format!(
                "{id} already ran with the same arguments just before this call, so it was not \
                 run a second time. It returned: {result}"
            ));
        }
    }
    let result = check(capability, args)
        .and_then(|()| permit(ctx, capability, args))
        .and_then(|()| (capability.run)(ctx, args));
    // A call that failed did nothing to repeat, and any call ends the last one's hold.
    let kept = result.as_ref().ok().filter(|_| guarded);
    end(ctx, &key, kept.map(String::as_str));
    // What was asked for from a chat app, and what came of it. Arguments stay out of the log:
    // they hold prompts and settings.
    match &result {
        Ok(_) => log::info!(
            capability = id, from, permission = capability.permission.name();
            "chat app ran {{capability}}"
        ),
        Err(error) => log::warn!(
            capability = id, from, permission = capability.permission.name(), error:% = error;
            "chat app could not run {{capability}}: {{error}}"
        ),
    }
    ctx.core
        .note_capability(from, id, capability.permission.name(), result.is_ok());
    result
}

impl Core {
    /// Runs the capability `id` for the dispatcher's chat `thread_id`, as its tool call.
    ///
    /// # Errors
    ///
    /// Returns an error when the chat is not a dispatcher, or as `invoke` does.
    pub(crate) fn run_capability(
        self: &Arc<Self>,
        thread_id: &str,
        id: &str,
        args: &Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        let caller = self
            .store
            .thread(thread_id)
            .filter(|t| self.is_dispatcher(&t.id))
            .ok_or("Only a chat app's dispatcher can do that")?;
        let ctx = Ctx {
            core: self,
            caller: &caller,
            stopped,
        };
        invoke(&ctx, id, args)
    }
}
