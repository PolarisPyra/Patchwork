//! Patchwork's folders on the machine, split by who owns what is in them, and the move from the
//! single config folder older versions kept everything in.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DIRS: OnceLock<Dirs> = OnceLock::new();

/// Patchwork's folders, each named `patchwork` inside the platform's own.
#[derive(Debug, Clone, Default)]
pub struct Dirs {
    /// What the user edits: skills, agents, plugins, MCP servers and API providers.
    pub config: PathBuf,
    /// What Patchwork keeps: the database and the API providers' conversations.
    pub data: PathBuf,
    /// What Patchwork rebuilds when it is gone: the files each CLI is launched with, fetched lists,
    /// and screenshots.
    pub cache: PathBuf,
    /// The relay file, which only matters while this machine runs Patchwork.
    pub state: PathBuf,
    /// One folder of logs per launch.
    pub logs: PathBuf,
}

/// The database and the files SQLite keeps beside it, which move together or not at all.
const DATABASE: [&str; 3] = ["patchwork.db", "patchwork.db-wal", "patchwork.db-shm"];

/// What older versions left in the config folder that nothing reads now: the files handed to the
/// CLIs, rebuilt at each launch in the cache, and the leftovers of versions before those.
const LEFT_BEHIND: [&str; 10] = [
    "claude",
    "codex",
    "opencode",
    "antigravity-hooks",
    "claude-agents",
    "codex-agents",
    "claude-plugin",
    "settings.json.bak",
    "threads.json.bak",
    "hooks.log",
];

/// The relay file, which each start rewrites in the state folder.
const RELAY_FILE: &str = "mcp-relay.json";

impl Dirs {
    /// Patchwork's folders inside the platform's config, local data and cache folders. macOS has
    /// one folder for config and data, and Windows one for data and cache, so the second goes in a
    /// subfolder of the first rather than among its files. The state folder is `$XDG_STATE_HOME`
    /// on Linux and the data folder elsewhere, which has none; macOS keeps logs in `~/Library/Logs`.
    pub fn new(config_home: &Path, data_home: &Path, cache_home: &Path) -> Self {
        let config = config_home.join("patchwork");
        let mut data = data_home.join("patchwork");
        if data == config {
            data = config.join("data");
        }
        let mut cache = cache_home.join("patchwork");
        if cache == data {
            cache = data.join("cache");
        }
        let state = if cfg!(target_os = "linux") {
            std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .or_else(|| super::fs::home_dir().map(|home| home.join(".local/state")))
                .map_or_else(|| data.clone(), |home| home.join("patchwork"))
        } else {
            data.clone()
        };
        let logs = super::fs::home_dir()
            .filter(|_| cfg!(target_os = "macos"))
            .map_or_else(
                || state.join("logs"),
                |home| home.join("Library/Logs/patchwork"),
            );
        Self {
            config,
            data,
            cache,
            state,
            logs,
        }
    }

    /// The folder each CLI is launched with: its copies of the enabled skills, its agents, and
    /// Claude's plugin hooks.
    pub fn launch(&self, cli: &str) -> PathBuf {
        self.cache.join("launch").join(cli)
    }

    /// Moves what older versions kept in the config folder to the folder it belongs in now, and
    /// deletes what nothing reads any more. Something already in its new place stays and the old
    /// copy is left. Returns what could not be moved, to log once the logger runs.
    pub fn adopt_old_layout(&self) -> Vec<String> {
        let old = &self.config;
        let mut failed = Vec::new();
        if !self.data.join(DATABASE[0]).exists() && old.join(DATABASE[0]).exists() {
            for name in DATABASE {
                if let Err(e) = move_file(&old.join(name), &self.data.join(name)) {
                    failed.push(format!("{name}: {e}"));
                }
            }
        }
        let moves = [
            ("api-sessions", self.data.join("api-sessions")),
            ("mcp-servers.json", self.config.join("mcp.json")),
            ("api-providers.json", self.config.join("providers.json")),
            (
                "model-manifest.json",
                self.cache.join("model-manifest.json"),
            ),
            ("mcp-cli-tools.json", self.cache.join("mcp-cli-tools.json")),
            ("screenshots", self.cache.join("screenshots")),
            ("logs", self.logs.clone()),
        ];
        for (name, to) in moves {
            let from = old.join(name);
            if from.exists()
                && !to.exists()
                && let Err(e) = move_path(&from, &to)
            {
                failed.push(format!("{name}: {e}"));
            }
        }
        for name in LEFT_BEHIND {
            let path = old.join(name);
            let _ = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
        }
        // Where the state folder is the config folder, the file there is the live one.
        if self.state != self.config {
            let _ = std::fs::remove_file(old.join(RELAY_FILE));
        }
        failed
    }
}

/// Moves a file, copying it when `to` is on another drive.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.exists() {
        return Ok(());
    }
    move_path(from, to).or_else(|_| {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)
    })
}

fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(from, to)
}

/// Sets the folders; called once when the server opens.
pub fn init(dirs: Dirs) {
    let _ = DIRS.set(dirs);
}

/// Patchwork's folders, once `init` has run.
pub fn dirs() -> &'static Dirs {
    DIRS.get_or_init(Dirs::default)
}
