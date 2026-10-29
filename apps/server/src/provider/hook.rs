//! Permission hooks. Codex and Antigravity decide on their own what to ask about, and in some
//! modes ask about nothing, so before each tool call they run Patchwork's binary as a hook
//! (`--permission-hook <agent>`). The hook asks the app, over the relay socket the MCP relay
//! uses, whether the permission rules refuse the call or hold it for the user, and answers the
//! agent: a refusal with its reason, or nothing, which lets the call go on as the mode decides.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock, PoisonError, mpsc};
use std::time::Duration;

use serde_json::{Value, json};

use super::TurnSpec;
use crate::permissions::{Guarded, Target};
use crate::types::{AgentKind, PermissionKind};

/// The argument that starts Patchwork's binary as an agent's permission hook.
pub const HOOK_ARG: &str = "--permission-hook";
/// Where the hook finds the relay and the thread it works for.
const ENV: &str = "PATCHWORK_PERMISSION_HOOK";

/// Environment that tells the hook which thread it works for, for the agents that run one.
pub fn env(agent: AgentKind, thread_id: &str, root: &str) -> Vec<(&'static str, String)> {
    let Some(relay) = super::mcp::relay_file() else {
        return Vec::new();
    };
    if !matches!(agent, AgentKind::Codex | AgentKind::Antigravity) {
        return Vec::new();
    }
    let context = json!({"relay": relay, "thread": thread_id, "root": root});
    vec![(ENV, context.to_string())]
}

/// The hook command for `agent`, with Patchwork's binary quoted for the shell.
fn command(agent: &str) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let quoted = exe.to_string_lossy().replace('\'', r"'\''");
    Some(format!("'{quoted}' {HOOK_ARG} {agent}"))
}

/// The hook command for Codex. On Windows Codex runs hooks in PowerShell, which reads a quoted
/// path as a string rather than a program unless `&` calls it.
fn codex_command() -> Option<String> {
    if !cfg!(windows) {
        return command("codex");
    }
    let exe = std::env::current_exe().ok()?;
    let quoted = exe.to_string_lossy().replace('\'', "''");
    Some(format!("& '{quoted}' {HOOK_ARG} codex"))
}

/// `-c` overrides that run the hook before Codex's tools the rules guard: the shell, file
/// patches, and MCP tools. Codex only runs a hook it trusts, so the hook's hash is trusted too.
pub fn codex_args(spec: &TurnSpec) -> Vec<String> {
    let tools: Vec<&str> = spec
        .guarded
        .iter()
        .filter_map(|g: &Guarded| match g.kind {
            PermissionKind::Command => Some("Bash"),
            PermissionKind::Edit => Some("apply_patch"),
            PermissionKind::Mcp => Some("mcp__.*"),
            PermissionKind::Folder | PermissionKind::Web | PermissionKind::Screen => None,
        })
        .collect();
    if tools.is_empty() || super::mcp::relay_file().is_none() {
        return Vec::new();
    }
    let Some(command) = codex_command() else {
        return Vec::new();
    };
    // The user may take a while to answer a request the hook holds.
    let hooks = format!(
        "hooks.PreToolUse=[{{matcher={},hooks=[{{type=\"command\",timeout=86400,command={}}}]}}]",
        json!(format!("^({})$", tools.join("|"))),
        json!(command),
    );
    match codex_trust(&hooks, spec.root) {
        Some(trust) => vec!["-c".into(), hooks, "-c".into(), trust],
        None => {
            log::warn!(
                "Codex did not list Patchwork's permission hook, so deny rules are not enforced"
            );
            Vec::new()
        }
    }
}

/// The `-c` override that trusts the hook `hooks` adds, as Codex lists it, by hook config.
fn codex_trust(hooks: &str, root: &str) -> Option<String> {
    static TRUSTED: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let cache = TRUSTED.get_or_init(Mutex::default);
    if let Some(trust) = cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(hooks)
    {
        return Some(trust.clone());
    }
    let (key, hash) = list_codex_hook(hooks, root)?;
    let trust = format!(
        "hooks.state={{{}={{trusted_hash={}}}}}",
        json!(key),
        json!(hash)
    );
    cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(hooks.into(), trust.clone());
    Some(trust)
}

/// The key and hash Codex gives the hook `hooks` adds, read from `hooks/list`.
fn list_codex_hook(hooks: &str, root: &str) -> Option<(String, String)> {
    let mut cmd = super::turn_command(AgentKind::Codex).ok()?;
    let mut child = cmd
        .args(["app-server", "-c", hooks])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let requests = [
        json!({"id": 1, "method": "initialize", "params": {
            "clientInfo": {"name": "patchwork", "title": "Patchwork", "version": crate::VERSION},
        }}),
        json!({"method": "initialized"}),
        json!({"id": 2, "method": "hooks/list", "params": {"cwds": [root]}}),
    ];
    for request in requests {
        writeln!(stdin, "{request}").ok()?;
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if v["id"] == 2 {
                let _ = tx.send(v);
                return;
            }
        }
    });
    let listed = rx.recv_timeout(Duration::from_secs(30)).ok();
    let _ = child.kill();
    let _ = child.wait();
    listed?["result"]["data"]
        .as_array()?
        .iter()
        .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
        .find(|h| h["source"] == "sessionFlags" && h["eventName"] == "preToolUse")
        .and_then(|h| Some((h["key"].as_str()?.into(), h["currentHash"].as_str()?.into())))
}

/// The hook command for Antigravity. `exec` is for a POSIX shell; on Windows, where the shell
/// may be cmd, PowerShell, or Git Bash, a path with forward slashes and no spaces runs in each.
fn antigravity_command() -> Option<String> {
    if cfg!(windows) {
        let exe = std::env::current_exe().ok()?;
        let path = exe.to_string_lossy().replace('\\', "/");
        if !path.contains(' ') {
            return Some(format!("{path} {HOOK_ARG} antigravity"));
        }
    }
    Some(format!("exec {}", command("antigravity")?))
}

/// Writes the hooks file Antigravity loads from a folder added with `--add-dir`, and returns
/// that folder. The file stays the same, so it is written once.
pub fn antigravity_dir() -> Option<std::path::PathBuf> {
    let relay = super::mcp::relay_file()?;
    let dir = relay.parent()?.join("antigravity-hook");
    let hooks = json!({"hooks": {"PreToolUse": [{
        "matcher": "*",
        "hooks": [{"type": "command", "command": antigravity_command()?, "timeout": 86400}],
    }]}});
    let file = dir.join(".agents").join("hooks.json");
    let text = serde_json::to_string_pretty(&hooks).ok()?;
    if std::fs::read_to_string(&file).ok().as_deref() != Some(text.as_str()) {
        std::fs::create_dir_all(file.parent()?).ok()?;
        std::fs::write(&file, text).ok()?;
    }
    Some(dir)
}

/// Runs the hook: reads the agent's tool call on stdin and writes its answer to stdout.
pub fn run() -> i32 {
    let agent = std::env::args().nth(2).unwrap_or_default();
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let call: Value = serde_json::from_str(&input).unwrap_or_default();
    let request = match agent.as_str() {
        "codex" => codex_call(&call),
        _ => antigravity_call(&call),
    };
    let refusal = match request {
        Some((title, detail, target)) => ask(&title, &detail, target).unwrap_or_else(Some),
        None => None,
    };
    let answer = match (agent.as_str(), refusal) {
        ("codex", Some(reason)) => json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }})
        .to_string(),
        // Codex goes on when the hook says nothing.
        ("codex", None) => String::new(),
        (_, Some(reason)) => json!({"decision": "deny", "reason": reason}).to_string(),
        (_, None) => json!({"decision": "allow"}).to_string(),
    };
    let mut stdout = std::io::stdout();
    let _ = stdout
        .write_all(answer.as_bytes())
        .and_then(|_| stdout.flush());
    0
}

/// Asks Patchwork about a call; the answer is why it is refused, or None to let it run.
fn ask(title: &str, detail: &str, target: Target) -> Result<Option<String>, String> {
    const UNREACHABLE: &str = "Patchwork could not check this call against its permission rules";
    let context: Value = std::env::var(ENV)
        .ok()
        .and_then(|v| serde_json::from_str(&v).ok())
        .ok_or(UNREACHABLE)?;
    let relay: Value = context["relay"]
        .as_str()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or(UNREACHABLE)?;
    let port =
        u16::try_from(relay["port"].as_u64().unwrap_or_default()).map_err(|_| UNREACHABLE)?;
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).map_err(|_| UNREACHABLE)?;
    let hello = json!({"token": relay["token"], "hook": {
        "thread": context["thread"], "root": context["root"],
        "title": title, "detail": detail, "kind": target.kind, "resources": target.resources,
    }});
    writeln!(stream, "{hello}").map_err(|_| UNREACHABLE)?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|_| UNREACHABLE)?;
    let answer: Value = serde_json::from_str(&line).map_err(|_| UNREACHABLE)?;
    Ok(answer["deny"].as_str().map(String::from))
}

/// A Codex hook's tool call as a request for the rules: `tool_name` and `tool_input`.
fn codex_call(call: &Value) -> Option<(String, String, Target)> {
    let tool = call["tool_name"].as_str()?;
    let input = &call["tool_input"];
    match tool {
        "Bash" => {
            let command = match &input["command"] {
                Value::Array(words) => words
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
                command => command.as_str()?.to_string(),
            };
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

/// An MCP tool named `mcp__<server>__<tool>`.
fn mcp_call(tool: &str, input: &Value) -> Option<(String, String, Target)> {
    let (server, name) = tool.strip_prefix("mcp__")?.split_once("__")?;
    let target = Target::new(PermissionKind::Mcp, [format!("{server}/{name}")]);
    Some((
        format!("Use {name} from {server}"),
        super::one_line(&input.to_string()),
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
            "path",
            "file_path",
            "filePath",
            "DirectoryPath",
            "SearchPath",
        ])
    };
    match tool {
        "run_command" | "execute_command" | "shell" => {
            let command = arg(&["CommandLine", "command", "cmd"])?;
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
                    .map_or(path.clone(), |p| p.to_string_lossy().into_owned())
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
