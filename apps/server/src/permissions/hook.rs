//! Permission hooks. Codex and Antigravity decide on their own what to ask about, and in some
//! modes ask about nothing, so before each tool call they run Patchwork's binary as a hook
//! (`--permission-hook <agent>`). The hook asks the app, over the relay socket the MCP relay
//! uses, whether the permission rules refuse the call or hold it for the user, and answers the
//! agent: a refusal with its reason, or nothing, which lets the call go on as the mode decides.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::permissions::Target;
use crate::permissions::PermissionKind;
use crate::provider::AgentKind;

/// The argument that starts Patchwork's binary as an agent's permission hook.
pub const HOOK_ARG: &str = "--permission-hook";
/// Where the hook finds the relay and the thread it works for.
const ENV: &str = "PATCHWORK_PERMISSION_HOOK";

/// Environment that tells the hook which thread it works for, for the agents that run one:
/// Codex, Claude, and Antigravity. The relay's port and token are copied in: a hook that read
/// them from the file could reach another copy of Patchwork that shares the state folder and
/// wrote that file since.
pub fn env(agent: AgentKind, thread_id: &str, root: &str) -> Vec<(&'static str, String)> {
    // The agents that run Patchwork's permission hook.
    if !matches!(
        agent,
        AgentKind::Codex | AgentKind::Claude | AgentKind::Antigravity
    ) {
        return Vec::new();
    }
    let Some(endpoint) = crate::provider::mcp::relay_endpoint() else {
        return Vec::new();
    };
    let context = json!({
        "port": endpoint["port"],
        "token": endpoint["token"],
        "thread": thread_id,
        "root": root,
    });
    vec![(ENV, context.to_string())]
}

/// Runs the hook: reads the agent's tool call on stdin and writes its answer to stdout.
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
        Some((title, detail, target)) => ask(&title, &detail, target).unwrap_or_else(Some),
        None => None,
    };
    let answer = crate::provider::hooks::answer(agent, refusal);
    let mut stdout = std::io::stdout();
    let _ = stdout
        .write_all(answer.as_bytes())
        .and_then(|_| stdout.flush());
    0
}

/// Asks Patchwork about a call; the answer is why it is refused, or None to let it run.
fn ask(title: &str, detail: &str, target: Target) -> Result<Option<String>, String> {
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
    let port =
        u16::try_from(relay["port"].as_u64().unwrap_or_default()).map_err(|_| UNREACHABLE)?;
    let mut stream = connect(port).map_err(|_| UNREACHABLE)?;
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

/// Connects to the relay, trying once more after a moment: it accepts one client at a time here,
/// and may be busy answering another call.
fn connect(port: u16) -> std::io::Result<std::net::TcpStream> {
    let mut attempt = 0;
    loop {
        match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Err(e) if attempt == 0 && e.kind() == std::io::ErrorKind::ConnectionRefused => {
                attempt += 1;
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
            let command = crate::provider::hooks::shell_command(call)?;
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
    let target = crate::provider::claude::claude_target(tool, input, root)?;
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
            crate::provider::one_line(&input.to_string()),
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
        crate::provider::one_line(&input.to_string()),
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

#[cfg(test)]
mod tests {
    use super::codex_call;
    use serde_json::json;

    #[test]
    fn codex_word_array_keeps_arguments_as_data_for_pretool_checks() {
        let call = json!({
            "tool_name": "Bash",
            "tool_input": {
                "command": ["printf", "%s\\n", "while true; do sleep 1; done"]
            }
        });

        let Some((title, detail, target)) = codex_call(&call) else {
            panic!("Codex Bash calls should produce a permission target");
        };

        assert_eq!(title, "Run command");
        assert_eq!(detail, "'printf' '%s\\n' 'while true; do sleep 1; done'");
        assert_eq!(target.resources, [detail]);
        assert_eq!(
            crate::plugins::builtin::BuiltinHook::BoundedWaits.check(&call),
            None
        );

        let executed = json!({
            "tool_input": {
                "command": ["sh", "-c", "while true; do sleep 1; done"]
            }
        });
        assert!(
            crate::plugins::builtin::BuiltinHook::BoundedWaits
                .check(&executed)
                .is_some()
        );
    }
}

#[cfg(test)]
mod claude_tests {
    use crate::provider::AgentKind;

    /// A hook with no context says nothing, so every agent that runs one has to be given it.
    #[test]
    fn each_agent_that_runs_the_hook_is_told_where_it_is() {
        for agent in [AgentKind::Codex, AgentKind::Claude, AgentKind::Antigravity] {
            // Only meaningful once the relay is listening; without it there is nothing to name.
            if crate::provider::mcp::relay_endpoint().is_none() {
                assert!(super::env(agent, "t", "/w").is_empty(), "{agent:?}");
                continue;
            }
            let env = super::env(agent, "t", "/w");
            assert_eq!(env.len(), 1, "{agent:?}");
            assert!(
                env[0].1.contains("\"thread\":\"t\""),
                "{agent:?}: {}",
                env[0].1
            );
        }
    }

    use super::claude_call;
    use crate::permissions::PermissionKind;
    use serde_json::json;

    /// A rule names Claude's calls the same way whether Claude asked about them or the hook did.
    #[test]
    fn a_claude_call_reads_as_the_same_request_claude_asks_about() {
        let bash = json!({"tool_name": "Bash", "tool_input": {"command": "cargo test -q"}});
        let (title, detail, target) = claude_call(&bash, "/w").expect("a command is a request");
        assert_eq!(
            (title.as_str(), detail.as_str()),
            ("Run command", "cargo test -q")
        );
        assert_eq!(target.kind, PermissionKind::Command);

        let edit = json!({"tool_name": "Edit", "tool_input": {"file_path": "/w/src/a.rs"}});
        let (_, detail, target) = claude_call(&edit, "/w").expect("an edit is a request");
        assert_eq!(target.kind, PermissionKind::Edit);
        assert_eq!(detail, "/w/src/a.rs");

        let read = json!({"tool_name": "Read", "tool_input": {"file_path": "/etc/hosts"}});
        let (_, _, target) = claude_call(&read, "/w").expect("a read is a request");
        assert_eq!(target.kind, PermissionKind::Folder);
        assert_eq!(target.resources, ["/etc"]);

        let mcp = json!({"tool_name": "mcp__github__search", "tool_input": {"q": "x"}});
        let (title, _, target) = claude_call(&mcp, "/w").expect("an MCP call is a request");
        assert_eq!(title, "Use github__search");
        assert_eq!(target.resources, ["github/search"]);

        // A tool no rule can answer is not a request, so the hook says nothing about it.
        let todo = json!({"tool_name": "TodoWrite", "tool_input": {"todos": []}});
        assert_eq!(claude_call(&todo, "/w"), None);
    }
}
