//! Custom agents: Markdown files in `agents` in the config folder, each a role an agent takes on
//! when a message summons it as `&name`. The name is the file's name without `.md`; the
//! frontmatter gives a description, and the text below it is the role's instructions.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub name: String,
    pub description: Option<String>,
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

    /// The message with the role of the first agent it summons as `&name` after it, or the
    /// message as it is when it summons none. The role goes last, so a message that starts with
    /// a command still does.
    pub fn summon(&self, text: String) -> String {
        let agents = self.list();
        let Some(agent) = mentioned(&text).find_map(|name| agents.iter().find(|a| a.name == name))
        else {
            return text;
        };
        format!(
            "{text}\n\n<agent name=\"{name}\">\nFor this message, act as the {name} agent and follow these instructions:\n\n{prompt}\n</agent>",
            name = agent.name,
            prompt = agent.prompt.trim(),
        )
    }
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
            let (_, description) = crate::skills::frontmatter(&text);
            Some(Agent {
                name: name.to_string(),
                description,
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
