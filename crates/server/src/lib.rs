//! Patchwork's backend. The desktop app calls these methods and receives
//! agent events from them.

// Rust guideline compliant 2026-10-09

/// The app's version, the workspace's in `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod agents;
pub mod android;
pub mod backgrounds;
pub mod checks;
pub mod computer;
mod error;
pub mod events;
pub mod git;
pub mod harness;
pub mod import;
pub mod logging;
pub mod orchestration;
pub mod permissions;
pub mod persistence;
pub mod hooks;
pub mod projects;
pub mod provider;
pub mod scheduled;
pub mod secrets;
pub mod settings;
pub mod skills;
pub mod system;
pub mod terminal;
pub mod threads;
pub mod updates;
pub mod usage;
pub mod watch;
pub mod workspace;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use orchestration::Runner;
use persistence::Store;

use crate::events::AgentEventEnvelope;

#[doc(inline)]
pub use error::Error;

/// Receives every agent event, e.g. to forward it to the UI.
pub type EventSink = Box<dyn Fn(AgentEventEnvelope) + Send + Sync>;

pub struct Server {
    pub store: Store,
    pub usage: usage::UsageCache,
    runner: Runner,
    /// Parent of the scratch folders that project-less chats run in.
    chats_dir: PathBuf,
    /// Pictures pasted into a message, which exist nowhere else.
    pasted_dir: PathBuf,
    /// Pictures picked as chat backgrounds, copied so they stay when the originals move.
    backgrounds_dir: PathBuf,
    /// Where an agent CLI's install script is saved while it runs.
    installers_dir: PathBuf,
    /// The git worktrees tasks work in apart from their project, one folder each.
    worktrees_dir: PathBuf,
    sink: EventSink,
    /// The watcher on the active project, with that project's id.
    project_watch: std::sync::Mutex<Option<(String, watch::ProjectWatcher)>>,
    /// The watcher on the skills folder, while Settings shows the skills.
    skills_watch: std::sync::Mutex<Option<skills::Watcher>>,
    /// The custom agents that `&name` summons.
    pub agents: Arc<agents::Library>,
    /// The integrated terminal's running shells.
    pub terminals: terminal::Terminals,
    /// Secret requests waiting for the user.
    secrets: secrets::Waiters,
    /// Where updating the app stands.
    updates: updates::Updates,
    /// The Android emulators Patchwork started.
    emulators: android::Emulators,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server").finish_non_exhaustive()
    }
}

impl Server {
    /// `dirs` are Patchwork's folders (see `system::dirs`); `data_dir` is the app's own folder,
    /// which holds chat scratch folders.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or read, or when recovering the turns an
    /// earlier run left unfinished fails.
    pub fn open(dirs: &system::Dirs, data_dir: &Path, sink: EventSink) -> Result<Arc<Self>, Error> {
        system::dirs::init(dirs.clone());
        crate::provider::api::config::init(&dirs.config);
        provider::mcp::init(&dirs.config);
        skills::init(&dirs.config);
        hooks::init(&dirs.config);
        let agents = agents::Library::open(&dirs.config);
        std::thread::spawn(system::scope::prepare);
        // Read at launch, so the first command does not wait on the shell.
        std::thread::spawn(system::login_env);
        // Ends what terminals left running when a Patchwork before this one was killed.
        std::thread::spawn(terminal::sweep);
        secrets::keep_swept();
        let logs = logging::session_dir(&dirs.logs);
        let store = Store::open(&dirs.data)?;
        logging::start(&logs, &store.settings().logging);
        logging::system::record(&logs);
        // Beside a running Patchwork, what looks left behind is that one's live work.
        if store.owns_data() {
            orchestration::turn::recovery::recover(&store)?;
        }
        let server = Arc::new(Self {
            store,
            usage: usage::UsageCache::default(),
            runner: Runner::default(),
            chats_dir: data_dir.join("chats"),
            pasted_dir: data_dir.join("pasted"),
            backgrounds_dir: data_dir.join("backgrounds"),
            installers_dir: data_dir.join("installers"),
            worktrees_dir: data_dir.join("worktrees"),
            sink,
            project_watch: std::sync::Mutex::default(),
            skills_watch: std::sync::Mutex::default(),
            agents,
            terminals: terminal::Terminals::default(),
            secrets: secrets::Waiters::default(),
            updates: updates::Updates::default(),
            emulators: android::Emulators::default(),
        });
        server.purge_removed_backgrounds();
        let weak = Arc::downgrade(&server);
        provider::mcp::set_hook_handler(Box::new(move |request| match weak.upgrade() {
            Some(server) => server.runner.hook_request(&server, request),
            None => serde_json::json!({"deny": "Patchwork is closing"}),
        }));
        let weak = Arc::downgrade(&server);
        provider::mcp::set_tool_handler(Box::new(move |thread_id, tool, args, stopped| match weak
            .upgrade()
        {
            Some(server) if tool == secrets::TOOL => server
                .runner
                .request_secret(&server, thread_id, args, stopped),
            Some(server) if tool == threads::delegate::TOOL => server
                .runner
                .run_sub_agent(&server, thread_id, args, stopped),
            Some(server) if threads::chat_tools::is_tool(tool) => server
                .runner
                .run_chat_tool(&server, thread_id, tool, args, stopped),
            Some(_) => Err(format!("Patchwork has no tool {tool}").into()),
            None => Err("Patchwork is closing".into()),
        }));
        let weak = Arc::downgrade(&server);
        provider::mcp::set_tool_lister(Box::new(move |thread_id| {
            weak.upgrade()
                .map(|server| {
                    let mut tools = server.chat_tools(thread_id);
                    tools.extend(server.delegate_tool(thread_id));
                    tools
                })
                .unwrap_or_default()
        }));
        Ok(server)
    }

    fn emit(&self, event: AgentEventEnvelope) {
        orchestration::log_event(&event);
        (self.sink)(event);
    }
}
