//! Permission hooks.
//!
//! Codex and Antigravity decide on their own what to ask about, and in some modes ask about
//! nothing, so before each tool call they run Patchwork's binary as a hook (`--permission-hook
//! <agent>`). The hook asks the app, over the relay socket the MCP relay uses, whether the
//! permission rules refuse the call or hold it for the user, and answers the agent: a refusal with
//! its reason, or nothing, which lets the call go on as the mode decides.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::target::Target;
use patchwork_model::permissions::PermissionKind;
use patchwork_model::provider::AgentKind;

/// The argument that starts Patchwork's binary as an agent's permission hook.
pub const HOOK_ARG: &str = "--permission-hook";
/// Where the hook finds the relay and the chat it works for.
pub const ENV: &str = "PATCHWORK_PERMISSION_HOOK";

/// Runs the hook: reads the agent's tool call on stdin and writes its answer to stdout.
#[must_use]
pub fn run() -> i32 {
    let mut input = String::new();
    let agent = match std::env::args().nth(2).as_deref() {
        Some("codex") => AgentKind::Codex,
        Some("claude") => AgentKind::Claude,
        _ => AgentKind::Antigravity,
    };
    let _ = std::io::stdin().read_to_string(&mut input);
    let call: Value = serde_json::from_str(&input).unwrap_or_default();
    let request = match agent {
        AgentKind::Codex => codex_call(&call),
        // A rule resolves the paths a call names by the folder the turn works in.
        AgentKind::Claude => claude_call(&call, &root_of_env()),
        _ => antigravity_call(&call),
    };
    let refusal = match request {
        Some((title, detail, target)) => {
            ask(&title, &detail, &target).unwrap_or_else(|e| Some(e.to_string()))
        }
        None => None,
    };
    let answer = crate::permission_hook::answer(agent, refusal);
    let mut stdout = std::io::stdout();
    let _ = stdout
        .write_all(answer.as_bytes())
        .and_then(|()| stdout.flush());
    0
}

/// Asks Patchwork about a call; the answer is why it is refused, or None to let it run.
fn ask(title: &str, detail: &str, target: &Target) -> Result<Option<String>, Error> {
    const UNREACHABLE: &str = "Patchwork's permission relay did not answer, so this call was not \
                               checked against its rules";
    let context: Value = match std::env::var(ENV)
        .ok()
        .and_then(|v| serde_json::from_str(&v).ok())
    {
        Some(context) => context,
        // No context: Patchwork did not set this process up to run a hook, so there are no
        // rules to check the call against. Refusing every call instead would end the turn.
        None => return Ok(None),
    };
    // A turn names the relay outright; one started before this version names only its file.
    let relay = match context["port"].as_u64() {
        Some(_) => context.clone(),
        None => context["relay"]
            .as_str()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .ok_or(UNREACHABLE)?,
    };
    let port = u16::try_from(relay["port"].as_u64().unwrap_or_default())
        .map_err(|_discarded| UNREACHABLE)?;
    let mut stream = connect(port).map_err(|_discarded| UNREACHABLE)?;
    let hello = json!({"token": relay["token"], "hook": {
        "chat": context["chat"], "root": context["root"],
        "title": title, "detail": detail, "kind": target.kind, "resources": target.resources,
    }});
    writeln!(stream, "{hello}").map_err(|_| UNREACHABLE)?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|_discarded| UNREACHABLE)?;
    let answer: Value = serde_json::from_str(&line).map_err(|_discarded| UNREACHABLE)?;
    Ok(answer["deny"].as_str().map(String::from))
}

/// Connects to the relay, trying once more after a moment: it accepts one client at a time here,
/// and may be busy answering another call.
fn connect(port: u16) -> std::io::Result<std::net::TcpStream> {
    let mut attempt = 0;
    loop {
        match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Err(e) if attempt == 0 && e.kind() == std::io::ErrorKind::ConnectionRefused => {
                attempt += 1;
                // The app may still be binding its port; one short wait and a retry covers that.
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            result => return result,
        }
    }
}

/// A Codex hook's tool call as a request for the rules: `tool_name` and `tool_input`.
fn codex_call(call: &Value) -> Option<(String, String, Target)> {
    let tool = call["tool_name"].as_str()?;
    let input = &call["tool_input"];
    match tool {
        "Bash" => {
            let command = crate::permission_hook::shell_command(call)?;
            let target = Target::new(PermissionKind::Command, [command.clone()]);
            Some(("Run command".into(), command, target))
        }
        "apply_patch" => {
            let patch = input["command"]
                .as_str()
                .or(input["patch"].as_str())
                .or(input["input"].as_str())?;
            let paths: Vec<&str> = patch
                .lines()
                .filter_map(|line| {
                    [
                        "*** Add File: ",
                        "*** Update File: ",
                        "*** Delete File: ",
                        "*** Move to: ",
                    ]
                    .iter()
                    .find_map(|prefix| line.strip_prefix(prefix))
                })
                .map(str::trim)
                .collect();
            Some((
                "Edit files".into(),
                paths.join(", "),
                Target::new(PermissionKind::Edit, paths),
            ))
        }
        _ => mcp_call(tool, input),
    }
}

/// The folder the turn works in, from the hook's environment; empty when there is none.
fn root_of_env() -> String {
    std::env::var(ENV)
        .ok()
        .and_then(|v| serde_json::from_str::<Value>(&v).ok())
        .and_then(|context| context["root"].as_str().map(String::from))
        .unwrap_or_default()
}

/// A Claude Code hook's tool call as a request for the rules: `tool_name` and `tool_input`, the
/// same call Claude would ask about itself, mapped by the same table as `claude_target`. None for
/// a tool no rule can answer, such as `TodoWrite`.
fn claude_call(call: &Value, root: &str) -> Option<(String, String, Target)> {
    let tool = call["tool_name"].as_str()?;
    let input = &call["tool_input"];
    let target = crate::target::claude_target(tool, input, root)?;
    let text = |key: &str| input[key].as_str().unwrap_or_default().to_string();
    let (title, detail) = match target.kind {
        PermissionKind::Command => ("Run command".to_string(), text("command")),
        PermissionKind::Edit => ("Edit files".to_string(), target.resources.join(", ")),
        PermissionKind::Folder => (
            "Read outside the project".to_string(),
            target.resources.join(", "),
        ),
        PermissionKind::Web => ("Fetch a web page".to_string(), text("url")),
        PermissionKind::Mcp => (
            format!("Use {}", tool.strip_prefix("mcp__").unwrap_or(tool)),
            patchwork_provider_core::one_line(&input.to_string()),
        ),
        PermissionKind::Screen => ("Use the screen".to_string(), tool.to_string()),
    };
    Some((title, detail, target))
}

/// An MCP tool named `mcp__<server>__<tool>`.
fn mcp_call(tool: &str, input: &Value) -> Option<(String, String, Target)> {
    let (server, name) = tool.strip_prefix("mcp__")?.split_once("__")?;
    let target = Target::new(PermissionKind::Mcp, [format!("{server}/{name}")]);
    Some((
        format!("Use {name} from {server}"),
        patchwork_provider_core::one_line(&input.to_string()),
        target,
    ))
}

/// An Antigravity hook's tool call as a request for the rules: `toolCall` with its `name` and
/// `args`. Argument names differ between tools and versions, so each is looked up by its usual
/// names.
fn antigravity_call(call: &Value) -> Option<(String, String, Target)> {
    let tool = call["toolCall"]["name"].as_str()?;
    let args = &call["toolCall"]["args"];
    let arg = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| args[*k].as_str())
            .map(String::from)
    };
    let path = || {
        arg(&[
            "TargetFile",
            "AbsolutePath",
            "Path",
            "path",
            "file_path",
            "filePath",
            "DirectoryPath",
            "SearchPath",
        ])
    };
    match tool {
        "run_command" | "execute_command" | "shell" => {
            let command = arg(&["CommandLine", "command_line", "command", "cmd"])?;
            let target = Target::new(PermissionKind::Command, [command.clone()]);
            Some(("Run command".into(), command, target))
        }
        "write_file"
        | "write_to_file"
        | "edit_file"
        | "replace_file_content"
        | "multi_replace_file_content"
        | "create_file"
        | "delete_file" => {
            let path = path()?;
            Some((
                "Edit files".into(),
                path.clone(),
                Target::new(PermissionKind::Edit, [path]),
            ))
        }
        "read_file" | "view_file" | "list_dir" | "grep_search" | "find_by_name" => {
            let path = path()?;
            // Reading a file asks for its folder.
            let folder = if tool.contains("file") {
                Path::new(&path)
                    .parent()
                    .map_or_else(|| path.clone(), |p| p.to_string_lossy().into_owned())
            } else {
                path.clone()
            };
            Some((
                "Read outside the project".into(),
                path,
                Target::new(PermissionKind::Folder, [folder]),
            ))
        }
        "read_url" | "read_url_content" | "execute_url" | "fetch_url" => {
            let url = arg(&["Url", "url", "URL"])?;
            Some((
                "Fetch a web page".into(),
                url.clone(),
                Target::new(PermissionKind::Web, [url]),
            ))
        }
        _ => mcp_call(tool, args),
    }
}

/// Extracts the shell command from an agent's tool-call payload.
///
/// The shell command a tool call runs, as each agent gives it to its hooks: Claude Code, Codex and
/// Patchwork's own agent in `tool_input.command` (Codex at times as its words), and Antigravity in
/// `toolCall.args`. None for a call that runs no command.
pub fn shell_command(call: &Value) -> Option<String> {
    let input = &call["tool_input"]["command"];
    if let Some(command) = input.as_str() {
        return Some(command.to_string());
    }
    // Codex's words are quoted as the shell would read them back, so a `-c` script stays one word.
    if let Some(words) = input.as_array() {
        let words: Vec<String> = words
            .iter()
            .filter_map(Value::as_str)
            .map(|word| format!("'{}'", word.replace('\'', r"'\''")))
            .collect();
        return Some(words.join(" "));
    }
    let args = &call["toolCall"]["args"];
    ["CommandLine", "command_line", "command", "cmd"]
        .iter()
        .find_map(|key| args[*key].as_str())
        .map(String::from)
}

/// What a `preToolUse` hook prints for `agent`: a refusal with its reason, or the answer that lets
/// the call go on as the agent's mode decides.
#[must_use]
pub fn answer(agent: AgentKind, refusal: Option<String>) -> String {
    match (agent, refusal) {
        (AgentKind::Antigravity, Some(reason)) => {
            json!({"decision": "deny", "reason": reason}).to_string()
        }
        (AgentKind::Antigravity, None) => json!({"decision": "allow"}).to_string(),
        (_, Some(reason)) => json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }})
        .to_string(),
        // Claude Code and Codex go on when the hook says nothing.
        (_, None) => String::new(),
    }
}
