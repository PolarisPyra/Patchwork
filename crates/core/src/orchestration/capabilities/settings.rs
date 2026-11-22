//! The capabilities that read and change Patchwork's settings and its color schemes.
//!
//! A setting is named by its path in the settings, such as `textSize` or `gateway.access`. What
//! holds secrets is never shown, and what decides which programs run or what agents may do
//! unasked is not changed from a chat app.

// Rust guideline compliant 2026-10-09

use serde_json::{Value, json};

use super::chats::text;
use super::types::Ctx;
use crate::Error;
use crate::events::{AgentEvent, AgentEventEnvelope};
use crate::settings::{Settings, Theme as ThemeSetting};
use crate::themes::{THEME_FORMAT, ThemeFile};

/// The settings that are never shown: they may hold keys and passwords.
const HIDDEN: &[&str] = &["terminal.env", "gateway.platforms"];

/// The settings a chat app cannot change, each with what to do instead.
const LOCKED: &[(&str, &str)] = &[
    (
        "projects",
        "use add_project, rename_project and remove_project",
    ),
    ("onboarded", "it is set by Patchwork"),
    (
        "permissions",
        "the rules for what agents may do are changed in Settings on the computer",
    ),
    (
        "agentPaths",
        "which programs run as agents is changed in Settings on the computer",
    ),
    (
        "skillsDir",
        "where skills are loaded from is changed in Settings on the computer",
    ),
    (
        "hooksDir",
        "where hooks are loaded from is changed in Settings on the computer",
    ),
    (
        "terminal",
        "the terminal's environment is changed in Settings on the computer",
    ),
    (
        "gateway.platforms",
        "chat app accounts are connected in Settings on the computer",
    ),
    ("androidSdk", "it is changed in Settings on the computer"),
    (
        "defaultMachine",
        "it is changed in Settings on the computer",
    ),
    ("themeId", "use apply_theme"),
    ("theme", "use apply_theme"),
];

/// The settings that widen what Patchwork or its agents may do, which the person is asked about
/// in every access mode.
const WIDENING: &[&str] = &[
    "gateway.access",
    "computerUse",
    "androidUse",
    "systemPrompts",
    "unlimitedSteps",
    "turnTokenLimit",
    "integrations",
];

/// Whether `path` is `at` or inside it.
fn within(path: &str, at: &str) -> bool {
    path == at
        || path
            .strip_prefix(at)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// Whether `path` is `at`, inside it, or holds it.
fn touches(path: &str, at: &str) -> bool {
    within(path, at) || within(at, path)
}

/// The value at `path` in `all`, when every part of the path is there.
fn at<'a>(all: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .try_fold(all, |value, key| value.as_object()?.get(key))
}

/// The settings as a chat app may see them, without what holds secrets.
fn visible(settings: &Settings) -> Result<Value, Error> {
    let mut all = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    for hidden in HIDDEN {
        let (parent, key) = hidden.rsplit_once('.').unwrap_or(("", hidden));
        let holder = if parent.is_empty() {
            Some(&mut all)
        } else {
            parent
                .split('.')
                .try_fold(&mut all, |value, key| value.as_object_mut()?.get_mut(key))
        };
        if let Some(holder) = holder.and_then(Value::as_object_mut) {
            holder.remove(key);
        }
    }
    Ok(all)
}

/// Tells the window that the settings changed, so it draws them.
fn changed(ctx: &Ctx<'_>) {
    ctx.core.emit(AgentEventEnvelope {
        chat_id: String::new(),
        event: AgentEvent::SettingsChanged,
    });
}

/// Reads the settings, or the one at a path.
pub(super) fn get(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let all = visible(&ctx.core.settings())?;
    let path = text(args, "path");
    if path.is_empty() {
        return Ok(all.to_string());
    }
    if HIDDEN.iter().any(|hidden| within(path, hidden)) {
        return Err("That setting may hold secrets, so it is not shown".into());
    }
    if let Some(value) = at(&all, path) {
        return Ok(json!({"setting": path, "value": value}).to_string());
    }
    let names: Vec<&str> = all
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    Err(format!(
        "Patchwork has no setting {path}. The settings are: {}",
        names.join(", ")
    )
    .into())
}

/// Changes one setting, checks that Patchwork took it, and saves it.
pub(super) fn set(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let core = ctx.core;
    let path = text(args, "path");
    let value = &args["value"];
    if let Some((_, instead)) = LOCKED.iter().find(|(locked, _)| touches(path, locked)) {
        return Err(format!("{path} cannot be changed from a chat app: {instead}").into());
    }
    let settings = core.settings();
    let mut all = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
    let was = at(&all, path).cloned();
    let (parent, key) = path.rsplit_once('.').unwrap_or(("", path));
    let holder = if parent.is_empty() {
        Some(&mut all)
    } else {
        parent
            .split('.')
            .try_fold(&mut all, |value, key| value.as_object_mut()?.get_mut(key))
    };
    let Some(holder) = holder.and_then(Value::as_object_mut) else {
        return Err(format!("Patchwork has no setting {path}; get_settings shows them").into());
    };
    if parent.is_empty() && !holder.contains_key(key) {
        return Err(format!("Patchwork has no setting {path}; get_settings shows them").into());
    }
    if ["gateway.projectId", "activeProjectId"].contains(&path)
        && let Some(id) = value.as_str()
        && !settings.projects.iter().any(|p| p.id == id)
    {
        return Err("No project has that id; list_projects names them".into());
    }
    if WIDENING.iter().any(|wide| touches(path, wide)) {
        ctx.escalate("Patchwork: change a setting", &format!("{path} to {value}"))?;
    }
    holder.insert(key.to_string(), value.clone());
    let next: Settings =
        serde_json::from_value(all).map_err(|e| format!("That value does not fit {path}: {e}"))?;
    // A value Patchwork does not keep as given was not a setting, or not one of its values.
    let kept = serde_json::to_value(&next).map_err(|e| e.to_string())?;
    if at(&kept, path).unwrap_or(&Value::Null) != value {
        return Err(format!(
            "Patchwork does not keep {value} at {path}; get_settings shows what it holds"
        )
        .into());
    }
    core.save_settings(next)?;
    changed(ctx);
    Ok(json!({"setting": path, "was": was, "now": value}).to_string())
}

/// The color schemes, and which one is drawn.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn list_themes(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let settings = ctx.core.settings();
    let themes: Vec<Value> = ctx
        .core
        .list_themes()
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "name": t.file.name,
                "scheme": t.file.scheme,
                "yours": t.custom,
                "applied": settings.theme_id.as_deref() == Some(&t.id),
                "notes": t.notes,
            })
        })
        .collect();
    Ok(json!({
        "themes": themes,
        "built_in": ["light", "dark", "oled", "system"],
        "applied_built_in": settings.theme_id.is_none().then_some(settings.theme),
    })
    .to_string())
}

/// One color scheme's file, or the format a new one is written in.
pub(super) fn read_theme(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let id = text(args, "id");
    if id.is_empty() {
        return Ok(THEME_FORMAT.to_string());
    }
    let themes = ctx.core.list_themes();
    let theme = themes
        .iter()
        .find(|t| t.id == id)
        .ok_or("No color scheme has that id; list_themes lists them")?;
    Ok(json!({"id": theme.id, "theme": theme.file, "notes": theme.notes}).to_string())
}

/// Saves a color scheme, new or over an existing one, once its text is readable.
pub(super) fn save_theme(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let core = ctx.core;
    let file = given_theme(args)?;
    let notes = file.notes();
    if !notes.is_empty() {
        return Err(format!(
            "Not saved, because it would be hard to read: {}. Change the colors and save again",
            notes.join("; ")
        )
        .into());
    }
    let id = match text(args, "id") {
        "" => crate::themes::free_theme_id(&file.name),
        id => id.to_string(),
    };
    core.write_theme(&id, &file)?;
    changed(ctx);
    Ok(json!({"saved": id, "name": file.name, "next": "apply_theme draws it"}).to_string())
}

/// The color scheme `args` holds as `theme`, as JSON or as text of it.
fn given_theme(args: &Value) -> Result<ThemeFile, Error> {
    let given = match &args["theme"] {
        Value::String(json) => {
            serde_json::from_str(json).map_err(|e| format!("The theme is not valid JSON: {e}"))?
        }
        other => other.clone(),
    };
    serde_json::from_value(given).map_err(|e| {
        format!("That is not a color scheme: {e}. read_theme without an id gives the format").into()
    })
}

/// Sends a picture of Patchwork drawn in a color scheme, saved or not, to the conversation.
pub(super) fn preview_theme(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let id = text(args, "id");
    let file = if id.is_empty() {
        if args["theme"].is_null() {
            return Err("Give the id of a color scheme, or the theme to preview".into());
        }
        given_theme(args)?
    } else {
        ctx.core
            .list_themes()
            .into_iter()
            .find(|t| t.id == id)
            .map(|t| t.file)
            .ok_or(
                "No color scheme has that id; list_themes lists them. The built-in light, dark \
                 and oled have no preview",
            )?
    };
    let picture = file.preview()?;
    let notes = file.notes();
    let caption = format!("{}, as Patchwork would be drawn in it", file.name);
    let took = ctx
        .core
        .send_file_to_chats(&ctx.caller.id, "preview.png", &picture, &caption)?;
    Ok(json!({
        "sent": "preview.png",
        "name": file.name,
        "conversations": took,
        "hard_to_read": notes,
        "colors": file.colors,
    })
    .to_string())
}

/// Draws Patchwork in a color scheme.
pub(super) fn apply_theme(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let core = ctx.core;
    let id = text(args, "id");
    let built_in = match id {
        "light" => Some(ThemeSetting::Light),
        "dark" => Some(ThemeSetting::Dark),
        "oled" => Some(ThemeSetting::Oled),
        "system" => Some(ThemeSetting::System),
        _ => None,
    };
    let mut settings = core.settings();
    // What it was drawn in, so the person can ask for it back.
    let was = settings.theme_id.clone().unwrap_or_else(|| {
        serde_json::to_value(settings.theme)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    });
    let name = if let Some(theme) = built_in {
        settings.theme_id = None;
        settings.theme = theme;
        id.to_string()
    } else {
        let themes = core.list_themes();
        let theme = themes
            .iter()
            .find(|t| t.id == id)
            .ok_or("No color scheme has that id; list_themes lists them")?;
        settings.theme_id = Some(theme.id.clone());
        theme.file.name.clone()
    };
    core.save_settings(settings)?;
    changed(ctx);
    Ok(format!(
        "Patchwork is drawn in {name} now. Before, it was {was}; apply_theme with that id brings it back."
    ))
}

/// Deletes a color scheme the person added.
pub(super) fn delete_theme(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let core = ctx.core;
    let id = text(args, "id");
    let themes = core.list_themes();
    let theme = themes
        .iter()
        .find(|t| t.id == id)
        .ok_or("No color scheme has that id; list_themes lists them")?;
    if !theme.custom {
        return Err("That color scheme comes with Patchwork and cannot be deleted".into());
    }
    core.remove_theme(id)?;
    core.update_settings(|s| s.drop_theme(id))?;
    changed(ctx);
    Ok(format!("Deleted the color scheme {}.", theme.file.name))
}
