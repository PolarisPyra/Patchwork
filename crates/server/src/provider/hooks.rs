//! Hooks Patchwork hands to each agent CLI.
//!
//! Handing hooks to each agent CLI: which events each one runs, the command that starts
//! Patchwork's own binary as a hook, where each CLI reads its hooks from, what it puts in a
//! call, and the answer it expects.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::sync::{Mutex, OnceLock, PoisonError, mpsc};
use std::time::Duration;

use serde_json::{Value, json};

use crate::permissions::PermissionKind;
use crate::permissions::hook::HOOK_ARG;
use crate::hooks::{AgentHook, HookAction, HookEvent};
use crate::provider::{AgentKind, TurnSpec};

/// Whether `agent` runs hooks on `event` itself.
///
/// Patchwork runs the turn's own events for every agent; Claude Code and Codex run all the others,
/// Antigravity its session start and tool calls, and Patchwork's own agent its tool calls. OpenCode
/// has no hooks of its own, but asks Patchwork before a call, so Patchwork runs the hooks for
/// before a tool call on that request: see `opencode_call`.
#[must_use]
pub fn runs(agent: AgentKind, event: HookEvent) -> bool {
    if event.runs_in_patchwork() {
        return false;
    }
    match agent {
        AgentKind::Claude | AgentKind::Codex => true,
        AgentKind::Antigravity => matches!(
            event,
            HookEvent::SessionStart | HookEvent::PreToolUse | HookEvent::PostToolUse
        ),
        AgentKind::Api => matches!(event, HookEvent::PreToolUse | HookEvent::PostToolUse),
        AgentKind::OpenCode => event == HookEvent::PreToolUse,
    }
}

/// The tool call an OpenCode permission request stands for, as a hook reads one.
///
/// The tool has the name Claude Code gives it, so one matcher covers every agent: `Bash` with its
/// `command`, `Edit` with its `file_path`, `WebFetch` with its `url`, and an MCP tool under the
/// name OpenCode asks with. None for a request no tool hook answers for, such as another folder.
#[must_use]
pub fn opencode_call(kind: PermissionKind, resource: &str) -> Option<(String, Value)> {
    match kind {
        PermissionKind::Command => Some(("Bash".into(), json!({ "command": resource }))),
        PermissionKind::Edit => Some(("Edit".into(), json!({ "file_path": resource }))),
        PermissionKind::Web => Some(("WebFetch".into(), json!({ "url": resource }))),
        PermissionKind::Mcp => Some((resource.to_string(), json!({}))),
        PermissionKind::Folder | PermissionKind::Screen => None,
    }
}

/// The shell command that runs Patchwork's own binary with `args`.
///
/// The command that starts Patchwork's own binary with `args`, quoted for the shell `agent` runs
/// hooks in: a POSIX shell for most. On Windows Codex runs them in PowerShell, which reads a quoted
/// path as a string rather than a program unless `&` calls it, and Antigravity's shell may be cmd,
/// PowerShell, or Git Bash, where a path with forward slashes and no spaces runs in each; elsewhere
/// Antigravity's `exec` is for a POSIX shell.
#[must_use]
pub fn patchwork_command(agent: AgentKind, args: &[&str]) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.to_string_lossy();
    let args = args.join(" ");
    let posix = || format!("'{}' {args}", path.replace('\'', r"'\''"));
    Some(match agent {
        AgentKind::Codex if cfg!(windows) => format!("& '{}' {args}", path.replace('\'', "''")),
        AgentKind::Antigravity if cfg!(windows) && !path.contains(' ') => {
            format!("{} {args}", path.replace('\\', "/"))
        }
        AgentKind::Antigravity => format!("exec {}", posix()),
        AgentKind::Claude | AgentKind::Codex | AgentKind::OpenCode | AgentKind::Api => posix(),
    })
}

/// The command an agent runs for `hook`.
fn command_line(hook: &AgentHook) -> &str {
    let HookAction::Command { command } = &hook.action;
    command
}

/// The hooks `agent` runs, in the `hooks.json` shape Claude Code and Antigravity read:
/// `{ "hooks": { "PreToolUse": [ { "matcher": …, "hooks": [ … ] } ] } }`.
#[must_use]
pub fn hooks_json(hooks: &[AgentHook], agent: AgentKind) -> Value {
    let mut by_event = serde_json::Map::new();
    for hook in hooks.iter().filter(|hook| runs(agent, hook.event)) {
        let command = command_line(hook);
        let mut group = json!({ "hooks": [{
            "type": "command",
            "command": command,
            "timeout": hook.timeout_secs,
        }] });
        if let Some(matcher) = &hook.matcher {
            group["matcher"] = Value::from(matcher.as_str());
        }
        if let Value::Array(groups) = by_event
            .entry(hook.event.cli_name())
            .or_insert_with(|| Value::Array(Vec::new()))
        {
            groups.push(group);
        }
    }
    json!({ "hooks": by_event })
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
    ["CommandLine", "command", "cmd"]
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

/// Why a turn started with `args` would run without Patchwork's permission hook.
///
/// Without the hook the agent runs without the user's rules, so such a turn is refused rather
/// than run unguarded. None when the hook is in place, and for the agents that ask Patchwork
/// another way.
#[must_use]
pub fn unguarded(agent: AgentKind, args: &[String]) -> Option<&'static str> {
    let relay = crate::provider::mcp::relay_file().is_some()
        && crate::provider::mcp::relay_endpoint().is_some();
    match agent {
        AgentKind::Codex | AgentKind::Antigravity if !relay => Some(
            "Patchwork's permission relay is not running, so this turn would run without your \
             permission rules. Restart Patchwork and send the message again.",
        ),
        AgentKind::Codex if !args.iter().any(|arg| arg.starts_with("hooks.PreToolUse=")) => Some(
            "Codex did not load Patchwork's permission hook, so this turn would run without your \
             permission rules. Update Codex, then send the message again.",
        ),
        _ => None,
    }
}

/// Builds the `-c` overrides that install Codex's permission hook and the user's hooks.
///
/// `-c` overrides with the hooks Codex runs: Patchwork's permission hook before the tools a rule
/// can answer for (the shell, file patches, and MCP tools), and the user's hooks. The permission
/// hook is installed for every turn, guarded or not, because it asks Patchwork on each call: a
/// rule added, changed, or removed while the turn runs then decides the calls that follow, with no
/// relaunch. Codex only runs a hook it trusts, so their hashes are trusted too.
#[must_use]
pub fn codex_args(spec: &TurnSpec) -> Vec<String> {
    // Each event's groups, in the order Codex numbers them.
    let mut events: Vec<(String, Vec<String>)> = Vec::new();
    let mut add = |event: String, group: String| match events.iter_mut().find(|(e, _)| *e == event)
    {
        Some((_, groups)) => groups.push(group),
        None => events.push((event, vec![group])),
    };
    if let Some(group) = permission_group() {
        add("PreToolUse".into(), group);
    } else {
        log::warn!(
            "Patchwork's permission relay is not running, so Codex runs without Patchwork's \
             permission rules"
        );
    }
    for hook in spec
        .hooks
        .iter()
        .filter(|hook| runs(AgentKind::Codex, hook.event))
    {
        let command = command_line(hook);
        let matcher = hook
            .matcher
            .as_ref()
            .map(|m| format!("matcher={},", json!(format!("^(?:{m})$"))))
            .unwrap_or_default();
        add(
            hook.event.cli_name(),
            format!(
                "{{{matcher}hooks=[{{type=\"command\",timeout={},command={}}}]}}",
                hook.timeout_secs,
                json!(command),
            ),
        );
    }
    if events.is_empty() {
        return Vec::new();
    }
    let overrides: Vec<String> = events
        .iter()
        .map(|(event, groups)| format!("hooks.{event}=[{}]", groups.join(",")))
        .collect();
    if let Some(trust) = codex_trust(&overrides, spec.root, spec.host) {
        overrides
            .into_iter()
            .chain(std::iter::once(trust))
            .flat_map(|config| ["-c".to_string(), config])
            .collect()
    } else {
        log::warn!(
            "Codex did not list Patchwork's hooks, so deny rules are not enforced and the \
             user's hooks do not run"
        );
        Vec::new()
    }
}

/// The Codex tool names the permission hook is asked about: the shell, file patches, and MCP
/// tools. A rule of any kind can answer one of these, so the hook runs whatever the rules guard
/// when the turn starts; what it asks Patchwork is decided by the rules in force at the call.
const CODEX_TOOLS: &str = "^(Bash|apply_patch|mcp__.*)$";

/// The hook group that asks Patchwork before a tool call the rules may refuse or hold. It is
/// installed even when no rule guards anything yet, so a rule saved mid-turn applies at once, and
/// None only when there is no relay for the hook to ask.
fn permission_group() -> Option<String> {
    // The turn is handed this relay's port and token; without one the hook could not ask.
    crate::provider::mcp::relay_endpoint()?;
    let command = patchwork_command(AgentKind::Codex, &[HOOK_ARG, "codex"])?;
    // The user may take a while to answer a request the hook holds.
    Some(format!(
        "{{matcher={},hooks=[{{type=\"command\",timeout=86400,command={}}}]}}",
        json!(CODEX_TOOLS),
        json!(command),
    ))
}

/// The `-c` override that trusts every hook `overrides` add, as Codex lists them, by config.
/// For a project on `host`, Codex there lists them, with the paths the hooks have there.
fn codex_trust(overrides: &[String], root: &str, host: Option<&str>) -> Option<String> {
    /// The override for each host (none for this machine) and list of hooks.
    type Trusted = HashMap<(Option<String>, Vec<String>), String>;
    static TRUSTED: OnceLock<Mutex<Trusted>> = OnceLock::new();
    let key = (host.map(str::to_string), overrides.to_vec());
    let cache = TRUSTED.get_or_init(Mutex::default);
    if let Some(trust) = cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
    {
        return Some(trust.clone());
    }
    let listed = list_codex_hooks(overrides, root, host)?;
    if listed.is_empty() {
        return None;
    }
    let entries: Vec<String> = listed
        .iter()
        .map(|(key, hash)| format!("{}={{trusted_hash={}}}", json!(key), json!(hash)))
        .collect();
    let trust = format!("hooks.state={{{}}}", entries.join(","));
    cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, trust.clone());
    Some(trust)
}

/// The key and hash Codex gives each hook `overrides` add, read from `hooks/list`.
fn list_codex_hooks(
    overrides: &[String],
    root: &str,
    host: Option<&str>,
) -> Option<Vec<(String, String)>> {
    let mut cmd = match host {
        Some(_) => crate::provider::remote_turn_command(AgentKind::Codex),
        None => crate::provider::turn_command(AgentKind::Codex),
    }
    .ok()?;
    cmd.arg("app-server");
    for config in overrides {
        cmd.args(["-c", config]);
    }
    if let Some(host) = host {
        cmd.current_dir(crate::system::remote::root(host, root));
        cmd = crate::system::remote::supervised(cmd)
            .map_err(
                |e| log::warn!(error:% = e; "Codex on the host could not list hooks: {{error}}"),
            )
            .ok()?;
    }
    let mut child = cmd
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
        for line in crate::system::lossy_lines(BufReader::new(stdout)) {
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if v["id"] == 2 {
                let _ = tx.send(v);
                return;
            }
        }
    });
    // A CLI that has not listed its hooks by then is not going to; the turn starts without them.
    let listed = rx.recv_timeout(Duration::from_secs(30)).ok();
    let _ = child.kill();
    let _ = child.wait();
    Some(
        listed?["result"]["data"]
            .as_array()?
            .iter()
            .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
            .filter(|h| h["source"] == "sessionFlags")
            .filter_map(|h| Some((h["key"].as_str()?.into(), h["currentHash"].as_str()?.into())))
            .collect(),
    )
}

/// The argument that starts Patchwork's binary as Antigravity's session start hook, which hands it
/// the system prompt from Settings.
pub const PROMPT_ARG: &str = "--system-prompt-hook";
/// Where that hook finds the prompt: the turn's environment, which Antigravity passes on to it.
const PROMPT_ENV: &str = "PATCHWORK_SYSTEM_PROMPT";

/// Environment for an Antigravity turn: the system prompt its session start hook hands over.
#[must_use]
pub fn antigravity_env(spec: &TurnSpec) -> Vec<(&'static str, String)> {
    antigravity_prompt(spec)
        .map(|text| (PROMPT_ENV, text.to_string()))
        .into_iter()
        .collect()
}

fn antigravity_prompt<'a>(spec: &TurnSpec<'a>) -> Option<&'a str> {
    spec.instructions.filter(|text| !text.trim().is_empty())
}

/// Runs the session start hook: answers Antigravity with the system prompt as a system message,
/// which it adds to the conversation before the first model call.
#[must_use]
pub fn run_prompt_hook() -> i32 {
    let answer = match std::env::var(PROMPT_ENV) {
        Ok(text) if !text.trim().is_empty() => {
            json!({"injectSteps": [{"systemMessage": {"systemMessage": text}}]})
        }
        _ => json!({}),
    };
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{answer}");
    let _ = out.flush();
    0
}

/// The folder passed to Antigravity with `--add-dir`, holding its hooks file.
///
/// The folder Antigravity is given with `--add-dir`, or None when there is nothing to put in it:
/// the hooks file it loads from there (the system prompt at session start, Patchwork's permission
/// hook before each tool call, and the user's hooks). Rules such as a `GEMINI.md` in that folder
/// are not read, so the prompt goes through a hook. Each hooks file has a folder of its own, named
/// by its hash, so turns that differ do not share a file.
#[must_use]
pub fn antigravity_dir(spec: &TurnSpec) -> Option<std::path::PathBuf> {
    let hooks = antigravity_hooks(spec)?;
    // FNV-1a, 64-bit, so the folder's name stays the same across builds.
    let hash = hooks.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    let dir = crate::skills::cli_dir("antigravity").join(format!("{hash:016x}"));
    let file = dir.join(".agents").join("hooks.json");
    if std::fs::read_to_string(&file).ok().as_deref() != Some(hooks.as_str()) {
        std::fs::create_dir_all(file.parent()?).ok()?;
        std::fs::write(&file, hooks).ok()?;
    }
    Some(dir)
}

/// The hooks file Antigravity runs, or None when there are no hooks to run.
fn antigravity_hooks(spec: &TurnSpec) -> Option<String> {
    let mut hooks = hooks_json(spec.hooks, AgentKind::Antigravity);
    // The prompt is the session's first system message, so a resumed conversation keeps the one
    // it started with.
    if antigravity_prompt(spec).is_some() {
        let prompt = json!({
            "hooks": [{"type": "command", "command": patchwork_command(AgentKind::Antigravity, &[PROMPT_ARG])?}],
        });
        let groups = hooks["hooks"]["SessionStart"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        hooks["hooks"]["SessionStart"] =
            Value::Array(std::iter::once(prompt).chain(groups).collect());
    }
    // Installed for every turn, so a rule saved while the turn runs decides its next call.
    if crate::provider::mcp::relay_file().is_some()
        && crate::provider::mcp::relay_endpoint().is_some()
    {
        let permission = json!({
            "matcher": "*",
            "hooks": [{"type": "command", "command": patchwork_command(AgentKind::Antigravity, &[HOOK_ARG, "antigravity"])?, "timeout": 86400}],
        });
        let groups = hooks["hooks"]["PreToolUse"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        hooks["hooks"]["PreToolUse"] =
            Value::Array(std::iter::once(permission).chain(groups).collect());
    }
    if hooks["hooks"]
        .as_object()
        .is_none_or(serde_json::Map::is_empty)
    {
        return None;
    }
    serde_json::to_string_pretty(&named(&hooks)).ok()
}

/// `hooks` in the shape Antigravity reads, which keys each hook by a name of its own: a tool
/// event's groups keep their matcher, and the other events list their commands directly. It
/// refuses a file in the shape Claude Code reads.
fn named(hooks: &Value) -> Value {
    let mut named = serde_json::Map::new();
    for (event, groups) in hooks["hooks"].as_object().into_iter().flatten() {
        for (i, group) in groups.as_array().into_iter().flatten().enumerate() {
            let entries = if matches!(event.as_str(), "PreToolUse" | "PostToolUse") {
                vec![group.clone()]
            } else {
                group["hooks"].as_array().cloned().unwrap_or_default()
            };
            let mut hook = serde_json::Map::new();
            hook.insert(event.clone(), Value::Array(entries));
            named.insert(
                format!("patchwork-{}-{i}", event.to_ascii_lowercase()),
                Value::Object(hook),
            );
        }
    }
    Value::Object(named)
}

#[cfg(test)]
mod tests {
    use super::CODEX_TOOLS;

    /// The hook runs for every turn, so it has to be asked about every call a rule of any kind
    /// can answer, and about nothing else.
    #[test]
    fn the_permission_hook_covers_the_tools_a_rule_answers() {
        let matcher = regex::Regex::new(CODEX_TOOLS).expect("the matcher is a regex");
        for tool in ["Bash", "apply_patch", "mcp__github__search"] {
            assert!(matcher.is_match(tool), "{tool} is asked about");
        }
        for tool in ["Read", "BashOutput", "WebFetch"] {
            assert!(!matcher.is_match(tool), "{tool} is not asked about");
        }
    }
}
