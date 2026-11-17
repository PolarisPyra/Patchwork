//! Patchwork's skills: folders holding a `SKILL.md`, all kept in one skills folder that every
//! agent loads. The folder is `skills` in the config folder unless the user picks another.
//!
//! The skills folder: listing, mentioning, and mirroring skills into each agent, and watching for changes.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use notify::{RecursiveMode, Watcher as _};
use serde::Serialize;

use crate::provider::AgentKind;
use crate::settings::Settings;

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Remembers the config folder, creates the default skills folder in it, and removes the folders
/// an older version built for the CLIs.
pub fn init(config_dir: &Path) {
    let _ = CONFIG_DIR.set(config_dir.to_path_buf());
    if let Err(e) = std::fs::create_dir_all(default_dir()) {
        log::warn!(error:% = e; "could not create the skills folder: {{error}}");
    }
    for old in ["skills-enabled", "agents-for-claude", "agents-for-codex"] {
        match std::fs::remove_dir_all(config_dir.join(old)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                log::warn!(error:% = e; "could not remove an old folder: {{error}}");
            }
            _ => {}
        }
    }
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
            let path = std::fs::canonicalize(&path)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned();
            Some(Skill {
                name,
                description,
                path,
                enabled: true,
            })
        })
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// The skills in the skills folder, each marked enabled unless switched off in Settings.
#[must_use]
pub fn all(settings: &Settings) -> Vec<Skill> {
    let mut skills = list(&dir(settings));
    for skill in &mut skills {
        skill.enabled = !settings.disabled_skills.contains(&skill.name);
    }
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

#[must_use]
pub fn enabled(settings: &Settings) -> Vec<Skill> {
    all(settings).into_iter().filter(|s| s.enabled).collect()
}

/// `name` and `description` from the `---` block at the top of a `SKILL.md`.
pub(crate) fn frontmatter(text: &str) -> (Option<String>, Option<String>) {
    (field(text, "name"), field(text, "description"))
}

/// `key`'s value in the `---` block at the top of a Markdown file. A value may be quoted, or
/// folded onto the indented lines below its key.
pub(crate) fn field(text: &str, key: &str) -> Option<String> {
    let rest = text.strip_prefix("---")?;
    let block = rest.split("\n---").next().unwrap_or_default();
    let lines: Vec<&str> = block.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.strip_prefix(key).is_some_and(|r| r.starts_with(':')))?;
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
}

/// The names of the `$name` words in a message, wherever they are, without trailing punctuation.
pub fn mentioned(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .filter_map(|word| word.strip_prefix('$'))
        .map(|name| name.trim_end_matches(['.', ',', '!', '?', ';', ':', ')']))
}

/// The first skill a message names as `$name`, and the text it goes with: the text after the
/// name when the message starts with it, or else the whole message as written.
#[must_use]
pub fn invoked<'a>(text: &'a str, skills: &[Skill]) -> Option<(Skill, &'a str)> {
    let skill = mentioned(text)
        .find_map(|name| skills.iter().find(|s| s.name == name))?
        .clone();
    let rest = text.trim_start();
    let after = rest
        .strip_prefix('$')
        .and_then(|r| r.strip_prefix(skill.name.as_str()))
        .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace));
    Some((skill, after.unwrap_or(rest).trim()))
}

/// The message as the agent should get it when it invokes `skill`: Claude runs the skill as a
/// command of the `claude` plugin, Codex reads a `$name` mention, and the rest are told
/// which `SKILL.md` to follow.
#[must_use]
pub fn invocation(agent: AgentKind, skill: &Skill, rest: &str) -> String {
    let text = match agent {
        AgentKind::Claude => format!("/{PLUGIN}:{} {rest}", skill.name),
        // A `$name` later in the message already mentions the skill.
        AgentKind::Codex if mentioned(rest).any(|name| name == skill.name) => rest.to_string(),
        AgentKind::Codex => format!("${} {rest}", skill.name),
        AgentKind::OpenCode | AgentKind::Antigravity | AgentKind::Api => {
            format!(
                "Use the {} skill: read {} and follow it.\n\n{rest}",
                skill.name, skill.path
            )
        }
    };
    text.trim_end().to_string()
}

/// Claude loads the skills as the plugin in `<cache>/launch/claude`, which takes its name from
/// that folder, so they show up as `claude:<name>`.
const PLUGIN: &str = "claude";

/// The folder a CLI loads its skills and agents from: `<cache>/launch/<cli>`, holding `skills`
/// and `agents`.
pub(crate) fn cli_dir(cli: &str) -> PathBuf {
    crate::system::dirs().launch(cli)
}

/// Syncs a CLI's `skills` folder with the enabled skills, and returns the CLI's folder.
///
/// Makes the `skills` folder in a CLI's folder hold a copy of each enabled skill and nothing
/// else, and returns the CLI's folder. The copies are brought up to date on every launch, so
/// switched-off skills stay out. They are copies rather than links, since Windows needs a
/// privilege to make links.
pub fn sync(settings: &Settings, cli: &str) -> Option<PathBuf> {
    let root = cli_dir(cli);
    let skills = root.join("skills");
    std::fs::create_dir_all(&skills).ok()?;
    let wanted: Vec<(std::ffi::OsString, PathBuf)> = enabled(settings)
        .iter()
        .filter_map(|skill| {
            let folder = Path::new(&skill.path).parent()?;
            Some((folder.file_name()?.to_owned(), folder.to_path_buf()))
        })
        .collect();
    for entry in std::fs::read_dir(&skills).ok()?.filter_map(Result::ok) {
        if !wanted.iter().any(|(name, _)| *name == entry.file_name()) {
            remove_path(&entry.path());
        }
    }
    for (name, folder) in &wanted {
        if let Err(e) = mirror(folder, &skills.join(name)) {
            log::warn!(
                skill:% = name.to_string_lossy(), cli:%, error:% = e;
                "failed to copy the {{skill}} skill for {{cli}}: {{error}}"
            );
        }
    }
    Some(root)
}

/// Makes `to` a copy of the folder `from`: files that differ are rewritten, and files `from` no
/// longer has are removed.
fn mirror(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            mirror(&source, &target)?;
            continue;
        }
        let bytes = std::fs::read(&source)?;
        if std::fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) {
            // Written whole and then moved in, so a turn starting at the same time never reads half.
            let part = to.join(format!("{}.part", uuid::Uuid::new_v4().simple()));
            std::fs::write(&part, bytes)?;
            std::fs::rename(&part, &target)?;
        }
    }
    for entry in std::fs::read_dir(to)? {
        let entry = entry?;
        if !from.join(entry.file_name()).exists() {
            remove_path(&entry.path());
        }
    }
    Ok(())
}

/// Removes a file or a folder with everything in it, without following links inside it.
fn remove_path(path: &Path) {
    let _ = std::fs::remove_dir_all(path).or_else(|_| std::fs::remove_file(path));
}

/// The agents' own skills folders under the home folder.
const AGENT_SKILLS: [&str; 8] = [
    ".claude/skills",
    ".claude/plugins",
    ".codex/skills",
    ".agents/skills",
    ".config/opencode/skills",
    ".config/opencode/skill",
    ".config/opencode/.opencode/skills",
    ".opencode/skills",
];

/// Every folder agents read skills from, which they may read without asking: Patchwork's skills
/// folder, each CLI's copies of the enabled skills, and the agents' own skills folders.
#[must_use]
pub fn folders(settings: &Settings) -> Vec<PathBuf> {
    let mut out = vec![dir(settings)];
    out.extend(
        ["claude", "codex", "opencode"]
            .iter()
            .map(|cli| cli_dir(cli).join("skills")),
    );
    if let Some(home) = crate::system::fs::home_dir() {
        out.extend(AGENT_SKILLS.iter().map(|folder| home.join(folder)));
    }
    out
}

/// Watches the skills folder until dropped.
#[derive(Debug)]
pub struct Watcher {
    _watcher: notify::RecommendedWatcher,
}

/// Quiet time that ends a burst of changes in the skills folder.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(200);

/// Watches `dir` and everything in it, following links, and runs `on_change` on a thread of its
/// own once each burst of changes settles.
///
/// # Errors
///
/// Returns an error when the watcher cannot be created, cannot watch `dir`, or its thread cannot be
/// started.
pub fn watch(dir: &Path, on_change: impl Fn() + Send + 'static) -> Result<Watcher, Error> {
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
#[must_use]
pub fn instructions(skills: &[Skill]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut text = String::from(
        "Skills: each is a SKILL.md with instructions for one kind of task. When a task matches a \
         skill, read its SKILL.md first and follow it.",
    );
    for skill in skills {
        let _ = write!(text, "\n- {} ({})", skill.name, skill.path);
        if let Some(description) = &skill.description {
            let _ = write!(text, ": {description}");
        }
    }
    Some(text)
}
