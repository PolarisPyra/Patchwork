//! Patchwork's backend. The desktop app calls these methods and receives
//! agent events from them.

// Rust guideline compliant 2026-10-09

pub mod android;
pub mod quilts;
pub mod backgrounds;
pub mod agents;
pub mod browser;
pub mod checks;
pub mod computer;
pub mod costs;
pub mod events;
pub mod extensions;
pub mod gateway;
pub mod harness;
pub mod import;
pub mod issues;
pub mod logging;
pub mod notifications;
pub mod orchestration;
pub mod permissions;
pub mod persistence;
pub mod projects;
pub mod provider;
pub mod scheduled;
pub mod service;
pub mod settings;
pub mod source_control;
pub mod terminal;
pub mod themes;
pub mod chats;
pub mod updates;
pub mod usage;
pub mod vault;
pub mod worktree;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use orchestration::Runner;
use extensions::{hooks, skills};
use persistence::Store;

use crate::events::{ChatEventEnvelope, ToWindow, WindowCommand};

use patchwork_platform as system;
pub use patchwork_platform::Error;

/// Receives what the core sends the window: chat events and window commands.
pub type WindowSink = Box<dyn Fn(ToWindow) + Send + Sync>;

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
    /// The tasks whose worktree a merge, update, discard or branch switch is working on, so no
    /// turn starts in it and no second step runs at the same time.
    worktree_steps: std::sync::Mutex<std::collections::HashSet<String>>,
    /// The replies whose checks are running. Two runs for one reply would race each other for
    /// its card, and the one that finished first could be what it is left showing.
    checks_running: std::sync::Mutex<std::collections::HashSet<String>>,
    /// One folder per agent, which holds the files it knows.
    agents_dir: PathBuf,
    /// The window's end: every chat event and every window command reaches it.
    sink: Arc<dyn Fn(ToWindow) + Send + Sync>,
    /// The watcher on the folder Source Control shows for the active project.
    project_watch: std::sync::Mutex<Option<projects::ProjectWatch>>,
    /// The watcher on the skills folder, while Settings shows the skills.
    skills_watch: std::sync::Mutex<Option<skills::Watcher>>,
    themes_watch: std::sync::Mutex<Option<themes::Watcher>>,
    /// The custom agents that `&name` summons.
    pub agents: Arc<extensions::agents::Library>,
    /// The integrated terminal's running shells.
    pub terminals: terminal::Terminals,
    /// Secret requests waiting for the user.
    secrets: vault::requests::Waiters,
    /// Where updating the app stands.
    updates: Arc<updates::Updates>,
    /// The Android emulators Patchwork started.
    emulators: crate::android::Emulators,
    /// The chat apps connected to Patchwork's chats.
    gateway: gateway::Hub,
    /// Whether the person is at Patchwork, for routing notifications.
    presence: notifications::Presence,
    /// This core, for the settings changes that start work which outlives the call.
    me: std::sync::Weak<Core>,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core").finish_non_exhaustive()
    }
}

impl Core {
    /// `dirs` are Patchwork's folders (see `system::files::dirs`); `data_dir` is the app's own folder,
    /// which holds chat scratch folders.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or read, or when recovering the turns an
    /// earlier run left unfinished fails.
    pub fn open(
        dirs: &system::Dirs,
        data_dir: &Path,
        sink: WindowSink,
    ) -> Result<Arc<Self>, Error> {
        system::files::dirs::init(dirs.clone());
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
        crate::provider::api::init(&dirs.config);
        skills::init(&dirs.config);
        hooks::init(&dirs.config);
        themes::init();
        let agents = extensions::agents::Library::open(&dirs.config);
        std::thread::spawn(system::processes::scope::prepare);
        // Read at launch, so the first command does not wait on the shell.
        std::thread::spawn(system::login_env);
        // Ends what terminals left running when a Patchwork before this one was killed.
        std::thread::spawn(terminal::sweep);
        vault::requests::keep_swept();
        let logs = logging::session_dir(&dirs.logs);
        let store = Store::open(&dirs.data)?;
        settings::follow(&store.settings());
        logging::start(&logs, &store.settings().logging);
        let (mcp_servers, mcp_on, mcp_shared) = provider::mcp::counts(&dirs.config);
        logging::system::record(
            &logs,
            logging::system::Extensions {
                agents: agents.list().len(),
                hooks: extensions::hooks::agent_hooks(&store.settings()).len(),
                mcp_servers,
                mcp_on,
                mcp_shared,
            },
        );
        vault::open(
            &dirs.data,
            &dirs.config,
            store.settings().vault.timeout == vault::VaultTimeout::Never,
        );
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
        let core = Arc::new_cyclic(|me| Self {
            store,
            usage: usage::UsageCache::default(),
            runner: Runner::default(),
            chats_dir: data_dir.join("chats"),
            pasted_dir: data_dir.join("pasted"),
            backgrounds_dir: data_dir.join("backgrounds"),
            installers_dir: data_dir.join("installers"),
            worktrees_dir: data_dir.join("worktrees"),
            worktree_steps: std::sync::Mutex::default(),
            checks_running: std::sync::Mutex::default(),
            agents_dir: agents::agents_folder(data_dir),
            sink: Arc::from(sink),
            project_watch: std::sync::Mutex::default(),
            skills_watch: std::sync::Mutex::default(),
            themes_watch: std::sync::Mutex::default(),
            agents,
            terminals: terminal::Terminals::default(),
            secrets: vault::requests::Waiters::default(),
            updates: Arc::default(),
            emulators: crate::android::Emulators::default(),
            gateway: gateway::Hub::default(),
            presence: notifications::Presence::default(),
            me: std::sync::Weak::clone(me),
        });
        core.purge_removed_backgrounds();
        core.install_handlers();
        // Only the Patchwork that owns the data answers chat apps, so two never both reply.
        if core.store.owns_data() {
            let starting = Arc::clone(&core);
            std::thread::spawn(move || starting.restart_gateway());
        }
        std::thread::spawn(vault::take_old);
        core.lock_when_idle();
        Ok(core)
    }

    /// Installs, once, what answers the requests that reach the core from outside its calls:
    /// the relay's hooks and tools, a second launch, git's sign-in questions, and update changes.
    /// Those that only tell the window something become window commands.
    fn install_handlers(self: &Arc<Self>) {
        let hook = Arc::downgrade(self);
        let tool = Arc::downgrade(self);
        let list = Arc::downgrade(self);
        let raise = Arc::clone(&self.sink);
        provider::mcp::install(provider::mcp::Handlers {
            hook: Box::new(move |request| match hook.upgrade() {
                Some(core) => core.runner.hook_request(&core, request),
                None => serde_json::json!({"deny": "Patchwork is closing"}),
            }),
            tool: Box::new(move |chat_id, name, args, stopped| match tool.upgrade() {
                Some(core) => core.run_own_tool(chat_id, name, args, stopped),
                None => Err("Patchwork is closing".into()),
            }),
            list_tools: Box::new(move |chat_id| {
                list.upgrade()
                    .map(|core| core.own_tools(chat_id))
                    .unwrap_or_default()
            }),
            raise: Box::new(move || raise(ToWindow::Command(WindowCommand::Raise))),
        });
        // Git asks for a sign-in on the thread that runs it; the question is shown by the window.
        // With no window left to ask in, the request drops and the command fails.
        let ask = Arc::clone(&self.sink);
        source_control::set_relay_endpoint_provider(provider::mcp::relay_endpoint);
        source_control::set_credential_prompter(Box::new(move |request| {
            ask(ToWindow::Command(WindowCommand::AskCredentials(request)));
        }));
        let redraw = Arc::clone(&self.sink);
        self.updates.set_listener(Box::new(move || {
            redraw(ToWindow::Command(WindowCommand::RedrawUpdate));
        }));
    }

    /// Runs Patchwork's own tool `tool` for the agent of `chat_id`, until `stopped`.
    fn run_own_tool(
        self: &Arc<Self>,
        chat_id: &str,
        tool: &str,
        args: &serde_json::Value,
        stopped: &dyn Fn() -> bool,
    ) -> Result<String, Error> {
        match tool {
            // A chat app's dispatcher has Patchwork's capabilities and nothing else.
            _ if self.is_dispatcher(chat_id) => self.run_capability(chat_id, tool, args, stopped),
            vault::requests::TOOL => self.runner.request_secret(self, chat_id, args, stopped),
            chats::delegate::TOOL => self.runner.run_sub_agent(self, chat_id, args, stopped),
            chats::delegate::RESULT => self.runner.sub_agent_result(self, chat_id, args, stopped),
            _ if chats::tools::is_tool(tool) => self
                .runner
                .run_chat_tool(self, chat_id, tool, args, stopped),
            quilts::TOOL => self.run_quilt_tool(chat_id, args),
            _ if tool.starts_with(crate::android::TOOL_PREFIX) => {
                self.android().tool(chat_id, tool, args, stopped)
            }
            _ => Err(format!("Patchwork has no tool {tool}").into()),
        }
    }

    /// The tools of Patchwork's own that the agent of `chat_id` has, as `tools/list` entries.
    fn own_tools(&self, chat_id: &str) -> Vec<serde_json::Value> {
        // A chat app's dispatcher has Patchwork's capabilities and nothing else.
        if self.is_dispatcher(chat_id) {
            return orchestration::capabilities::tools();
        }
        let mut tools = vec![serde_json::json!({
            "name": vault::requests::TOOL,
            "description": vault::requests::DESCRIPTION,
            "inputSchema": vault::requests::input_schema(),
        })];
        tools.extend(self.chat_tools(chat_id));
        tools.extend(self.delegate_tools(chat_id));
        tools.extend(self.android().tools(chat_id));
        tools.extend(self.quilt_tools(chat_id));
        tools
    }

    /// Tells every observer of a chat event: the log, chat apps, desktop notifications, and the
    /// window.
    fn emit(&self, event: ChatEventEnvelope) {
        orchestration::log_event(&event);
        self.gateway.observe(&event);
        self.notify_desktop(&event.chat_id, &event.event);
        (self.sink)(ToWindow::Event(event));
    }

    /// Asks the window to do something. Only the window hears of it, and the log.
    pub(crate) fn command(&self, command: WindowCommand) {
        orchestration::log_command(&command);
        (self.sink)(ToWindow::Command(command));
    }
}

#[cfg(test)]
impl Core {
    /// A core over a database in `dir`, without the globals `open` starts.
    pub(crate) fn for_tests(dir: &Path, sink: WindowSink) -> Arc<Self> {
        let data_dir = dir.join("data");
        let store = Store::open(&data_dir).expect("a test database opens");
        let core = Arc::new_cyclic(|me| Self {
            store,
            usage: usage::UsageCache::default(),
            runner: Runner::default(),
            chats_dir: data_dir.join("chats"),
            pasted_dir: data_dir.join("pasted"),
            backgrounds_dir: data_dir.join("backgrounds"),
            installers_dir: data_dir.join("installers"),
            worktrees_dir: data_dir.join("worktrees"),
            worktree_steps: std::sync::Mutex::default(),
            checks_running: std::sync::Mutex::default(),
            agents_dir: agents::agents_folder(&data_dir),
            sink: Arc::from(sink),
            project_watch: std::sync::Mutex::default(),
            skills_watch: std::sync::Mutex::default(),
            themes_watch: std::sync::Mutex::default(),
            agents: extensions::agents::Library::open(&dir.join("config")),
            terminals: terminal::Terminals::default(),
            secrets: vault::requests::Waiters::default(),
            updates: Arc::default(),
            emulators: crate::android::Emulators::default(),
            gateway: gateway::Hub::default(),
            presence: notifications::Presence::default(),
            me: std::sync::Weak::clone(me),
        });
        // A focused window keeps the tests from showing desktop notifications.
        core.set_window_focused(true);
        core
    }
}

/// What runs for a helper role's first argument, returning the exit code. None when `arg` is
/// not one.
#[must_use]
pub fn helper_role(arg: &str) -> Option<fn() -> i32> {
    Some(match arg {
        // An OpenCode turn's bridge to `opencode serve`.
        provider::opencode::bridge::BRIDGE_ARG => provider::opencode::bridge::run,
        // An API provider's turn, run as Patchwork's own agent.
        harness::BRIDGE_ARG => harness::run,
        // Checks a tool call against the permission rules.
        permissions::hook::HOOK_ARG => permissions::hook::run,
        // Antigravity's session start, for the system prompt from Settings.
        provider::hooks::PROMPT_ARG => provider::hooks::run_prompt_hook,
        // Claude Code's and Codex's relay to an MCP server Patchwork runs.
        provider::mcp::PROXY_ARG => provider::mcp::run_proxy,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_role_argument_is_known() {
        for arg in [
            provider::opencode::bridge::BRIDGE_ARG,
            harness::BRIDGE_ARG,
            permissions::hook::HOOK_ARG,
            provider::hooks::PROMPT_ARG,
            provider::mcp::PROXY_ARG,
        ] {
            assert!(helper_role(arg).is_some(), "{arg} has no role");
        }
        assert!(helper_role("--not-a-role").is_none());
    }
}
