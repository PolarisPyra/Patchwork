//! The tools Patchwork's own agent gives an API model: reading, searching, and changing the
//! project's files, running shell commands, and fetching web pages.

use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::types::AccessMode;

/// Longest tool result sent back to the model; the middle of a longer one is cut.
const MAX_RESULT: usize = 40_000;
/// Lines `read_file` returns when no limit is given.
const READ_LINES: usize = 2000;
const MAX_LINE: usize = 2000;
const MAX_MATCHES: usize = 200;
const BASH_TIMEOUT: u64 = 120;
const MAX_BASH_TIMEOUT: u64 = 600;
const MAX_FETCH: usize = 300_000;
/// Output a command's stdout or stderr keeps in memory.
const MAX_PIPE: usize = 1_000_000;

/// The tools offered to the model. Plan mode offers only those that change nothing.
pub fn definitions(plan: bool) -> Value {
    let tool = |name: &str, description: &str, parameters: Value| {
        json!({"type": "function", "function": {"name": name, "description": description, "parameters": parameters}})
    };
    let path = json!({"type": "string", "description": "A path relative to the project root, or absolute"});
    let mut tools = vec![
        tool(
            "read_file",
            "Read a text file. Returns numbered lines. Use offset and limit for long files.",
            json!({"type": "object", "properties": {
                "path": path,
                "offset": {"type": "integer", "description": "First line to read, from 1"},
                "limit": {"type": "integer", "description": "How many lines to read"}
            }, "required": ["path"]}),
        ),
        tool(
            "list_dir",
            "List a directory's entries. Directories end with /.",
            json!({"type": "object", "properties": {"path": path}}),
        ),
        tool(
            "glob",
            "Find files whose path matches a glob such as **/*.rs, newest first. Skips ignored files.",
            json!({"type": "object", "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string", "description": "Directory to search in; the project root by default"}
            }, "required": ["pattern"]}),
        ),
        tool(
            "grep",
            "Search file contents with a regular expression. Returns path:line: text. Skips ignored files.",
            json!({"type": "object", "properties": {
                "pattern": {"type": "string", "description": "A Rust regular expression"},
                "path": {"type": "string", "description": "File or directory to search; the project root by default"},
                "glob": {"type": "string", "description": "Only search files matching this glob, e.g. *.ts"},
                "case_insensitive": {"type": "boolean"}
            }, "required": ["pattern"]}),
        ),
        tool(
            "fetch_url",
            "Fetch a web page or file over HTTP(S) and return its text.",
            json!({"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}),
        ),
        tool(
            "ask_user",
            "Ask the user one or more multiple-choice questions when a decision is genuinely theirs. The user can also type their own answer.",
            json!({"type": "object", "properties": {"questions": {"type": "array", "items": {
                "type": "object",
                "properties": {
                    "header": {"type": "string", "description": "A label of a few words"},
                    "question": {"type": "string"},
                    "options": {"type": "array", "items": {"type": "object", "properties": {
                        "label": {"type": "string"},
                        "description": {"type": "string"}
                    }, "required": ["label"]}},
                    "multi_select": {"type": "boolean"}
                },
                "required": ["question", "options"]
            }}}, "required": ["questions"]}),
        ),
        tool(
            "update_plan",
            "Show the user a checklist of the steps of a multi-step task, and keep it current as steps finish.",
            json!({"type": "object", "properties": {"steps": {"type": "array", "items": {
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "status": {"type": "string", "enum": ["pending", "running", "done"]}
                },
                "required": ["text", "status"]
            }}}, "required": ["steps"]}),
        ),
    ];
    if plan {
        tools.push(tool(
            "propose_plan",
            "Present the finished implementation plan to the user, as Markdown. Call this once, when the plan is ready.",
            json!({"type": "object", "properties": {"plan": {"type": "string"}}, "required": ["plan"]}),
        ));
        return Value::Array(tools);
    }
    tools.extend([
        tool(
            "write_file",
            "Create or overwrite a file with the given content. Prefer edit_file for changes to an existing file.",
            json!({"type": "object", "properties": {
                "path": path,
                "content": {"type": "string"}
            }, "required": ["path", "content"]}),
        ),
        tool(
            "edit_file",
            "Replace old_string with new_string in a file. old_string must match exactly, including indentation, and be unique unless replace_all is set. Read the file first.",
            json!({"type": "object", "properties": {
                "path": path,
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "replace_all": {"type": "boolean"}
            }, "required": ["path", "old_string", "new_string"]}),
        ),
        tool(
            "bash",
            "Run a shell command in the project root and return its output and exit code.",
            json!({"type": "object", "properties": {
                "command": {"type": "string"},
                "timeout": {"type": "integer", "description": "Seconds before the command is stopped; 120 by default, at most 600"}
            }, "required": ["command"]}),
        ),
    ]);
    Value::Array(tools)
}

/// The display name of a tool, as the chat shows tool calls.
pub fn display_name(name: &str) -> &str {
    match name {
        "read_file" => "Read",
        "write_file" => "Write",
        "edit_file" => "Edit",
        "list_dir" => "LS",
        "glob" => "Glob",
        "grep" => "Grep",
        "bash" => "Bash",
        "fetch_url" => "WebFetch",
        "ask_user" => "AskUserQuestion",
        "update_plan" => "TodoWrite",
        "propose_plan" => "ExitPlanMode",
        other => other,
    }
}

/// A one-line summary of a call's arguments.
pub fn summary(name: &str, args: &Value, root: &Path) -> String {
    let field = |key: &str| args[key].as_str().unwrap_or_default().to_string();
    let summary = match name {
        "bash" => field("command"),
        "fetch_url" => field("url"),
        "glob" | "grep" => field("pattern"),
        "ask_user" => args["questions"][0]["question"].as_str().unwrap_or_default().to_string(),
        "update_plan" | "propose_plan" => String::new(),
        _ if name.starts_with("mcp__") => args.to_string(),
        _ => args["path"].as_str().map(|p| shown(&resolve(root, p), root)).unwrap_or_default(),
    };
    summary.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect()
}

/// A path as the user sees it: relative to the project when inside it.
pub fn shown(path: &Path, root: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

/// The approval a call needs before it runs, as its title and detail, or None to run at once.
pub fn approval(name: &str, args: &Value, mode: AccessMode, root: &Path) -> Option<(String, String)> {
    if mode == AccessMode::FullAccess {
        return None;
    }
    let path = args["path"].as_str().map(|p| resolve(root, p));
    if let Some(path) = &path
        && !path.starts_with(root)
    {
        let title = if writes(name) { "Change a file outside the project" } else { "Read outside the project" };
        return Some((title.into(), path.display().to_string()));
    }
    match name {
        "write_file" | "edit_file" if mode == AccessMode::Supervised => {
            let title = if name == "write_file" { "Write a file" } else { "Edit a file" };
            Some((title.into(), path.map(|p| shown(&p, root)).unwrap_or_default()))
        }
        "bash" if matches!(mode, AccessMode::Supervised | AccessMode::AutoAcceptEdits) => {
            Some(("Run a command".into(), args["command"].as_str().unwrap_or_default().into()))
        }
        "fetch_url" if mode == AccessMode::Supervised => {
            Some(("Fetch a web page".into(), args["url"].as_str().unwrap_or_default().into()))
        }
        _ => None,
    }
}

pub fn writes(name: &str) -> bool {
    matches!(name, "write_file" | "edit_file")
}

/// `path` resolved against the root, with `.` and `..` folded away.
pub fn resolve(root: &Path, path: &str) -> PathBuf {
    let path = path.trim();
    let expanded = match path.strip_prefix("~/") {
        Some(rest) => super::home_dir().unwrap_or_default().join(rest),
        None => PathBuf::from(path),
    };
    let joined = if expanded.is_absolute() { expanded } else { root.join(expanded) };
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Runs a file, search, shell, or web tool. Ok carries the result for the model; Err a failure
/// the model is told about.
pub fn run(name: &str, args: &Value, root: &Path) -> Result<String, String> {
    let path = || {
        args["path"]
            .as_str()
            .filter(|p| !p.trim().is_empty())
            .map(|p| resolve(root, p))
            .ok_or_else(|| "path is required".to_string())
    };
    let text = |key: &str| {
        args[key]
            .as_str()
            .map(String::from)
            .ok_or_else(|| format!("{key} is required"))
    };
    let result = match name {
        "read_file" => read_file(
            &path()?,
            args["offset"].as_u64().unwrap_or(1).max(1) as usize,
            args["limit"].as_u64().map_or(READ_LINES, |l| l as usize),
        ),
        "write_file" => write_file(&path()?, &text("content")?, root),
        "edit_file" => edit_file(
            &path()?,
            &text("old_string")?,
            &text("new_string")?,
            args["replace_all"] == true,
            root,
        ),
        "list_dir" => list_dir(&path().unwrap_or_else(|_| root.to_path_buf()), root),
        "glob" => glob(&text("pattern")?, &path().unwrap_or_else(|_| root.to_path_buf()), root),
        "grep" => grep(
            &text("pattern")?,
            &path().unwrap_or_else(|_| root.to_path_buf()),
            args["glob"].as_str(),
            args["case_insensitive"] == true,
            root,
        ),
        "bash" => bash(
            &text("command")?,
            args["timeout"].as_u64().unwrap_or(BASH_TIMEOUT).clamp(1, MAX_BASH_TIMEOUT),
            root,
        ),
        "fetch_url" => fetch_url(&text("url")?),
        other => Err(format!("There is no tool named {other}")),
    }?;
    Ok(cap(result))
}

/// Cuts a long result to its start and end.
pub fn cap(text: String) -> String {
    if text.len() <= MAX_RESULT {
        return text;
    }
    let mut head = MAX_RESULT * 3 / 4;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - MAX_RESULT / 4;
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    let cut = text[head..tail].lines().count();
    format!("{}\n\n[… {cut} lines cut …]\n\n{}", &text[..head], &text[tail..])
}

fn read_file(path: &Path, offset: usize, limit: usize) -> Result<String, String> {
    if path.is_dir() {
        return Err(format!("{} is a directory; use list_dir", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return Err(format!("{} is a binary file", path.display()));
    }
    let text = String::from_utf8_lossy(&bytes);
    if text.is_empty() {
        return Ok("(empty file)".into());
    }
    let total = text.lines().count();
    let mut out = String::new();
    for (i, line) in text.lines().enumerate().skip(offset - 1).take(limit) {
        let line: String = line.chars().take(MAX_LINE).collect();
        out.push_str(&format!("{:>6}\t{line}\n", i + 1));
    }
    let shown_end = (offset - 1 + limit).min(total);
    if shown_end < total {
        out.push_str(&format!("\n({total} lines in all; read on with offset {})", shown_end + 1));
    }
    Ok(out)
}

fn write_file(path: &Path, content: &str, root: &Path) -> Result<String, String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let existed = path.exists();
    std::fs::write(path, content).map_err(|e| format!("{}: {e}", path.display()))?;
    let verb = if existed { "Wrote" } else { "Created" };
    Ok(format!("{verb} {} ({} lines)", shown(path, root), content.lines().count()))
}

fn edit_file(path: &Path, old: &str, new: &str, all: bool, root: &Path) -> Result<String, String> {
    if old == new {
        return Err("old_string and new_string are the same".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if old.is_empty() {
        if !text.is_empty() {
            return Err("old_string is empty, but the file is not; give the text to replace".into());
        }
        std::fs::write(path, new).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(format!("Edited {}", shown(path, root)));
    }
    let count = text.matches(old).count();
    let updated = match count {
        0 => {
            return Err(format!(
                "old_string was not found in {}. Read the file again and match it exactly",
                shown(path, root)
            ));
        }
        1 => text.replacen(old, new, 1),
        _ if all => text.replace(old, new),
        n => {
            return Err(format!(
                "old_string appears {n} times in {}; add surrounding lines to make it unique, or set replace_all",
                shown(path, root)
            ));
        }
    };
    std::fs::write(path, updated).map_err(|e| format!("{}: {e}", path.display()))?;
    let times = if count > 1 { format!(" ({count} places)") } else { String::new() };
    Ok(format!("Edited {}{times}", shown(path, root)))
}

fn list_dir(path: &Path, root: &Path) -> Result<String, String> {
    let mut entries: Vec<String> = std::fs::read_dir(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .flatten()
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.file_type().is_ok_and(|t| t.is_dir()) { format!("{name}/") } else { name }
        })
        .collect();
    entries.sort();
    let total = entries.len();
    entries.truncate(500);
    let mut out = format!("{}:\n{}", shown(path, root), entries.join("\n"));
    if total > entries.len() {
        out.push_str(&format!("\n({total} entries in all)"));
    }
    Ok(out)
}

/// Files under `dir`, honoring .gitignore and skipping `.git`.
fn walk(dir: &Path) -> impl Iterator<Item = PathBuf> {
    ignore::WalkBuilder::new(dir)
        .hidden(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .map(|e| e.into_path())
}

fn glob(pattern: &str, dir: &Path, root: &Path) -> Result<String, String> {
    let matcher = globset::GlobBuilder::new(pattern)
        .literal_separator(pattern.contains('/'))
        .build()
        .map_err(|e| e.to_string())?
        .compile_matcher();
    let mut found: Vec<(PathBuf, std::time::SystemTime)> = walk(dir)
        .filter(|p| {
            let rel = p.strip_prefix(dir).unwrap_or(p);
            matcher.is_match(rel) || (!pattern.contains('/') && p.file_name().is_some_and(|n| matcher.is_match(n)))
        })
        .map(|p| {
            let modified = p.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (p, modified)
        })
        .collect();
    if found.is_empty() {
        return Ok("No files found".into());
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.1));
    let total = found.len();
    let mut out: Vec<String> = found.iter().take(MAX_MATCHES).map(|(p, _)| shown(p, root)).collect();
    if total > MAX_MATCHES {
        out.push(format!("({total} files in all; narrow the pattern)"));
    }
    Ok(out.join("\n"))
}

fn grep(pattern: &str, path: &Path, filter: Option<&str>, nocase: bool, root: &Path) -> Result<String, String> {
    let regex = regex::RegexBuilder::new(pattern)
        .case_insensitive(nocase)
        .build()
        .map_err(|e| e.to_string())?;
    let filter = filter
        .map(|g| globset::Glob::new(g).map(|g| g.compile_matcher()))
        .transpose()
        .map_err(|e| e.to_string())?;
    let files: Box<dyn Iterator<Item = PathBuf>> = if path.is_file() {
        Box::new(std::iter::once(path.to_path_buf()))
    } else {
        Box::new(walk(path))
    };
    let mut out = Vec::new();
    let mut more = false;
    'files: for file in files {
        if let Some(filter) = &filter {
            let rel = file.strip_prefix(path).unwrap_or(&file);
            if !filter.is_match(rel) && !file.file_name().is_some_and(|n| filter.is_match(n)) {
                continue;
            }
        }
        let Ok(bytes) = std::fs::read(&file) else { continue };
        if bytes[..bytes.len().min(8192)].contains(&0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        for (i, line) in text.lines().enumerate() {
            if regex.is_match(line) {
                // One match past the limit is enough to know there are more.
                if out.len() == MAX_MATCHES {
                    more = true;
                    break 'files;
                }
                let line: String = line.trim().chars().take(300).collect();
                out.push(format!("{}:{}: {line}", shown(&file, root), i + 1));
            }
        }
    }
    if out.is_empty() {
        return Ok("No matches".into());
    }
    if more {
        out.push(format!("(more than {MAX_MATCHES} matches; narrow the search)"));
    }
    Ok(out.join("\n"))
}

fn bash(command: &str, timeout: u64, root: &Path) -> Result<String, String> {
    // The command stays in the agent's process group, so stopping the turn stops it too.
    #[cfg(unix)]
    let mut cmd = {
        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or("/bin/sh".into());
        let mut cmd = crate::host_command(shell);
        cmd.args(["-c", command]);
        cmd
    };
    #[cfg(not(unix))]
    let mut cmd = {
        let mut cmd = crate::host_command("cmd");
        cmd.args(["/C", command]);
        cmd
    };
    cmd.current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("Failed to run the command: {e}"))?;
    let (Some(out_pipe), Some(err_pipe)) = (child.stdout.take(), child.stderr.take()) else {
        return Err("The command's output is unavailable".into());
    };
    // Kept well above what the model is shown, so the head and tail it sees are the real ones.
    let (out, out_reader) = super::read_pipe_upto(out_pipe, MAX_PIPE);
    let (err, err_reader) = super::read_pipe_upto(err_pipe, MAX_PIPE);
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // A process the command left running can hold the pipes open; its output so far is kept.
    let grace = Instant::now() + Duration::from_secs(1);
    while !(out_reader.is_finished() && err_reader.is_finished()) && Instant::now() < grace {
        std::thread::sleep(Duration::from_millis(10));
    }
    let (stdout, stderr) = (super::text(&out), super::text(&err));
    let mut text = stdout.trim_end().to_string();
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim_end());
    }
    match status {
        None => Err(format!("{text}\n\nThe command was stopped after {timeout} s").trim_start().into()),
        Some(s) if s.success() => Ok(if text.is_empty() { "(no output)".into() } else { text }),
        Some(s) => {
            let code = s.code().map_or("a signal".into(), |c| c.to_string());
            Err(format!("{text}\n\nExit code {code}").trim_start().into())
        }
    }
}

fn fetch_url(url: &str) -> Result<String, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only http and https URLs can be fetched".into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("User-Agent", "Patchwork")
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    let status = response.status();
    let html = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.contains("html"));
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_FETCH as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let body = String::from_utf8_lossy(&bytes).into_owned();
    let text = if html { html_text(&body) } else { body };
    if !status.is_success() {
        return Err(format!("{url} answered {status}\n\n{}", text.chars().take(2000).collect::<String>()));
    }
    Ok(text)
}

/// The readable text of an HTML page: tags, scripts, and styles removed, blank runs folded.
fn html_text(html: &str) -> String {
    let mut out = String::new();
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while i < html.len() {
        let rest = &lower[i..];
        if rest.starts_with("<script") || rest.starts_with("<style") {
            let close = if rest.starts_with("<script") { "</script>" } else { "</style>" };
            i += rest.find(close).map_or(rest.len(), |end| end + close.len());
            continue;
        }
        if rest.starts_with('<') {
            let block = ["<p", "<br", "<div", "<li", "<h", "<tr", "</p", "</div", "</li"]
                .iter()
                .any(|t| rest.starts_with(t));
            if block {
                out.push('\n');
            }
            i += rest.find('>').map_or(rest.len(), |end| end + 1);
            continue;
        }
        let next = rest.find('<').unwrap_or(rest.len());
        out.push_str(&html[i..i + next]);
        i += next;
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let mut lines = Vec::new();
    for line in decoded.lines().map(str::trim) {
        if line.is_empty() && lines.last().is_none_or(|l: &&str| l.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    lines.join("\n")
}
