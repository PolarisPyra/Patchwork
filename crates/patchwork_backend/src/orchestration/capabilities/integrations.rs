//! The capabilities that set up what Patchwork works with: its chat apps and who is paired, the
//! agents it runs, and the MCP servers, custom agents and hooks those agents are given.
//!
//! None of them reads or takes a key, a token or a password: those are entered on the computer.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;

use serde_json::{Value, json};

use crate::orchestration::tasks::chats::text;
use super::service::{agent, changed};
use super::types::Ctx;
use crate::Error;
use crate::extensions::hooks;
use crate::gateway::{Need, Platform};
use crate::provider::{AgentKind, detect_agent, mcp};
use crate::system::now_ms;

/// The chat app `args` names.
fn platform(args: &Value) -> Result<Platform, Error> {
    let name = text(args, "platform").to_lowercase();
    Platform::ALL
        .into_iter()
        .find(|p| p.id() == name || p.label().to_lowercase() == name)
        .ok_or_else(|| "No chat app is called that; gateway_status lists them".into())
}

/// Reconnects the chat apps without holding up the caller.
pub(super) fn reconnect(ctx: &Ctx<'_>) -> Result<(), Error> {
    let core = Arc::clone(ctx.core);
    std::thread::Builder::new()
        .name("gateway-restart".into())
        .spawn(move || core.restart_gateway())
        .map(drop)
        .map_err(|e| format!("Patchwork could not start the reconnect: {e}").into())
}

/// Turns a chat app on or off, once its account is set up on the computer.
pub(super) fn set_chat_app(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let platform = platform(args)?;
    let on = args["enabled"].as_bool().unwrap_or(true);
    if on {
        let saved = ctx.core.chat_app_credentials();
        let missing: Vec<&str> = platform
            .credentials()
            .iter()
            .filter(|c| c.need == Need::Required && !saved.contains(&(platform, c.key)))
            .map(|c| c.label)
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "{} has no {} saved. Those are entered in Settings → Chat apps on the computer; \
                 they cannot be given from a chat app",
                platform.label(),
                missing.join(" or ")
            )
            .into());
        }
    }
    // Saving a switch that moved reconnects the chat apps; one already set is reconnected here.
    let moved = ctx.core.update_settings(|s| {
        let entry = s.gateway.platforms.entry(platform).or_default();
        std::mem::replace(&mut entry.enabled, on) != on
    })?;
    changed(ctx);
    if !moved {
        reconnect(ctx)?;
    }
    Ok(format!(
        "{} is {}. The chat apps are reconnecting; gateway_status shows where each one stands.",
        platform.label(),
        if on { "on" } else { "off" }
    ))
}

/// Makes a one-time code that pairs whoever sends it to one of Patchwork's bots.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn pairing_code(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let (code, expires) = ctx.core.new_pairing_code();
    Ok(json!({
        "code": code,
        "works_for_minutes": expires.saturating_sub(now_ms()) / 60_000,
        "use": "Whoever sends this code to one of Patchwork's bots is paired, once.",
    })
    .to_string())
}

/// Every agent Patchwork can run: whether it is installed, signed in and in use.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn providers(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let settings = ctx.core.settings();
    let agents: Vec<Value> = AgentKind::ALL
        .into_iter()
        .map(|kind| {
            let status = detect_agent(kind);
            json!({
                "agent": kind,
                "installed": status.installed,
                "version": status.version,
                "signed_in": status.signed_in,
                "detail": status.detail,
                "in_use": settings.agents.contains(&kind),
                "usual_model": settings.tokens.default_models.get(&kind),
            })
        })
        .collect();
    Ok(json!({
        "agents": agents,
        "note": "Signing in and API keys are done on the computer, in Settings → Providers.",
    })
    .to_string())
}

/// Downloads and installs an agent's program from its maker.
pub(super) fn install_agent(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let kind = agent(text(args, "agent"))?;
    ctx.core.install_cli(kind)?;
    let status = detect_agent(kind);
    Ok(json!({
        "agent": kind,
        "installed": status.installed,
        "version": status.version,
        "signed_in": status.signed_in,
        "detail": status.detail,
        "next": "Signing in is done on the computer, in Settings → Providers.",
    })
    .to_string())
}

/// The MCP servers agents are given, without what they are started with.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn mcp_servers(_: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let servers: Vec<Value> = mcp::list()
        .into_iter()
        .map(|s| {
            // Arguments, environment and headers stay out: they can hold keys.
            json!({
                "name": s.name,
                "runs": if s.server.url.is_some() { "over the network" } else { "on this computer" },
                "enabled": !s.server.disabled,
                "status": s.status,
                "error": s.error,
                "tools": s.tools.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
                "tools_off": s.server.disabled_tools,
            })
        })
        .collect();
    Ok(Value::Array(servers).to_string())
}

/// The MCP server `args` names, as it is saved.
fn mcp_server(args: &Value) -> Result<mcp::ServerView, Error> {
    let name = text(args, "server").to_lowercase();
    mcp::list()
        .into_iter()
        .find(|s| s.name.to_lowercase() == name)
        .ok_or_else(|| "No MCP server is called that; list_mcp_servers lists them".into())
}

/// Starts an MCP server over.
pub(super) fn restart_mcp(_: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = mcp_server(args)?;
    mcp::restart(&found.name);
    Ok(format!(
        "Restarting {}. list_mcp_servers shows where it stands.",
        found.name
    ))
}

/// Turns an MCP server on or off for every agent.
pub(super) fn set_mcp(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = mcp_server(args)?;
    let on = args["enabled"].as_bool().unwrap_or(true);
    if on {
        // A server that is on gives every agent its tools.
        ctx.escalate("Patchwork: turn an MCP server on", &found.name)?;
    }
    let mut server = found.server;
    server.disabled = !on;
    // The values it is started with are empty here, which keeps the saved ones.
    mcp::save(&found.name, Some(found.name.clone()), server)?;
    Ok(format!(
        "{} is {}.",
        found.name,
        if on { "on" } else { "off" }
    ))
}

/// Removes an MCP server and what it was started with.
pub(super) fn remove_mcp(_: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let found = mcp_server(args)?;
    mcp::remove(&found.name)?;
    Ok(format!("Removed the MCP server {}.", found.name))
}

/// The custom agents a chat can summon.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn custom_agents(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let agents: Vec<Value> = ctx
        .core
        .project_agents(None)
        .into_iter()
        .map(|a| {
            json!({
                "name": a.name,
                "title": a.title,
                "about": a.description,
                "model": a.model,
            })
        })
        .collect();
    Ok(Value::Array(agents).to_string())
}

/// The hook bundles that run on agents' events, and whether each is on.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every capability's handler has one signature"
)]
pub(super) fn list_hooks(ctx: &Ctx<'_>, _: &Value) -> Result<String, Error> {
    let bundles: Vec<Value> = hooks::list(&ctx.core.settings())
        .into_iter()
        .map(|b| {
            json!({
                "id": b.id,
                "name": b.name,
                "version": b.version,
                "about": b.description,
                "hooks": b.hooks.len(),
                "enabled": b.enabled,
            })
        })
        .collect();
    Ok(Value::Array(bundles).to_string())
}

/// Turns a hook bundle on or off.
pub(super) fn set_hook(ctx: &Ctx<'_>, args: &Value) -> Result<String, Error> {
    let wanted = text(args, "hook").to_lowercase();
    let bundle = hooks::list(&ctx.core.settings())
        .into_iter()
        .find(|b| b.id.to_lowercase() == wanted || b.name.to_lowercase() == wanted)
        .ok_or("No hook bundle is called that; list_hooks lists them")?;
    let on = args["enabled"].as_bool().unwrap_or(true);
    if on {
        // A bundle that is on runs its programs on every agent's events.
        ctx.escalate("Patchwork: turn a hook bundle on", &bundle.name)?;
    }
    let id = bundle.id.clone();
    ctx.core.update_settings(|s| {
        s.disabled_hooks.retain(|p| *p != id);
        if !on {
            s.disabled_hooks.push(id);
        }
    })?;
    changed(ctx);
    Ok(format!(
        "{} is {}.",
        bundle.name,
        if on { "on" } else { "off" }
    ))
}
