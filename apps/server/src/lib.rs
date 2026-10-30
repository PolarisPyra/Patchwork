//! Patchwork's backend. The desktop shell exposes these methods as Tauri
//! commands and forwards agent events to the UI.

/// The app's version, from `tauri.conf.json` (see build.rs).
pub const VERSION: &str = env!("PATCHWORK_VERSION");

pub mod agents;
pub mod computer;
pub mod error;
pub mod import;
pub mod logging;
pub mod orchestration;
pub mod permissions;
pub mod persistence;
pub mod plugins;
pub mod projects;
pub mod provider;
pub mod scheduled;
pub mod settings;
pub mod skills;
pub mod system;
pub mod terminal;
pub mod threads;
pub mod usage;
pub mod watch;
pub mod workspace;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use orchestration::Runner;
use persistence::Store;

use crate::orchestration::AgentEventEnvelope;
use crate::settings::{Settings, LogLevel};

pub use error::Error;

pub type Res<T> = Result<T, String>;

/// Sets how much each log records. The desktop shell installs the logger of the app's own.
fn follow_log_levels(settings: &Settings) {
    let levels = &settings.logging;
    logging::AGENTS.set_level(levels.agents);
    logging::RENDERER.set_level(levels.window);
    import::debug_log::LOG.set_level(levels.imports);
    logging::set_app_level(levels.app);
    log::set_max_level(match levels.app {
        LogLevel::Off => log::LevelFilter::Off,
        LogLevel::Error => log::LevelFilter::Error,
        LogLevel::Warning => log::LevelFilter::Warn,
        LogLevel::Info => log::LevelFilter::Info,
        LogLevel::Debug => log::LevelFilter::Debug,
        LogLevel::Trace => log::LevelFilter::Trace,
    });
}

/// Receives every agent event, e.g. to forward it to the UI.
pub type EventSink = Box<dyn Fn(AgentEventEnvelope) + Send + Sync>;

pub struct Server {
    pub store: Store,
    pub usage: usage::UsageCache,
    runner: Runner,
    /// Parent of the scratch folders that project-less chats run in.
    chats_dir: PathBuf,
    /// Images pasted into a chat, saved as files so they attach like dropped ones.
    pasted_dir: PathBuf,
    /// Where an agent CLI's install script is saved while it runs.
    installers_dir: PathBuf,
    sink: EventSink,
    /// The watcher on the active project, with that project's id.
    project_watch: std::sync::Mutex<Option<(String, watch::ProjectWatcher)>>,
    /// The watcher on the skills folder, while Settings shows the skills.
    skills_watch: std::sync::Mutex<Option<skills::Watcher>>,
    /// The custom agents that `&name` summons.
    pub agents: Arc<agents::Library>,
}

impl Server {
    /// `dirs` are Patchwork's folders (see `system::dirs`); `data_dir` is the app's own folder,
    /// which holds chat scratch folders.
    pub fn open(dirs: system::Dirs, data_dir: &Path, sink: EventSink) -> Res<Arc<Self>> {
        system::dirs::init(dirs.clone());
        provider::model_manifest::start(&dirs.cache);
        crate::provider::api::config::init(&dirs.config);
        provider::mcp::init(&dirs.config);
        skills::init(&dirs.config);
        plugins::init(&dirs.config);
        let agents = agents::Library::open(&dirs.config);
        std::thread::spawn(system::scope::prepare);
        let logs = logging::session_dir(&dirs.logs);
        import::debug_log::LOG.init(logs.clone());
        logging::RENDERER.init(logs.clone());
        logging::AGENTS.init(logs);
        let store = Store::open(dirs.data)?;
        follow_log_levels(&store.settings());
        orchestration::turn::recovery::recover(&store)?;
        let server = Arc::new(Self {
            store,
            usage: usage::UsageCache::default(),
            runner: Runner::default(),
            chats_dir: data_dir.join("chats"),
            pasted_dir: data_dir.join("pasted"),
            installers_dir: data_dir.join("installers"),
            sink,
            project_watch: std::sync::Mutex::default(),
            skills_watch: std::sync::Mutex::default(),
            agents,
        });
        let weak = Arc::downgrade(&server);
        provider::mcp::set_hook_handler(Box::new(move |request| match weak.upgrade() {
            Some(server) => server.runner.hook_request(&server, request),
            None => serde_json::json!({"deny": "Patchwork is closing"}),
        }));
        Ok(server)
    }

    fn emit(&self, event: AgentEventEnvelope) {
        (self.sink)(event);
    }
}
