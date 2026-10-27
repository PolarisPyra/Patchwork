//! Patchwork's skills: folders holding a `SKILL.md`, all kept in one skills folder that every
//! agent loads. The folder is `skills` in the config folder unless the user picks another.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use notify::{RecursiveMode, Watcher as _};
use serde::Serialize;

use crate::types::{AgentKind, Settings};

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Remembers the config folder and creates the default skills folder in it.
pub fn init(config_dir: &Path) {
    let _ = CONFIG_DIR.set(config_dir.to_path_buf());
    let _ = std::fs::create_dir_all(default_dir());
}

fn default_dir() -> PathBuf {
    CONFIG_DIR.get().cloned().unwrap_or_default().join("skills")
}

/// The skills folder in use.
pub fn dir(settings: &Settings) -> PathBuf {
    match settings.skills_dir.as_deref().map(str::trim) {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => default_dir(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub name: String,
    pub description: Option<String>,
    /// The skill's `SKILL.md`.
    pub path: String,
    /// Switched on in Settings; agents only load enabled skills.
    pub enabled: bool,
}

/// The skills in `dir`, by name: each subfolder with a `SKILL.md`.
pub fn list(dir: &Path) -> Vec<Skill> {
    let mut skills: Vec<Skill> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path().join("SKILL.md");
            let text = std::fs::read_to_string(&path).ok()?;
            let (name, description) = frontmatter(&text);
            let name = name.unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
            // The real path, also for skills reached through the links agents load.
            let path = std::fs::canonicalize(&path).unwrap_or(path).to_string_lossy().into_owned();
            Some(Skill { name, description, path, enabled: true })
        })
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// The skills in the skills folder, each marked enabled unless switched off in Settings.
pub fn all(settings: &Settings) -> Vec<Skill> {
    let mut skills = list(&dir(settings));
    for skill in &mut skills {
        skill.enabled = !settings.disabled_skills.contains(&skill.name);
    }
    skills
}

pub fn enabled(settings: &Settings) -> Vec<Skill> {
    all(settings).into_iter().filter(|s| s.enabled).collect()
}

/// `name` and `description` from the `---` block at the top of a `SKILL.md`. A value may be
/// quoted, or folded onto the indented lines below its key.
fn frontmatter(text: &str) -> (Option<String>, Option<String>) {
    let Some(rest) = text.strip_prefix("---") else { return (None, None) };
    let block = rest.split("\n---").next().unwrap_or_default();
    let lines: Vec<&str> = block.lines().collect();
    let value = |key: &str| -> Option<String> {
        let at = lines.iter().position(|l| l.strip_prefix(key).is_some_and(|r| r.starts_with(':')))?;
        let inline = lines[at][key.len() + 1..].trim();
        let text = if matches!(inline, "" | ">" | "|" | ">-" | "|-") {
            lines[at + 1..]
                .iter()
                .take_while(|l| l.starts_with([' ', '\t']))
                .map(|l| l.trim())
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            inline.trim_matches(['"', '\'']).to_string()
        };
        Some(text).filter(|t| !t.is_empty())
    };
    (value("name"), value("description"))
}

/// The names of the `$name` words in a message, wherever they are, without trailing punctuation.
pub fn mentioned(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .filter_map(|word| word.strip_prefix('$'))
        .map(|name| name.trim_end_matches(['.', ',', '!', '?', ';', ':', ')']))
}

/// The first skill a message names as `$name`, and the text it goes with: the text after the
/// name when the message starts with it, or else the whole message as written.
pub fn invoked<'a>(text: &'a str, skills: &[Skill]) -> Option<(Skill, &'a str)> {
    let skill = mentioned(text).find_map(|name| skills.iter().find(|s| s.name == name))?.clone();
    let rest = text.trim_start();
    let after = rest
        .strip_prefix('$')
        .and_then(|r| r.strip_prefix(skill.name.as_str()))
        .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace));
    Some((skill, after.unwrap_or(rest).trim()))
}

/// The message as the agent should get it when it invokes `skill`: Claude runs the skill as a
/// command of the `patchwork` plugin, Codex reads a `$name` mention, and the rest are told
/// which `SKILL.md` to follow.
pub fn invocation(agent: AgentKind, skill: &Skill, rest: &str) -> String {
    let text = match agent {
        AgentKind::Claude => format!("/{PLUGIN}:{} {rest}", skill.name),
        // A `$name` later in the message already mentions the skill.
        AgentKind::Codex if mentioned(rest).any(|name| name == skill.name) => rest.to_string(),
        AgentKind::Codex => format!("${} {rest}", skill.name),
        AgentKind::OpenCode | AgentKind::Antigravity | AgentKind::Api => {
            format!("Use the {} skill: read {} and follow it.\n\n{rest}", skill.name, skill.path)
        }
    };
    text.trim_end().to_string()
}

/// Claude loads the skills as this plugin's, so they show up as `patchwork:<name>`.
const PLUGIN: &str = "patchwork";

/// A plugin folder for Claude's `--plugin-dir`, whose `skills` folder links to each enabled
/// skill. Claude only finds skills in a plugin's own `skills` folder, the skills folder can be
/// anywhere, and switched-off skills must stay out, so the links are brought up to date on
/// every launch. Codex and OpenCode load the same links.
#[cfg(unix)]
pub fn claude_plugin(settings: &Settings) -> Option<PathBuf> {
    let plugin = CONFIG_DIR.get()?.join("claude-plugin");
    let manifest = plugin.join(".claude-plugin");
    std::fs::create_dir_all(&manifest).ok()?;
    let json = format!("{{\"name\":\"{PLUGIN}\"}}\n");
    if std::fs::read_to_string(manifest.join("plugin.json")).ok().as_deref() != Some(json.as_str()) {
        std::fs::write(manifest.join("plugin.json"), json).ok()?;
    }
    let links = plugin.join("skills");
    // It used to be one link to the whole skills folder.
    if links.is_symlink() {
        let _ = std::fs::remove_file(&links);
    }
    std::fs::create_dir_all(&links).ok()?;
    let wanted: Vec<(std::ffi::OsString, PathBuf)> = enabled(settings)
        .iter()
        .filter_map(|skill| {
            let folder = Path::new(&skill.path).parent()?;
            Some((folder.file_name()?.to_owned(), folder.to_path_buf()))
        })
        .collect();
    for entry in std::fs::read_dir(&links).ok()?.filter_map(Result::ok) {
        let target = std::fs::read_link(entry.path()).ok();
        if !wanted.iter().any(|(name, folder)| *name == entry.file_name() && target.as_ref() == Some(folder)) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    for (name, folder) in &wanted {
        let link = links.join(name);
        if !link.is_symlink() {
            let _ = std::os::unix::fs::symlink(folder, link);
        }
    }
    Some(plugin)
}

#[cfg(not(unix))]
pub fn claude_plugin(_settings: &Settings) -> Option<PathBuf> {
    None
}

/// The folder agents load skills from: the links to the enabled skills, or the skills folder
/// itself where there are no links.
pub fn loaded_dir(settings: &Settings) -> PathBuf {
    claude_plugin(settings).map(|plugin| plugin.join("skills")).unwrap_or_else(|| dir(settings))
}

/// Watches the skills folder until dropped.
pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
}

/// Quiet time that ends a burst of changes in the skills folder.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(200);

/// Watches `dir` and everything in it, following links, and runs `on_change` on a thread of its
/// own once each burst of changes settles.
pub fn watch(dir: &Path, on_change: impl Fn() + Send + 'static) -> Result<Watcher, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if !res.is_ok_and(|e| matches!(e.kind, notify::EventKind::Access(_))) {
            let _ = tx.send(());
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(dir, RecursiveMode::Recursive)
        .map_err(|e| format!("Could not watch {}: {e}", dir.display()))?;
    // Ends once the watcher is dropped, which closes the channel.
    std::thread::Builder::new()
        .name("skills-watch".into())
        .spawn(move || {
            while rx.recv().is_ok() {
                while rx.recv_timeout(SETTLE).is_ok() {}
                on_change();
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Watcher { _watcher: watcher })
}

/// What Patchwork's own agent is told about the skills, which it reads with its file tools.
pub fn instructions(skills: &[Skill]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut text = String::from(
        "Skills: each is a SKILL.md with instructions for one kind of task. When a task matches a \
         skill, read its SKILL.md first and follow it.",
    );
    for skill in skills {
        text.push_str(&format!("\n- {} ({})", skill.name, skill.path));
        if let Some(description) = &skill.description {
            text.push_str(&format!(": {description}"));
        }
    }
    Some(text)
}
