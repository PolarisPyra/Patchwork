//! Custom agents: Markdown files in `agents` in the config folder, or in a project's
//! `.patchwork/agents`, each a role an agent takes on when a message summons it as `&name`. The
//! name is the file's name without `.md`; the frontmatter gives a title (`name`), a description,
//! a model and a mode, and the text below it is the role's instructions. A `primary` agent stays
//! the chat's agent once summoned; any other is summoned for one message.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub name: String,
    /// The frontmatter's `name`, which can differ from the file's.
    pub title: Option<String>,
    pub description: Option<String>,
    /// The model the chat switches to when the agent is summoned, if the chat's CLI has it.
    pub model: Option<String>,
    /// `mode: primary`: summoned, it stays the chat's agent until another replaces it.
    pub primary: bool,
    pub path: String,
    /// The text below the frontmatter, which goes with each message that summons the agent.
    #[serde(skip)]
    pub prompt: String,
}

type OnChange = Box<dyn Fn(Arc<[Agent]>) + Send>;

/// The agents folder's agents, read once and again after each change to the folder, so a
/// message or a menu never reads the disk for them.
pub struct Library {
    dir: PathBuf,
    agents: RwLock<Arc<[Agent]>>,
    on_change: Mutex<Option<OnChange>>,
    watcher: Mutex<Option<crate::skills::Watcher>>,
}

impl Library {
    /// Reads `agents` in `config_dir`, creating it first, and watches it for as long as the
    /// library lives. A folder that can't be watched is still read once.
    pub fn open(config_dir: &Path) -> Arc<Self> {
        let dir = config_dir.join("agents");
        let _ = std::fs::create_dir_all(&dir);
        let library = Arc::new(Self {
            agents: RwLock::new(load(&dir).into()),
            dir,
            on_change: Mutex::default(),
            watcher: Mutex::default(),
        });
        // The watcher lives in the library, so it holds the library weakly.
        let weak = Arc::downgrade(&library);
        match crate::skills::watch(&library.dir, move || {
            if let Some(library) = weak.upgrade() {
                library.reload();
            }
        }) {
            Ok(watcher) => {
                *library
                    .watcher
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = Some(watcher)
            }
            Err(e) => log::warn!("{e}"),
        }
        library
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn list(&self) -> Arc<[Agent]> {
        self.agents
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The agents a chat in `root` can summon: the agents folder's, and the project's own, which
    /// replace any of the same name.
    pub fn available(&self, root: &Path) -> Vec<Agent> {
        let mut agents = self.list().to_vec();
        for agent in load(&root.join(PROJECT_DIR)) {
            match agents.iter_mut().find(|a| a.name == agent.name) {
                Some(same) => *same = agent,
                None => agents.push(agent),
            }
        }
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        agents
    }

    /// Adds `name.md` to the agents folder, for the user to fill in.
    pub fn create(&self, name: &str) -> Result<(), String> {
        if !summonable(name) {
            return Err("Use letters and digits joined by - or _, starting with a letter".into());
        }
        let path = self.dir.join(format!("{name}.md"));
        let template = format!(
            "---\nname: {name}\ndescription:\nmode: subagent\nmodel:\n---\n\nInstructions for the {name} agent.\n"
        );
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut file| std::io::Write::write_all(&mut file, template.as_bytes()))
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::AlreadyExists => {
                    format!("There is already an agent named {name}")
                }
                _ => e.to_string(),
            })
    }

    /// Passes the agents to `on_change` after each change to them, in place of any earlier one.
    pub fn subscribe(&self, on_change: impl Fn(Arc<[Agent]>) + Send + 'static) {
        *self
            .on_change
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Box::new(on_change));
    }

    fn reload(&self) {
        let agents: Arc<[Agent]> = load(&self.dir).into();
        {
            let mut current = self.agents.write().unwrap_or_else(PoisonError::into_inner);
            // An editor's save is several events; only a real change goes out.
            if *current == agents {
                return;
            }
            *current = agents.clone();
        }
        if let Some(on_change) = self
            .on_change
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            on_change(agents);
        }
    }
}

/// The folder in a project that holds its own agents.
const PROJECT_DIR: &str = ".patchwork/agents";

/// What a chat's agent has been told of the chat's standing role, before a message.
pub enum Standing<'a> {
    /// Nothing new: it knows the role, or there is none.
    Told,
    /// The chat's agent is this one, and the agent was not told yet.
    Untold(&'a str),
    /// The chat's agent was removed, and the agent was not told yet.
    Removed,
}

pub struct Summoned {
    pub text: String,
    /// The primary agent the message summoned, which is the chat's agent from now on.
    pub role: Option<String>,
}

/// `text` with the roles it summons as `&name` after it, each once and in order, and the chat's
/// standing role when the agent must be told of it. The roles go last, so a message that starts
/// with a command still does. A message sent again, in a new turn, keeps the roles it has.
pub fn summon(agents: &[Agent], text: String, standing: Standing) -> Summoned {
    let written = text
        .split_once("\n\n<agent")
        .map_or(text.as_str(), |(written, _)| written);
    let has = |agent: &Agent| text.contains(&format!("<agent name=\"{}\">", agent.name));
    let mut summoned: Vec<&Agent> = Vec::new();
    for agent in mentioned(written).filter_map(|name| agents.iter().find(|a| a.name == name)) {
        if !summoned.iter().any(|a| a.name == agent.name) {
            summoned.push(agent);
        }
    }
    let role = summoned
        .iter()
        .rev()
        .find(|a| a.primary)
        .map(|a| a.name.clone());
    let mut roles = Vec::new();
    if role.is_none() {
        match standing {
            Standing::Told => {}
            Standing::Untold(name) => roles.extend(
                agents
                    .iter()
                    .find(|a| a.name == name && !has(a))
                    .map(|agent| block(agent, "In this chat")),
            ),
            Standing::Removed if !text.contains(REMOVED) => roles.push(REMOVED.to_string()),
            Standing::Removed => {}
        }
    }
    for agent in summoned.into_iter().filter(|a| !has(a)) {
        let scope = if role.as_ref() == Some(&agent.name) {
            "From now on in this chat"
        } else {
            "For this message"
        };
        roles.push(block(agent, scope));
    }
    let text = if roles.is_empty() {
        text
    } else {
        format!("{text}\n\n{}", roles.join("\n\n"))
    };
    Summoned { text, role }
}

/// What the agent is told when the chat's agent was removed.
const REMOVED: &str = "<agent>\nThe custom agent role given earlier in this chat no longer applies; go on as yourself.\n</agent>";

/// An agent's role, for as long as `scope` says.
fn block(agent: &Agent, scope: &str) -> String {
    format!(
        "<agent name=\"{name}\">\n{scope}, act as the {name} agent and follow these instructions:\n\n{prompt}\n</agent>",
        name = agent.name,
        prompt = agent.prompt.trim(),
    )
}

/// Whether `name` can be summoned: words of letters and digits joined by `-` or `_`, starting
/// with a letter, as the composer's `&name` rule reads them.
fn summonable(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && !name.ends_with(['-', '_'])
        && !name.contains("--")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The agents in `dir`, by name: each `.md` file in it.
fn load(dir: &Path) -> Vec<Agent> {
    let mut agents: Vec<Agent> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.strip_suffix(".md")?;
            if !summonable(name) {
                return None;
            }
            let text = std::fs::read_to_string(&path).ok()?;
            let field = |key| crate::skills::field(&text, key);
            Some(Agent {
                name: name.to_string(),
                title: field("name").filter(|title| title != name),
                description: field("description"),
                model: field("model"),
                primary: field("mode").is_some_and(|mode| mode == "primary"),
                prompt: body(&text).to_string(),
                path: path.to_string_lossy().into_owned(),
            })
        })
        .collect();
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    agents
}

/// The text below the `---` block at the top of a file, or all of it when there is none.
fn body(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---") else {
        return text;
    };
    match rest.find("\n---") {
        Some(end) => rest[end + 4..]
            .split_once('\n')
            .map_or("", |(_, body)| body),
        None => text,
    }
}

/// The names of the `&name` words in a message, wherever they are, without trailing punctuation.
pub fn mentioned(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .filter_map(|word| word.strip_prefix('&'))
        .map(|name| name.trim_end_matches(['.', ',', '!', '?', ';', ':', ')']))
}

/// `wanted`, a model an agent names, as the one of `ids` a CLI offers: the whole name, or the
/// model alone when it is written `provider/model`.
pub fn offered<'a>(wanted: &str, mut ids: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let bare = wanted.rsplit_once('/').map_or(wanted, |(_, bare)| bare);
    ids.find(|id| *id == wanted || *id == bare)
}

impl Agent {
    /// What the agent is for, which a CLI reads to pick it as a subagent.
    pub fn about(&self) -> &str {
        self.description
            .as_deref()
            .or(self.title.as_deref())
            .unwrap_or(&self.name)
    }
}

/// Writes `content` to `name` in a folder of the config folder that `cli` reads at launch for
/// the project at `root`, unless the file holds it already, and returns the file's path.
pub fn launch_file(cli: &str, root: &str, name: &str, content: &str) -> std::io::Result<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hasher);
    let dir = crate::skills::config_dir()
        .ok_or_else(|| std::io::Error::other("no config folder"))?
        .join(format!("{cli}-agents"))
        .join(format!("{:016x}", hasher.finish()));
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(name);
    if std::fs::read_to_string(&file).ok().as_deref() != Some(content) {
        // Written whole and then moved in, so a turn starting at the same time never reads half.
        let part = dir.join(format!("{}.part", uuid::Uuid::new_v4().simple()));
        std::fs::write(&part, content)?;
        std::fs::rename(&part, &file)?;
    }
    Ok(file)
}
