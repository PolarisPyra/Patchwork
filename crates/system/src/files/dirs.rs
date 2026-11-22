//! Patchwork's folders on the machine, split by who owns what is in them, and the move from the
//! single config folder older versions kept everything in.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DIRS: OnceLock<Dirs> = OnceLock::new();

/// The name of the released app's folders.
const RELEASE_NAME: &str = "patchwork";

/// The name of this build's folders. A debug build keeps its own, so a migration that is not
/// released yet never reaches the database the released app opens.
pub(crate) const NAME: &str = if cfg!(debug_assertions) {
    "patchwork-dev"
} else {
    RELEASE_NAME
};

/// Patchwork's folders, each named after the build inside the platform's own.
#[derive(Debug, Clone, Default)]
pub struct Dirs {
    /// What the user edits: skills, agents, hooks, MCP servers and API providers.
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
    /// The released app's folders, in a debug build, which starts from a copy of them.
    pub released: Option<Box<Dirs>>,
}

impl Dirs {
    /// Patchwork's folders inside the platform's config, local data and cache folders. macOS has
    /// one folder for config and data, and Windows one for data and cache, so the second goes in a
    /// subfolder of the first rather than among its files. The state folder is `$XDG_STATE_HOME`
    /// on Linux and the data folder elsewhere, which has none; macOS keeps logs in `~/Library/Logs`.
    #[must_use]
    pub fn new(config_home: &Path, data_home: &Path, cache_home: &Path) -> Self {
        let mut dirs = Self::named(config_home, data_home, cache_home, NAME);
        if NAME != RELEASE_NAME {
            let released = Self::named(config_home, data_home, cache_home, RELEASE_NAME);
            dirs.released = Some(Box::new(released));
        }
        dirs
    }

    fn named(config_home: &Path, data_home: &Path, cache_home: &Path, name: &str) -> Self {
        let config = config_home.join(name);
        let mut data = data_home.join(name);
        if data == config {
            data = config.join("data");
        }
        let mut cache = cache_home.join(name);
        if cache == data {
            cache = data.join("cache");
        }
        let state = if cfg!(target_os = "linux") {
            std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .or_else(|| super::paths::home_dir().map(|home| home.join(".local/state")))
                .map_or_else(|| data.clone(), |home| home.join(name))
        } else {
            data.clone()
        };
        let logs = super::paths::home_dir()
            .filter(|_| cfg!(target_os = "macos"))
            .map_or_else(
                || state.join("logs"),
                |home| home.join("Library/Logs").join(name),
            );
        Self {
            config,
            data,
            cache,
            state,
            logs,
            released: None,
        }
    }

    /// Copies the released app's config folder to this build's, when this one has none yet.
    /// True when it copied.
    ///
    /// # Errors
    ///
    /// Returns an error when a file cannot be read or written.
    pub fn copy_config_from(&self, released: &Dirs) -> std::io::Result<bool> {
        if self.config.exists() || !released.config.is_dir() {
            return Ok(false);
        }
        copy_dir(&released.config, &self.config)?;
        Ok(true)
    }

    /// The folder each CLI is launched with: its copies of the enabled skills, its agents, and
    /// Claude's plugin hooks.
    #[must_use]
    pub fn launch(&self, cli: &str) -> PathBuf {
        self.cache.join("launch").join(cli)
    }
}

/// Copies the folder `from` to `to`, with everything in it.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Sets the folders; called once when the core opens.
pub fn init(dirs: Dirs) {
    let _ = DIRS.set(dirs);
}

/// Patchwork's folders, once `init` has run.
pub fn dirs() -> &'static Dirs {
    DIRS.get_or_init(Dirs::default)
}
