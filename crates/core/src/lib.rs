//! Patchwork's backend. The desktop app calls these methods and receives
//! agent events from them.

// Rust guideline compliant 2026-10-09

#[doc(inline)]
pub use patchwork_system::VERSION;

pub mod android;
pub mod backgrounds;
pub mod bots;
pub mod checks;
pub mod computer;
pub mod events;
pub mod extensions;
pub mod gateway;
pub mod harness;
pub mod import;
pub mod logging;
pub mod notifications;
pub mod orchestration;
pub mod permissions;
pub mod persistence;
pub mod projects;
pub mod provider;
pub mod scheduled;
pub mod secrets;
pub mod settings;
pub mod source_control;
pub mod terminal;
pub mod themes;
pub mod threads;
pub mod updates;
pub mod usage;
pub mod vault;
pub mod workspace;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use orchestration::Runner;
use extensions::{agents, hooks, skills};
use persistence::Store;
use workspace::watch;

use crate::events::AgentEventEnvelope;

#[doc(inline)]
pub use patchwork_system as system;
pub use patchwork_system::Error;

/// Receives every agent event, e.g. to forward it to the UI.
pub type EventSink = Box<dyn Fn(AgentEventEnvelope) + Send + Sync>;

pub struct Core {
    pub(crate) store: Store,
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
    /// One folder per bot, which holds the files it knows.
    bots_dir: PathBuf,
    sink: EventSink,
    /// The watcher on the active project, with that project's id.
    project_watch: std::sync::Mutex<Option<(String, watch::ProjectWatcher)>>,
    /// The watcher on the skills folder, while Settings shows the skills.
    skills_watch: std::sync::Mutex<Option<skills::Watcher>>,
    themes_watch: std::sync::Mutex<Option<themes::Watcher>>,
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
    /// The chat apps connected to Patchwork's threads.
    gateway: gateway::Hub,
    /// Whether the person is at Patchwork, for routing notifications.
    presence: notifications::Presence,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core").finish_non_exhaustive()
    }
}

impl Core {
    /// `dirs` are Patchwork's folders (see `system::dirs`); `data_dir` is the app's own folder,
    /// which holds chat scratch folders.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or read, or when recovering the turns an
    /// earlier run left unfinished fails.
    pub fn open(dirs: &system::Dirs, data_dir: &Path, sink: EventSink) -> Result<Arc<Self>, Error> {
        system::dirs::init(dirs.clone());
        system::ssh::set_helper_source(updates::release_helper);
        // A debug build's folders start as a copy of the released app's, on its first launch.
        let mut not_copied = Vec::new();
        if let Some(released) = &dirs.released {
            if let Err(e) = dirs.copy_config_from(released) {
                not_copied.push(format!("config: {e}"));
            }
            if let Err(e) = Store::copy(&released.data, &dirs.data) {
                not_copied.push(format!("database: {e}"));
            }
        }
        crate::provider::api::config::init(&dirs.config);
        skills::init(&dirs.config);
        hooks::init(&dirs.config);
        themes::init();
        let agents = agents::Library::open(&dirs.config);
        std::thread::spawn(system::scope::prepare);
        // Read at launch, so the first command does not wait on the shell.
        std::thread::spawn(system::login_env);
        // Ends what terminals left running when a Patchwork before this one was killed.
        std::thread::spawn(terminal::sweep);
        secrets::keep_swept();
        let logs = logging::session_dir(&dirs.logs);
        let store = Store::open(&dirs.data)?;
        settings::follow(&store.settings());
        logging::start(&logs, &store.settings().logging);
        logging::system::record(&logs);
        vault::open(&dirs.data, &dirs.config);
        settings::unseal(&store)?;
        // Starts the MCP servers, which read their saved values: after the vault is open.
        provider::mcp::init(&dirs.config);
        for what in not_copied {
            log::warn!(what:% = what; "could not copy the released app's data: {{what}}");
        }
        // Beside a running Patchwork, what looks left behind is that one's live work.
        if store.owns_data() {
            orchestration::turn::recovery::recover(&store)?;
        }
        let core = Arc::new(Self {
            store,
            usage: usage::UsageCache::default(),
            runner: Runner::default(),
            chats_dir: data_dir.join("chats"),
            pasted_dir: data_dir.join("pasted"),
            backgrounds_dir: data_dir.join("backgrounds"),
            installers_dir: data_dir.join("installers"),
            worktrees_dir: data_dir.join("worktrees"),
            bots_dir: data_dir.join("bots"),
            sink,
            project_watch: std::sync::Mutex::default(),
            skills_watch: std::sync::Mutex::default(),
            themes_watch: std::sync::Mutex::default(),
            agents,
            terminals: terminal::Terminals::default(),
            secrets: secrets::Waiters::default(),
            updates: updates::Updates::default(),
            emulators: android::Emulators::default(),
            gateway: gateway::Hub::default(),
            presence: notifications::Presence::default(),
        });
        core.purge_removed_backgrounds();
        let weak = Arc::downgrade(&core);
        provider::mcp::set_hook_handler(Box::new(move |request| match weak.upgrade() {
            Some(core) => core.runner.hook_request(&core, request),
            None => serde_json::json!({"deny": "Patchwork is closing"}),
        }));
        let weak = Arc::downgrade(&core);
        provider::mcp::set_tool_handler(Box::new(move |thread_id, tool, args, stopped| match weak
            .upgrade()
        {
            // A chat app's dispatcher has Patchwork's capabilities and nothing else.
            Some(core) if core.is_dispatcher(thread_id) => {
                core.run_capability(thread_id, tool, args, stopped)
            }
            Some(core) if tool == secrets::TOOL => {
                core.runner.request_secret(&core, thread_id, args, stopped)
            }
            Some(core) if tool == threads::delegate::TOOL => {
                core.runner.run_sub_agent(&core, thread_id, args, stopped)
            }
            Some(core) if tool == threads::delegate::RESULT => core
                .runner
                .sub_agent_result(&core, thread_id, args, stopped),
            Some(core) if threads::chat_tools::is_tool(tool) => core
                .runner
                .run_chat_tool(&core, thread_id, tool, args, stopped),
            Some(core) if tool.starts_with(android::TOOL_PREFIX) => {
                core.android_tool(thread_id, tool, args, stopped)
            }
            Some(_) => Err(format!("Patchwork has no tool {tool}").into()),
            None => Err("Patchwork is closing".into()),
        }));
        let weak = Arc::downgrade(&core);
        provider::mcp::set_tool_lister(Box::new(move |thread_id| {
            weak.upgrade()
                .map(|core| {
                    // A chat app's dispatcher has Patchwork's capabilities and nothing else.
                    if core.is_dispatcher(thread_id) {
                        return orchestration::capabilities::tools();
                    }
                    let mut tools = vec![serde_json::json!({
                        "name": secrets::TOOL,
                        "description": secrets::DESCRIPTION,
                        "inputSchema": secrets::input_schema(),
                    })];
                    tools.extend(core.chat_tools(thread_id));
                    tools.extend(core.delegate_tools(thread_id));
                    tools.extend(core.android_tools(thread_id));
                    tools
                })
                .unwrap_or_default()
        }));
        // Only the Patchwork that owns the data answers chat apps, so two never both reply.
        if core.store.owns_data() {
            let starting = Arc::clone(&core);
            std::thread::spawn(move || starting.restart_gateway());
        }
        std::thread::spawn(vault::take_old);
        core.lock_when_idle();
        Ok(core)
    }

    fn emit(&self, event: AgentEventEnvelope) {
        orchestration::log_event(&event);
        self.gateway.observe(&event);
        self.notify_desktop(&event.thread_id, &event.event);
        (self.sink)(event);
    }
}
