//! Patchwork's backend. The desktop shell exposes these methods as Tauri
//! commands and forwards agent events to the UI.

/// The app's version, from `tauri.conf.json` (see build.rs).
pub const VERSION: &str = env!("PATCHWORK_VERSION");

pub mod agents;
pub mod computer;
pub mod debug_log;
pub mod error;
pub mod import;
pub mod launch;
pub mod orchestration;
pub mod permissions;
pub mod persistence;
pub mod plugins;
pub mod provider;
pub mod scheduled;
mod scope;
mod screen;
pub mod skills;
pub mod terminal;
mod time;
mod title;
pub mod types;
pub mod usage;
pub mod watch;
pub mod workspace;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use orchestration::{ApprovalDecision, Delivery, FollowUpInput, Runner, TurnRequest};
use persistence::Store;

use crate::import::debug_log::ilog;
use crate::types::{
    AgentKind, Project, Settings, LogLevel, Role, MessageStatus, StepStatus, AccessMode,
    ApprovalStatus, Attachment, Message, Thread, Folder, GoalStatus, ImportSource, ImportScan,
    AgentEventEnvelope, AgentEvent, now_ms, new_id,
};

pub use error::Error;

pub type Res<T> = Result<T, String>;

/// Sets how much each log records. The desktop shell installs the logger of the app's own.
fn follow_log_levels(settings: &Settings) {
    let levels = &settings.logging;
    debug_log::AGENTS.set_level(levels.agents);
    debug_log::RENDERER.set_level(levels.window);
    import::debug_log::LOG.set_level(levels.imports);
    debug_log::set_app_level(levels.app);
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

/// The options for a new task or chat, as the desktop app sends them.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewThread {
    pub project_id: Option<String>,
    pub agent: AgentKind,
    pub model: Option<String>,
    pub access_mode: AccessMode,
    pub effort: Option<String>,
}

pub struct Server {
    pub store: Store,
    pub usage: usage::UsageCache,
    runner: Runner,
    /// Parent of the scratch folders that project-less chats run in.
    chats_dir: PathBuf,
    /// Images pasted into a chat, saved as files so they attach like dropped ones.
    pasted_dir: PathBuf,
    /// Where Patchwork installs agent CLIs (`clis` under the config folder).
    clis_dir: PathBuf,
    sink: EventSink,
    /// The watcher on the active project, with that project's id.
    project_watch: std::sync::Mutex<Option<(String, watch::ProjectWatcher)>>,
    /// The watcher on the skills folder, while Settings shows the skills.
    skills_watch: std::sync::Mutex<Option<skills::Watcher>>,
    /// The custom agents that `&name` summons.
    pub agents: Arc<agents::Library>,
}

/// Trims a user-entered name; blank names are refused.
/// `app_state` key for the thread the user had open last.
const LAST_THREAD_KEY: &str = "last_thread_id";
/// `app_state` key for the projects and folders the sidebar has collapsed (JSON array).
const COLLAPSED_KEY: &str = "collapsed_sections";

/// A command for a program on the user's system. The AppImage's launcher points the library,
/// Python, GTK and data paths into the AppImage for Patchwork itself; agents, git, and the other
/// programs Patchwork starts get the user's own paths back. A copy restarted into an update
/// inherits the previous copy's paths too, so entries in any AppImage mount (`/tmp/.mount_*`)
/// are removed, not only the ones in `APPDIR`.
pub fn host_command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    hide_console(&mut cmd);
    for (key, value) in host_env_changes() {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
    #[cfg(windows)]
    if let Some(path) = path_var() {
        cmd.env("PATH", path);
    }
    cmd
}

/// The PATH to find programs on. On Windows that is the PATH as the system now has it, read from
/// the registry ahead of the one Patchwork started with: installers write the registry, and a
/// running app never sees it change, so a CLI installed after Patchwork opened would not be found
/// until it restarted. Read at most every few seconds.
pub fn path_var() -> Option<std::ffi::OsString> {
    #[cfg(windows)]
    {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};
        static CACHE: Mutex<Option<(Instant, Option<std::ffi::OsString>)>> = Mutex::new(None);
        let mut cache = CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((at, path)) = cache.as_ref()
            && at.elapsed() < Duration::from_secs(3)
        {
            return path.clone();
        }
        let path = windows_path();
        *cache = Some((Instant::now(), path.clone()));
        path
    }
    #[cfg(not(windows))]
    std::env::var_os("PATH")
}

/// Starts a console program on Windows with a console of its own that has no window. Patchwork
/// has no console, so without this every program it starts, and every program an agent starts
/// in turn, opens a console window. Their children share the hidden console.
pub fn hide_console(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// The system's PATH, then the user's, from the registry, then the one Patchwork started with.
#[cfg(windows)]
fn windows_path() -> Option<std::ffi::OsString> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let read = |root, key: &str| {
        let value: String = RegKey::predef(root)
            .open_subkey(key)
            .ok()?
            .get_value("Path")
            .ok()?;
        Some(expand_windows_vars(&value))
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for value in [
        read(
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
        read(HKEY_CURRENT_USER, "Environment"),
    ]
    .into_iter()
    .flatten()
    {
        paths.extend(std::env::split_paths(&value));
    }
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| !p.as_os_str().is_empty() && seen.insert(p.to_string_lossy().to_lowercase()));
    std::env::join_paths(paths).ok()
}

/// `%NAME%` references in a registry value, replaced with the variables' values.
#[cfg(windows)]
fn expand_windows_vars(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match std::env::var(name) {
                    Ok(v) if !name.is_empty() => out.push_str(&v),
                    _ => out.push_str(&rest[start..start + end + 2]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Patchwork's environment as `host_command` passes it on, without the AppImage's paths.
pub fn host_env() -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let changes: std::collections::HashMap<_, _> = host_env_changes().into_iter().collect();
    std::env::vars_os()
        .filter_map(|(key, value)| match changes.get(&key) {
            Some(Some(changed)) => Some((key, changed.into())),
            Some(None) => None,
            None => Some((key, value)),
        })
        .collect()
}

/// The variables `host_command` changes: each with its value without AppImage entries, or None
/// to remove it.
fn host_env_changes() -> Vec<(std::ffi::OsString, Option<String>)> {
    let appdir = std::env::var("APPDIR").ok().filter(|dir| !dir.is_empty());
    let in_appimage = |entry: &str| {
        appdir.as_deref().is_some_and(|dir| entry.starts_with(dir))
            || entry.split('/').any(|part| part.starts_with(".mount_"))
    };
    let mut changes = Vec::new();
    for (key, value) in std::env::vars_os() {
        let value = value.to_string_lossy();
        if !value.split(':').any(in_appimage) {
            continue;
        }
        let kept: Vec<&str> = value
            .split(':')
            .filter(|entry| !entry.is_empty() && !in_appimage(entry))
            .collect();
        changes.push((key, (!kept.is_empty()).then(|| kept.join(":"))));
    }
    for key in ["APPDIR", "APPIMAGE", "ARGV0", "OWD"] {
        changes.push((key.into(), None));
    }
    changes
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name can't be empty".into());
    }
    Ok(name.chars().take(120).collect())
}

/// The message without the attachments it opens with: `@name` mentions of files and images
/// written as Markdown, which the composer puts first. `is_file` says whether a bare `@name` is
/// a file; a word with a slash or a dot always is, as the composer writes paths.
pub fn without_leading_attachments(text: &str, is_file: impl Fn(&str) -> bool) -> &str {
    let mut rest = text.trim_start();
    loop {
        let end = if rest.starts_with("![") {
            rest.find("](<")
                .and_then(|open| rest[open..].find(">)").map(|close| open + close + 2))
        } else if rest.starts_with('@') {
            let token_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let word = &rest[1..token_end];
            (word.contains(['/', '.']) || is_file(word)).then_some(token_end)
        } else {
            None
        };
        match end {
            Some(end) => rest = rest[end..].trim_start(),
            None => return rest,
        }
    }
}

/// A thread title from the first line of a message that is not a code fence, without a
/// leading `/goal`.
pub fn title_from(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("```"))
        .unwrap_or("");
    let objective = if provider::is_goal_command(line) {
        line["/goal".len()..].trim()
    } else {
        line
    };
    let title = if objective.is_empty() {
        line
    } else {
        objective
    };
    title.chars().take(60).collect()
}

/// Whether a thread has gone idle just before its prompt cache expires, with a context large
/// enough to be worth compacting and nothing else pending.
/// Entries in /etc/shells that aren't shells to run agents in: git's push-only shell, and
/// systemd's placeholder for accounts without a shell.
const NOT_AGENT_SHELLS: [&str; 2] = ["git-shell", "systemd-home-fallback-shell"];

/// Shells from /etc/shells that are installed, for the shell choice in settings. Entries that
/// resolve to the same binary (such as /bin/zsh and /usr/bin/zsh) are listed once, under the
/// real path when /etc/shells has it. Windows has no such file, so its shells are looked for.
pub fn available_shells() -> Vec<String> {
    if cfg!(windows) {
        return windows_shells();
    }
    let listed: Vec<(String, PathBuf)> = std::fs::read_to_string("/etc/shells")
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('/') && Path::new(line).is_file())
        .filter(|line| {
            let name = Path::new(line).file_name().and_then(|n| n.to_str());
            !name.is_some_and(|n| NOT_AGENT_SHELLS.contains(&n))
        })
        .map(|line| {
            let real = std::fs::canonicalize(line).unwrap_or_else(|_| PathBuf::from(line));
            (line.to_string(), real)
        })
        .collect();
    listed
        .iter()
        .filter(|(path, real)| {
            let kept = listed
                .iter()
                .find(|(p, r)| r == real && Path::new(p) == real.as_path())
                .or_else(|| listed.iter().find(|(_, r)| r == real));
            kept.is_some_and(|(p, _)| p == path)
        })
        .map(|(path, _)| path.clone())
        .collect()
}

/// The login shell in the passwd database for the current user. Windows has none.
#[cfg(unix)]
pub fn login_shell() -> Option<String> {
    let mut buf = vec![0 as libc::c_char; 4096];
    // SAFETY: passwd is plain C data, so the all-zero bit pattern is a valid value; getpwuid_r
    // overwrites it before it is read.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found = std::ptr::null_mut();
    // SAFETY: entry, buf, and found are valid for the lengths passed, and getpwuid_r writes only
    // within them.
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut entry,
            buf.as_mut_ptr(),
            buf.len(),
            &mut found,
        )
    };
    if rc != 0 || found.is_null() {
        return None;
    }
    // SAFETY: the rc and null checks passed, so pw_shell points at a NUL-terminated string inside
    // buf, which is still alive here.
    let shell = unsafe { std::ffi::CStr::from_ptr(entry.pw_shell) };
    shell
        .to_str()
        .ok()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(not(unix))]
pub fn login_shell() -> Option<String> {
    None
}

/// The shells installed on Windows, best for agents first: Git Bash, since models write commands
/// for a POSIX shell, then PowerShell and cmd. The `bash.exe` in System32 is WSL's, which runs in
/// another system, so Git's is found from `git` or where Git installs.
fn windows_shells() -> Vec<String> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let find = |name: &str| which::which_in(name, path_var(), &cwd).ok();
    let mut git_bash: Vec<PathBuf> = find("git")
        .and_then(|git| Some(git.parent()?.parent()?.join("bin").join("bash.exe")))
        .into_iter()
        .collect();
    for var in ["ProgramFiles", "LOCALAPPDATA"] {
        if let Some(dir) = std::env::var_os(var) {
            let programs = if var == "LOCALAPPDATA" {
                "Programs"
            } else {
                ""
            };
            git_bash.push(PathBuf::from(dir).join(programs).join(r"Git\bin\bash.exe"));
        }
    }
    let shells = [
        git_bash.into_iter().find(|p| p.is_file()),
        find("pwsh"),
        find("powershell"),
        std::env::var_os("ComSpec")
            .map(PathBuf::from)
            .or_else(|| find("cmd")),
    ];
    shells
        .into_iter()
        .flatten()
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// The shell agents run commands in: the user's choice in Settings if it is still installed,
/// otherwise the login shell, otherwise the SHELL environment variable. On Windows, which has no
/// login shell, the best installed one comes before SHELL, which Git Bash sets to a path only it
/// can read. None when nothing is known.
pub fn effective_shell(chosen: Option<&str>) -> Option<String> {
    let installed = available_shells();
    let windows_default = cfg!(windows).then(|| installed.first().cloned()).flatten();
    chosen
        .filter(|shell| installed.iter().any(|s| s == shell))
        .map(str::to_string)
        .or_else(login_shell)
        .or(windows_default)
        .or_else(|| std::env::var("SHELL").ok().filter(|s| !s.is_empty()))
}

/// The arguments that make `shell` run `command` and exit.
pub fn shell_args(shell: &str, command: &str) -> Vec<String> {
    let name = Path::new(shell)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match name.as_str() {
        "cmd" => vec!["/C".into(), command.into()],
        "powershell" | "pwsh" => ["-NoProfile", "-NonInteractive", "-Command", command]
            .map(String::from)
            .into(),
        _ => vec!["-c".into(), command.into()],
    }
}

/// `thread` with only its last message and those still streaming, queued, or waiting on an
/// approval, so listing every thread does not send each one's whole history.
fn thread_summary(thread: &Thread) -> Thread {
    let last = thread.messages.len().saturating_sub(1);
    let messages = thread
        .messages
        .iter()
        .enumerate()
        .filter(|(i, m)| {
            *i == last
                || m.status == MessageStatus::Streaming
                || m.queued
                || m.approvals
                    .iter()
                    .any(|a| a.status == ApprovalStatus::Pending)
        })
        .map(|(_, m)| m.clone())
        .collect();
    Thread {
        messages,
        ..thread.clone()
    }
}

/// A message the user sent, complete from the start.
fn user_message(text: &str, attachments: Vec<Attachment>) -> Message {
    Message {
        id: new_id(),
        role: Role::User,
        text: text.into(),
        created_at: now_ms(),
        status: MessageStatus::Complete,
        tools: vec![],
        plan: vec![],
        files: vec![],
        error: None,
        approvals: vec![],
        attachments,
        steer: false,
        queued: false,
        background: Vec::new(),
        cut_off: false,
        refusal: None,
        parts: Vec::new(),
        finished_at: None,
        usage: None,
        turn: None,
    }
}

impl Server {
    /// `config_dir` holds the database and config files; `data_dir` holds chat scratch folders.
    pub fn open(config_dir: PathBuf, data_dir: &Path, sink: EventSink) -> Res<Arc<Self>> {
        provider::model_manifest::start(&config_dir);
        provider::api_config::init(&config_dir);
        provider::mcp::init(&config_dir);
        skills::init(&config_dir);
        plugins::init(&config_dir);
        let agents = agents::Library::open(&config_dir);
        std::thread::spawn(scope::prepare);
        let clis_dir = config_dir.join("clis");
        let logs = debug_log::session_dir(&config_dir);
        import::debug_log::LOG.init(logs.clone());
        debug_log::RENDERER.init(logs.clone());
        debug_log::AGENTS.init(logs);
        let store = Store::open(config_dir)?;
        follow_log_levels(&store.settings());
        orchestration::recovery::recover(&store)?;
        let server = Arc::new(Self {
            store,
            usage: usage::UsageCache::default(),
            runner: Runner::default(),
            chats_dir: data_dir.join("chats"),
            pasted_dir: data_dir.join("pasted"),
            clis_dir,
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

    /// Whether the Antigravity CLI (`agy`) is installed in Patchwork's CLI folder.
    pub fn antigravity_installed(&self) -> bool {
        self.clis_dir.join(provider::antigravity::BINARY).is_file()
    }

    /// Downloads Antigravity's install script to a file, then runs it into Patchwork's CLI
    /// folder. The script is saved first, so the file that runs is the one that was fetched.
    /// Windows gets the PowerShell script, which installs `agy.exe` the same way.
    pub fn install_antigravity(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.clis_dir).map_err(|e| e.to_string())?;
        let (name, runner): (&str, &[&str]) = if cfg!(windows) {
            (
                "install.ps1",
                &[
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ],
            )
        } else {
            ("install.sh", &[])
        };
        let script = self.clis_dir.join(format!("install-antigravity-{name}"));
        let fetch = crate::host_command("curl")
            .args([
                "-fsSL",
                &format!("https://antigravity.google/cli/{name}"),
                "-o",
            ])
            .arg(&script)
            .output()
            .map_err(|e| format!("Could not run curl: {e}"))?;
        if !fetch.status.success() {
            return Err(format!(
                "Download failed: {}",
                String::from_utf8_lossy(&fetch.stderr).trim()
            ));
        }
        let shell = if cfg!(windows) { "powershell" } else { "bash" };
        let run = crate::host_command(shell)
            .args(runner)
            .arg(&script)
            .arg("--dir")
            .arg(&self.clis_dir)
            .output()
            .map_err(|e| format!("Could not run the installer: {e}"))?;
        let _ = std::fs::remove_file(&script);
        if !run.status.success() || !self.antigravity_installed() {
            return Err(format!(
                "The install did not finish: {}",
                String::from_utf8_lossy(&run.stderr).trim()
            ));
        }
        Ok(())
    }

    /// Scratch working directory for a chat that has no project.
    pub fn chat_dir(&self, thread_id: &str) -> PathBuf {
        self.chats_dir.join(thread_id)
    }

    /// Saves an image pasted from the clipboard (base64 `data`) as a file, and returns it as an
    /// attachment like a dropped image.
    pub fn save_pasted_image(
        &self,
        mime_type: &str,
        data: &str,
        name: Option<&str>,
    ) -> Result<Attachment, String> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| format!("Pasted image is not valid base64: {e}"))?;
        self.save_pasted_image_bytes(mime_type, &bytes, name)
    }

    /// Saves a pasted image's raw bytes as a file, and returns it as an attachment named `name`,
    /// the name the copy came with, or "Pasted image" when it has none.
    pub fn save_pasted_image_bytes(
        &self,
        mime_type: &str,
        bytes: &[u8],
        name: Option<&str>,
    ) -> Result<Attachment, String> {
        let ext = match mime_type {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => return Err(format!("Can't attach a {mime_type} image")),
        };
        std::fs::create_dir_all(&self.pasted_dir).map_err(|e| e.to_string())?;
        let path = self.pasted_dir.join(format!("{}.{ext}", new_id()));
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        let mut attachment = Attachment::from_path(&path.to_string_lossy())?;
        attachment.name = name
            .and_then(|name| std::path::Path::new(name.trim()).file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("Pasted image.{ext}"));
        Ok(attachment)
    }

    /// Saves text pasted from the clipboard as a file, and returns it as an attachment the agent
    /// opens itself, so a long paste does not fill the composer.
    pub fn save_pasted_text(&self, text: &str) -> Result<Attachment, String> {
        std::fs::create_dir_all(&self.pasted_dir).map_err(|e| e.to_string())?;
        let path = self.pasted_dir.join(format!("{}.txt", new_id()));
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        let mut attachment = Attachment::from_path(&path.to_string_lossy())?;
        attachment.name = "Pasted text.txt".to_string();
        Ok(attachment)
    }

    /// Watches the project for changes, in place of any project watched before. After each
    /// settled burst it runs one `git status`, adds what replies under way there changed to
    /// them, and passes the result to `on_change`.
    pub fn watch_project(
        self: &Arc<Self>,
        project_id: &str,
        on_change: impl Fn(types::ProjectChanged) + Send + 'static,
    ) -> Result<(), String> {
        let mut current = self
            .project_watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current.as_ref().is_some_and(|(id, _)| id == project_id) {
            return Ok(());
        }
        let root = self.project_root(project_id)?;
        let watched = PathBuf::from(&root);
        // The watcher lives in the server, so it holds the server weakly.
        let server = Arc::downgrade(self);
        let id = project_id.to_string();
        let watcher = watch::ProjectWatcher::start(&watched, move |burst| {
            let Some(server) = server.upgrade() else {
                return;
            };
            let changes = workspace::git_changes(&root).ok();
            if let Some(changes) = &changes {
                server.note_project_changes(&id, changes, &burst);
            }
            on_change(types::ProjectChanged {
                project_id: id.clone(),
                paths: burst.into_iter().map(|c| c.path).collect(),
                branch: workspace::git_branch(&root),
                changes,
            });
        })?;
        *current = Some((project_id.to_string(), watcher));
        Ok(())
    }

    /// Watches the skills folder in place of any watch before, passing the skills to
    /// `on_change` after each settled burst of changes.
    pub fn watch_skills(
        self: &Arc<Self>,
        on_change: impl Fn(Vec<skills::Skill>) + Send + 'static,
    ) -> Result<(), String> {
        let mut current = self
            .skills_watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *current = None;
        // The watcher lives in the server, so it holds the server weakly.
        let server = Arc::downgrade(self);
        let watcher = skills::watch(&skills::dir(&self.store.settings()), move || {
            if let Some(server) = server.upgrade() {
                on_change(skills::all(&server.store.settings()));
            }
        })?;
        *current = Some(watcher);
        Ok(())
    }

    pub fn unwatch_skills(&self) {
        *self
            .skills_watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    pub fn project_root(&self, project_id: &str) -> Result<String, String> {
        self.store
            .settings()
            .projects
            .into_iter()
            .find(|p| p.id == project_id)
            .map(|p| p.path)
            .ok_or_else(|| format!("unknown project {project_id}"))
    }

    // ---- settings & projects ---------------------------------------------

    pub fn save_settings(&self, settings: Settings) -> Result<(), String> {
        follow_log_levels(&settings);
        let guards_changed =
            !permissions::same_guards(&self.store.settings().permissions, &settings.permissions);
        self.store.update_settings(|s| *s = settings)?;
        // A rule the user just added may answer requests already waiting.
        self.runner.apply_rules(self);
        // Idle agents were told the old deny and ask rules, so the next message starts a new one.
        if guards_changed {
            self.runner.retire_all_idle();
        }
        Ok(())
    }

    /// Adds `project` unless its path is already registered; returns the stored one.
    fn register_project(&self, project: Project, activate: bool) -> Result<Project, String> {
        self.store.update_settings(|s| {
            let p = match s.projects.iter().find(|p| p.path == project.path) {
                Some(existing) => existing.clone(),
                None => {
                    s.projects.push(project.clone());
                    project
                }
            };
            if activate {
                s.active_project_id = Some(p.id.clone());
            }
            p
        })
    }

    pub fn add_project(&self, path: &str) -> Result<Project, String> {
        self.register_project(workspace::make_project(path)?, true)
    }

    pub fn create_project(&self, parent: &str, name: &str) -> Result<Project, String> {
        self.register_project(workspace::create_project_dir(parent, name)?, true)
    }

    pub fn rename_project(&self, id: &str, name: &str) -> Result<(), String> {
        let name = clean_name(name)?;
        self.store.update_settings(|s| {
            let project = s.projects.iter_mut().find(|p| p.id == id);
            project.ok_or_else(|| format!("unknown project {id}"))?.name = name;
            Ok::<_, String>(())
        })?
    }

    /// Points a project at another folder, such as after it was moved or renamed. Agents keep
    /// their sessions by folder, so the project's tasks start new ones, sent their transcript.
    pub fn relocate_project(&self, id: &str, path: &str) -> Result<(), String> {
        let path = workspace::make_project(path)?.path;
        let threads: Vec<String> = self
            .store
            .threads()
            .into_iter()
            .filter(|t| t.project_id.as_deref() == Some(id))
            .map(|t| t.id)
            .collect();
        if threads.iter().any(|t| self.runner.is_running(t)) {
            return Err("Stop this project's running tasks before changing its folder".into());
        }
        let moved = self.store.update_settings(|s| {
            if let Some(other) = s.projects.iter().find(|p| p.path == path && p.id != id) {
                return Err(format!("{path} is already the folder of {}", other.name));
            }
            let project = s.projects.iter_mut().find(|p| p.id == id);
            let project = project.ok_or_else(|| format!("unknown project {id}"))?;
            let moved = project.path != path;
            project.path = path;
            Ok(moved)
        })??;
        if moved {
            for thread_id in &threads {
                // An idle agent still runs in the old folder.
                self.runner.retire_idle(thread_id);
                self.store.update_thread(thread_id, |t| {
                    t.session_id = None;
                    t.context_pending = !t.messages.is_empty();
                })?;
            }
        }
        Ok(())
    }

    pub fn remove_project(&self, id: &str) -> Result<(), String> {
        self.store.update_settings(|s| {
            s.projects.retain(|p| p.id != id);
            if s.active_project_id.as_deref() == Some(id) {
                s.active_project_id = s.projects.first().map(|p| p.id.clone());
            }
        })?;
        self.store.delete_project_threads(id)
    }

    // ---- threads & turns -------------------------------------------------

    /// Every thread (project tasks and project-less chats), in sidebar order.
    /// Every thread, each with only the messages the task list reads: the last one, and any
    /// still streaming, queued, or waiting on an approval. `thread` returns one in full.
    pub fn list_threads(&self) -> Vec<Thread> {
        let mut ts: Vec<Thread> = self
            .store
            .with_threads(|threads| threads.iter().map(thread_summary).collect());
        ts.sort_by_key(|t| t.position);
        ts
    }

    /// One thread with all its messages.
    pub fn thread(&self, id: &str) -> Result<Thread, String> {
        Ok(self.store.thread(id).ok_or("Task not found")?)
    }

    /// Puts `ids` in `folder_id` (None: the top level) in their new order.
    pub fn reorder_threads(&self, ids: &[String], folder_id: Option<&str>) -> Result<(), String> {
        if let Some(folder_id) = folder_id {
            self.folder(folder_id)?;
        }
        self.store.reorder_threads(ids, folder_id)
    }

    /// Files of a project that a mention can refer to.
    pub fn list_files(&self, project_id: &str) -> Result<Vec<String>, String> {
        Ok(workspace::list_files(&self.project_root(project_id)?)?)
    }

    /// The thread that was open when the app last closed, if one was recorded.
    pub fn last_thread_id(&self) -> Option<String> {
        self.store.state(LAST_THREAD_KEY)
    }

    pub fn set_last_thread_id(&self, id: &str) -> Result<(), String> {
        self.store.set_state(LAST_THREAD_KEY, id)
    }

    /// Ids of the projects and folders that were collapsed in the sidebar.
    pub fn collapsed_sections(&self) -> Vec<String> {
        self.store
            .state(COLLAPSED_KEY)
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn set_collapsed_sections(&self, ids: &[String]) -> Result<(), String> {
        let json = serde_json::to_string(ids).map_err(|e| e.to_string())?;
        self.store.set_state(COLLAPSED_KEY, &json)
    }

    // ---- folders ---------------------------------------------------------

    pub fn list_folders(&self) -> Vec<Folder> {
        self.store.folders()
    }

    fn folder(&self, id: &str) -> Result<Folder, String> {
        self.store
            .folders()
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| format!("unknown folder {id}"))
    }

    pub fn create_folder(&self, project_id: &str, name: &str) -> Result<Folder, String> {
        self.project_root(project_id)?;
        let folder = Folder {
            id: new_id(),
            project_id: project_id.into(),
            name: clean_name(name)?,
        };
        self.store.create_folder(folder.clone())?;
        Ok(folder)
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> Result<(), String> {
        let name = clean_name(name)?;
        self.folder(id)?;
        self.store.rename_folder(id, &name)
    }

    /// Its tasks move to the project's top level.
    pub fn remove_folder(&self, id: &str) -> Result<(), String> {
        self.folder(id)?;
        self.store.delete_folder(id)
    }

    pub fn create_thread(&self, new: NewThread) -> Result<Thread, String> {
        let NewThread {
            project_id,
            agent,
            model,
            access_mode,
            effort,
        } = new;
        if let Some(id) = &project_id {
            self.project_root(id)?;
        }
        let title = if project_id.is_some() {
            "New task"
        } else {
            "New chat"
        };
        let created_at = now_ms();
        let thread = Thread {
            id: new_id(),
            project_id,
            title: title.into(),
            agent,
            model,
            access_mode,
            effort,
            session_id: None,
            created_at,
            messages: vec![],
            goal: None,
            position: -(created_at as i64),
            folder_id: None,
            archived_at: None,
            parent: None,
            plan_mode: false,
            slash_commands: vec![],
            imported_from: None,
            context_pending: false,
            role: None,
            role_pending: false,
        };
        self.store.insert_threads(vec![thread.clone()])?;
        Ok(thread)
    }

    /// Starts a new thread with the conversation of `thread_id` up to and including `message_id`,
    /// and the same agent and settings. Its first turn gives the agent that conversation as a
    /// transcript, in a session of its own, so the original thread goes on unchanged.
    pub fn fork_thread(&self, thread_id: &str, message_id: &str) -> Result<Thread, String> {
        let source = self.thread(thread_id)?;
        let end = source
            .messages
            .iter()
            .position(|m| m.id == message_id)
            .ok_or("Message not found")?;
        let created_at = now_ms();
        // Message ids are unique across threads, so the copies get new ones.
        let messages = source.messages[..=end]
            .iter()
            .map(|m| Message {
                id: new_id(),
                queued: false,
                background: Vec::new(),
                // The original thread's checkpoints stay with it.
                turn: None,
                ..m.clone()
            })
            .collect();
        let thread = Thread {
            id: new_id(),
            title: format!("{} (fork)", source.title),
            session_id: None,
            created_at,
            messages,
            goal: None,
            position: -(created_at as i64),
            archived_at: None,
            parent: None,
            imported_from: None,
            context_pending: true,
            role_pending: false,
            ..source
        };
        self.store.insert_threads(vec![thread.clone()])?;
        Ok(thread)
    }

    /// Starts a task in a new thread for `task`, and returns the thread.
    ///
    /// A task whose project no longer exists runs as a chat, so a deleted project does not stop
    /// the schedule without saying so.
    pub fn start_scheduled_task(
        self: &Arc<Self>,
        task: &types::ScheduledTask,
    ) -> Result<Thread, String> {
        let project_id = task.project_id.clone().filter(|id| {
            self.store
                .settings()
                .projects
                .iter()
                .any(|project| &project.id == id)
        });
        let thread = self.create_thread(NewThread {
            project_id,
            agent: task.agent,
            model: task.model.clone(),
            access_mode: task.access_mode,
            effort: None,
        })?;
        let thread = self.store.thread(&thread.id).unwrap_or(thread);
        self.emit(AgentEventEnvelope {
            thread_id: thread.id.clone(),
            event: AgentEvent::ThreadCreated {
                thread: thread.clone(),
            },
        });
        self.send_message(thread.id.clone(), task.prompt.clone(), Vec::new(), false)?;
        // Named after the task, not after its prompt: the first message is what the schedule says,
        // and the task's own name is what the list should show. This comes after the message so
        // the naming a new thread gets from its first line cannot take the name back.
        self.store
            .update_thread(&thread.id, |t| t.title = task.name.clone())?;
        let thread = self.store.thread(&thread.id).unwrap_or(thread);
        Ok(thread)
    }

    /// Starts one scheduled task now, whatever its schedule says.
    pub fn run_scheduled_task(self: &Arc<Self>, id: &str) -> Result<Thread, String> {
        let task = self
            .store
            .scheduled_tasks()
            .into_iter()
            .find(|task| task.id == id)
            .ok_or("The scheduled task no longer exists")?;
        let thread = self.start_scheduled_task(&task)?;
        let now = now_ms() as i64;
        let next = scheduled::next_run_at(&task.schedule, chrono::Local::now());
        self.store
            .scheduled_task_ran(&task.id, now, next, Ok(thread.id.as_str()))?;
        Ok(thread)
    }

    /// Whether a reply is under way in a thread of `project_id`. Chats have no project and never
    /// collide, since each has a scratch folder of its own.
    fn busy_in_project(&self, project_id: Option<&str>) -> bool {
        let Some(project_id) = project_id else {
            return false;
        };
        self.runner.running().iter().any(|thread_id| {
            self.store
                .thread(thread_id)
                .is_some_and(|thread| thread.project_id.as_deref() == Some(project_id))
        })
    }

    /// Starts every scheduled task whose time has come, and returns their names. Called on a
    /// timer by the desktop shell; a task that fails to start is logged and recorded. A task whose
    /// project has a reply under way waits for a later tick, and a slot that went by too long ago
    /// is skipped.
    pub fn run_due_scheduled_tasks(self: &Arc<Self>) -> Vec<String> {
        let now = now_ms() as i64;
        let tasks = self.store.scheduled_tasks();
        let mut started = Vec::new();
        for task in &tasks {
            let next = || scheduled::next_run_at(&task.schedule, chrono::Local::now());
            match scheduled::action(task, now) {
                scheduled::Action::Wait => {}
                scheduled::Action::Skip => {
                    log::info!("scheduled task {} missed its time; skipped", task.name);
                    if let Err(e) = self.store.scheduled_task_skipped(&task.id, next()) {
                        log::warn!("scheduled task {}: {e}", task.id);
                    }
                }
                // It stays due, so the next tick tries again.
                scheduled::Action::Run if self.busy_in_project(task.project_id.as_deref()) => {}
                scheduled::Action::Run => match self.start_scheduled_task(task) {
                    Ok(thread) => {
                        let ran = self.store.scheduled_task_ran(
                            &task.id,
                            now,
                            next(),
                            Ok(thread.id.as_str()),
                        );
                        if let Err(e) = ran {
                            log::warn!("scheduled task {}: {e}", task.id);
                        }
                        started.push(task.name.clone());
                    }
                    Err(e) => {
                        log::warn!("scheduled task {} did not start: {e}", task.name);
                        // Record the attempt so a task that cannot start does not retry every tick.
                        let _ =
                            self.store
                                .scheduled_task_ran(&task.id, now, next(), Err(e.as_str()));
                    }
                },
            }
        }
        started
    }

    pub fn rename_thread(&self, id: &str, title: &str) -> Result<(), String> {
        let title = clean_name(title)?;
        self.store.thread(id).ok_or("Task not found")?;
        self.store.update_thread(id, |t| t.title = title)
    }

    /// Moves a thread to the archive, or back to the task list. Archiving stops its running turn.
    pub fn set_thread_archived(&self, id: &str, archived: bool) -> Result<(), String> {
        self.store.thread(id).ok_or("Task not found")?;
        if archived {
            self.runner.cancel(id, None);
        }
        self.store
            .update_thread(id, |t| t.archived_at = archived.then(now_ms))
    }

    /// Stops every running agent; for quitting the app.
    /// How many threads have an agent replying now.
    pub fn running_turns(&self) -> usize {
        self.runner.running().len()
    }

    pub fn stop_agents(&self) {
        self.runner.stop_all();
    }

    /// Permanently deletes a thread and its messages.
    pub fn delete_thread(&self, id: &str) {
        self.runner.cancel(id, None);
        self.remove_checkpoints(id);
        self.remove_api_session(id);
        if self
            .store
            .thread(id)
            .is_some_and(|t| t.project_id.is_none())
        {
            let _ = std::fs::remove_dir_all(self.chat_dir(id));
        }
        if let Err(e) = self.store.delete_thread(id) {
            log::warn!("failed to delete thread: {e}");
        }
        // Its subagents' threads go with it.
        let children = self
            .store
            .threads()
            .into_iter()
            .filter(|t| t.parent.as_ref().is_some_and(|p| p.thread_id == id));
        children.for_each(|child| {
            if let Err(e) = self.store.delete_thread(&child.id) {
                log::warn!("failed to delete a subagent's thread: {e}");
            }
        });
    }

    /// Sets the thread's model. The agent can only change before the first message,
    /// since the conversation session belongs to one CLI. An imported chat has no session yet,
    /// so it can take any agent until its first turn.
    pub fn set_thread_model(
        &self,
        id: &str,
        agent: AgentKind,
        model: Option<String>,
    ) -> Result<(), String> {
        let thread = self.store.thread(id).ok_or("Task not found")?;
        if thread.agent != agent && !thread.messages.is_empty() && !thread.context_pending {
            return Err("The agent can't change after the conversation has started".into());
        }
        let changed = thread.agent != agent || thread.model != model;
        self.store.update_thread(id, |t| {
            t.agent = agent;
            t.model = model;
        })?;
        // An idle agent was started with the old model.
        if changed {
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Takes effect from the thread's next turn.
    pub fn set_thread_access_mode(&self, id: &str, mode: AccessMode) -> Result<(), String> {
        let thread = self.store.thread(id).ok_or("Task not found")?;
        self.store.update_thread(id, |t| t.access_mode = mode)?;
        // An idle agent was started with the old setting.
        if thread.access_mode != mode {
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Writes `text` to `path`, a file the user picked in a save dialog.
    pub fn save_text_file(&self, path: &str, text: &str) -> Result<(), String> {
        std::fs::write(path, text).map_err(|e| e.to_string())
    }

    /// The custom agents a chat in the project can summon: the agents folder's and the
    /// project's own. A chat without a project has only the folder's.
    pub fn project_agents(&self, project_id: Option<&str>) -> Vec<agents::Agent> {
        match project_id.and_then(|id| self.project_root(id).ok()) {
            Some(root) => self.agents.available(Path::new(&root)),
            None => self.agents.list().to_vec(),
        }
    }

    fn thread_agents(&self, thread_id: &str) -> Vec<agents::Agent> {
        let project_id = self.store.with_threads(|threads| {
            threads
                .iter()
                .find(|t| t.id == thread_id)
                .and_then(|t| t.project_id.clone())
        });
        self.project_agents(project_id.as_deref())
    }

    /// Makes `role` the chat's custom agent, or none; the agent is told with the next message.
    pub fn set_thread_role(&self, id: &str, role: Option<String>) -> Result<(), String> {
        let mut changed = false;
        self.store.update_thread(id, |t| {
            if t.role != role {
                t.role = role.clone();
                t.role_pending = true;
                changed = true;
            }
        })?;
        if changed {
            self.emit(AgentEventEnvelope {
                thread_id: id.into(),
                event: AgentEvent::RoleChanged { role },
            });
        }
        Ok(())
    }

    /// Switches the chat to the model of the last custom agent `text` summons that names one,
    /// when the chat's CLI offers it, as `provider/model` or as the model alone. An idle agent
    /// is replaced; a running turn keeps its model until it ends.
    fn switch_to_agent_model(&self, thread_id: &str, text: &str) {
        if !text.contains('&') {
            return;
        }
        let agents = self.thread_agents(thread_id);
        let Some(wanted) = agents::mentioned(text)
            .filter_map(|name| agents.iter().find(|a| a.name == name)?.model.as_deref())
            .last()
        else {
            return;
        };
        let Some((agent, model)) = self.store.with_threads(|threads| {
            threads
                .iter()
                .find(|t| t.id == thread_id)
                .map(|t| (t.agent, t.model.clone()))
        }) else {
            return;
        };
        let models = provider::list_models(agent);
        let Some(offered) = agents::offered(wanted, models.iter().filter_map(|m| m.id.as_deref()))
            .map(str::to_string)
        else {
            return;
        };
        if model.as_ref() == Some(&offered) {
            return;
        }
        match self.set_thread_model(thread_id, agent, Some(offered.clone())) {
            Ok(()) => self.emit(AgentEventEnvelope {
                thread_id: thread_id.into(),
                event: AgentEvent::ModelChanged {
                    model: Some(offered),
                },
            }),
            Err(e) => log::warn!("failed to switch to the agent's model: {e}"),
        }
    }

    /// `text` with the roles of the custom agents it summons (see `agents::summon`), and of the
    /// chat's own agent when its agent was not told of it yet: the role was changed by hand, or
    /// `fresh`, a new agent session, knows of none. Also the primary agent it makes the chat's
    /// agent, which `told_role` keeps once the message is sent.
    fn summon_agents(
        &self,
        thread_id: &str,
        text: String,
        fresh: bool,
    ) -> (String, Option<String>) {
        let Some((role, pending)) = self.store.with_threads(|threads| {
            threads
                .iter()
                .find(|t| t.id == thread_id)
                .map(|t| (t.role.clone(), t.role_pending))
        }) else {
            return (text, None);
        };
        let standing = match &role {
            Some(name) if pending || fresh => agents::Standing::Untold(name),
            None if pending => agents::Standing::Removed,
            _ => agents::Standing::Told,
        };
        // Most messages summon no one, and then the agents are not read.
        if matches!(standing, agents::Standing::Told) && !text.contains('&') {
            return (text, None);
        }
        let summoned = agents::summon(&self.thread_agents(thread_id), text, standing);
        let switched = summoned.role.filter(|new| role.as_ref() != Some(new));
        (summoned.text, switched)
    }

    /// Records that the chat's agent was told of its role with a message just sent, and makes
    /// `switched`, a primary agent the message summoned, the chat's agent.
    fn told_role(&self, thread_id: &str, switched: Option<String>) {
        let pending = self
            .store
            .with_threads(|threads| threads.iter().any(|t| t.id == thread_id && t.role_pending));
        if !pending && switched.is_none() {
            return;
        }
        let saved = self.store.update_thread(thread_id, |t| {
            t.role_pending = false;
            if let Some(role) = &switched {
                t.role = Some(role.clone());
            }
        });
        if let Err(e) = saved {
            log::warn!("failed to save the chat's agent: {e}");
        } else if switched.is_some() {
            self.emit(AgentEventEnvelope {
                thread_id: thread_id.into(),
                event: AgentEvent::RoleChanged { role: switched },
            });
        }
    }

    /// Takes effect from the thread's next turn.
    pub fn set_thread_plan_mode(&self, id: &str, on: bool) -> Result<(), String> {
        let thread = self.store.thread(id).ok_or("Task not found")?;
        self.store.update_thread(id, |t| t.plan_mode = on)?;
        // An idle agent was started with the old setting.
        if thread.plan_mode != on {
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Takes effect from the thread's next turn.
    pub fn set_thread_effort(&self, id: &str, effort: Option<String>) -> Result<(), String> {
        let thread = self.store.thread(id).ok_or("Task not found")?;
        self.store
            .update_thread(id, |t| t.effort = effort.clone())?;
        // An idle agent was started with the old setting.
        if thread.effort != effort {
            self.runner.retire_idle(id);
        }
        Ok(())
    }

    /// Replaces the `placeholder` title with one a model writes from `text`, in the background,
    /// unless the thread has been renamed meanwhile. With titles switched off in Settings, a
    /// slash command, or a message the model gives no title for, the first line names it. A
    /// message that is only attachments stays blank.
    fn name_thread(
        self: &Arc<Self>,
        thread_id: &str,
        text: &str,
        placeholder: String,
        agent: AgentKind,
        model: Option<String>,
    ) {
        let server = Arc::downgrade(self);
        let thread_id = thread_id.to_string();
        let text = text.to_string();
        let enabled = self.store.settings().tokens.generate_titles;
        std::thread::spawn(move || {
            let first_line = title_from(&text);
            let generated = if !enabled || first_line.starts_with('/') || text.trim().is_empty() {
                None
            } else {
                title::generate(&text, agent, model.as_deref())
            };
            let generated = generated.unwrap_or(first_line);
            let Some(server) = server.upgrade() else {
                return;
            };
            let mut renamed = false;
            let _ = server.store.update_thread(&thread_id, |t| {
                if t.title == placeholder {
                    t.title = generated.clone();
                    renamed = true;
                }
            });
            if renamed {
                server.emit(AgentEventEnvelope {
                    thread_id,
                    event: AgentEvent::TitleUpdated { title: generated },
                });
            }
        });
    }

    /// Appends the user message, then starts the agent turn in the background.
    /// Returns the stored user message; assistant output arrives via the event sink.
    pub fn send_message(
        self: &Arc<Self>,
        thread_id: String,
        text: String,
        attachments: Vec<Attachment>,
        queue: bool,
    ) -> Result<Message, String> {
        let images = provider::load_images(&attachments)?;
        if provider::is_goal_command(&text) {
            if self.runner.is_running(&thread_id) {
                return Err(
                    "A /goal command runs as a turn of its own; send it once this turn ends".into(),
                );
            }
            let agent = self.store.thread(&thread_id).map(|t| t.agent);
            let arg = text.trim()["/goal".len()..].trim().to_lowercase();
            if agent == Some(AgentKind::OpenCode) {
                return Err(
                    "OpenCode has no /goal command. Describe the task in a message instead.".into(),
                );
            }
            if agent == Some(AgentKind::Api) {
                return Err(
                    "API providers have no /goal command. Describe the task in a message instead."
                        .into(),
                );
            }
            if agent == Some(AgentKind::Antigravity) {
                if provider::antigravity::antigravity_bin().is_none() {
                    return Err("/goal needs the Antigravity CLI. Install it in Settings.".into());
                }
                // Antigravity's /goal only sets a goal, so any other word would become one.
                let subcommands = [
                    "", "pause", "resume", "clear", "stop", "off", "reset", "none", "cancel",
                ];
                if subcommands.contains(&arg.as_str()) {
                    return Err("Antigravity's /goal only sets a goal: /goal <objective>. Clear a goal from the goal banner.".into());
                }
            }
            // Claude would take either word as a new goal it can never meet, and loop on it.
            if agent == Some(AgentKind::Claude) && matches!(arg.as_str(), "pause" | "resume") {
                return Err(format!(
                    "Claude has no /goal {arg}. Send a message to carry on with the goal, or /goal clear to drop it."
                ));
            }
            if !matches!(
                arg.as_str(),
                "" | "clear" | "stop" | "off" | "reset" | "none" | "cancel" | "pause"
            ) {
                // Only an explicit new goal or resume request can reopen a dismissed goal.
                // A provider restating cached goal state must not erase the dismissal.
                orchestration::goal::clear_dismissal(&self.store, &thread_id)?;
            }
        }
        // Checked before the message is stored, so a refused message leaves no trace.
        let mut msg = user_message(&text, attachments.clone());
        // Stored before the agent sees it, so its answer is stored after it.
        let m = msg.clone();
        let mut session_id = None;
        let mut subagents = Vec::new();
        let mut first_title = None;
        self.store.update_thread(&thread_id, |t| {
            if t.messages.is_empty() {
                first_title = Some((
                    t.title.clone(),
                    t.agent,
                    t.model.clone(),
                    t.project_id.clone(),
                ));
            }
            // Subagents of the running turn still working, which a steer must not cut off.
            subagents = t
                .messages
                .iter()
                .flat_map(|m| &m.tools)
                .filter(|c| {
                    c.subagent.is_some() && c.parent.is_none() && c.status == StepStatus::Running
                })
                .map(|c| c.id.clone())
                .collect();
            t.messages.push(m);
            session_id = t.session_id.clone();
        })?;
        if let Some((placeholder, agent, model, project_id)) = first_title {
            let root = project_id.and_then(|id| {
                self.store
                    .settings()
                    .projects
                    .into_iter()
                    .find(|p| p.id == id)
                    .map(|p| PathBuf::from(p.path))
            });
            // Files the message opens with, by attachment or by a name in the project, name nothing.
            let subject = without_leading_attachments(&text, |word| {
                attachments.iter().any(|a| a.name == word)
                    || root.as_ref().is_some_and(|root| root.join(word).is_file())
            });
            self.name_thread(&thread_id, subject, placeholder, agent, model);
        }
        // A goal that has ended is over: the next message starts unrelated work, so its banner
        // goes. The agent has already dropped the goal, or `settle` told it to with a /goal
        // command, which is why those leave the banner alone.
        if !provider::is_goal_command(&text)
            && let Some(goal) = self.store.thread(&thread_id).and_then(|t| t.goal)
            && matches!(goal.status, GoalStatus::Complete | GoalStatus::Blocked)
        {
            self.store
                .dismiss_goal(&thread_id, &orchestration::goal::signature(&goal))?;
            orchestration::publish(self, &thread_id, AgentEvent::GoalUpdated { goal: None });
        }
        self.switch_to_agent_model(&thread_id, &text);
        // A /goal runs as a turn of its own, not in an idle agent's process.
        if provider::is_goal_command(&text) {
            self.runner.retire_idle(&thread_id);
        }
        // A message that names a skill as "$name" or summons an agent as "&name" anywhere reaches
        // the agent in the form it runs, as `start_turn` sends it too. Only a steer sends it
        // from here, since summoning can change the chat's agent.
        let mut switched = None;
        let mut steer_text = || {
            let skills = skills::enabled(&self.store.settings());
            let agent = self.store.with_threads(|threads| {
                threads.iter().find(|t| t.id == thread_id).map(|t| t.agent)
            });
            let text = match (agent, skills::invoked(&text, &skills)) {
                (Some(agent), Some((skill, rest))) => skills::invocation(agent, &skill, rest),
                _ => text.clone(),
            };
            let (text, role) = self.summon_agents(&thread_id, text, false);
            switched = Some(role);
            text
        };
        // While a turn runs, the message steers it, or waits for it to end when the
        // agent cannot take it now.
        let delivery = if queue {
            match self.runner.enqueue(
                &thread_id,
                FollowUpInput {
                    message_id: &msg.id,
                    text: &text,
                    attachments: &attachments,
                    images: &images,
                },
            ) {
                Ok(true) => Delivery::Queued,
                Ok(false) => self.runner.steer(
                    &thread_id,
                    FollowUpInput {
                        message_id: &msg.id,
                        text: &steer_text(),
                        attachments: &attachments,
                        images: &images,
                    },
                    session_id.as_deref(),
                    &subagents,
                ),
                Err(error) => {
                    self.store.update_thread(&thread_id, |t| {
                        t.messages.retain(|queued| queued.id != msg.id);
                    })?;
                    return Err(error);
                }
            }
        } else {
            self.runner.steer(
                &thread_id,
                FollowUpInput {
                    message_id: &msg.id,
                    text: &steer_text(),
                    attachments: &attachments,
                    images: &images,
                },
                session_id.as_deref(),
                &subagents,
            )
        };
        // A message that did not reach the running turn starts one, which summons again.
        if !matches!(delivery, Delivery::NotRunning)
            && let Some(switched) = switched
        {
            self.told_role(&thread_id, switched);
        }
        match delivery {
            Delivery::Steered => {
                msg.steer = true;
                self.store.update_thread(&thread_id, |t| {
                    if let Some(m) = t.messages.iter_mut().find(|m| m.id == msg.id) {
                        m.steer = true;
                    }
                })?;
            }
            Delivery::Queued => {
                // Read under the store's lock: a turn that ends meanwhile has already taken the
                // message from its queue, and then its own unflagging comes after this write.
                self.store.update_thread(&thread_id, |t| {
                    if let Some(m) = t.messages.iter_mut().find(|m| m.id == msg.id) {
                        m.queued = self.runner.is_queued(&thread_id, &m.id);
                        msg.queued = m.queued;
                    }
                })?;
            }
            Delivery::Resumed => self.capture_checkpoint(&thread_id, &msg.id),
            Delivery::NotRunning => {
                // The app adds a message it sent once this returns, but not one the server sends
                // itself (a goal clear), so the chat learns of it before the reply starts.
                self.emit(AgentEventEnvelope {
                    thread_id: thread_id.clone(),
                    event: AgentEvent::MessageStarted {
                        message: msg.clone(),
                    },
                });
                self.runner.start_turn(
                    self,
                    TurnRequest {
                        thread_id,
                        text,
                        attachments,
                        images,
                    },
                )?
            }
        }
        Ok(msg)
    }

    /// Stops the running turn by interrupting the agent, so its session records the stop and an
    /// agent that can stays up for the next message. Codex is first told to pause the goal it is
    /// pursuing, so it does not carry on with it later.
    pub fn cancel_turn(&self, thread_id: &str) {
        self.stop_turn(thread_id, true);
    }

    /// Stops the running turn as `cancel_turn` does; without `interrupt`, the agent process ends
    /// with it.
    fn stop_turn(&self, thread_id: &str, interrupt: bool) {
        let thread = self.store.thread(thread_id);
        let session_id = thread.as_ref().and_then(|t| t.session_id.clone());
        let pause = thread.and_then(|t| {
            let active = t.goal.is_some_and(|g| g.status == GoalStatus::Active);
            provider::goal_pause(t.agent, t.session_id.as_deref()?).filter(|_| active)
        });
        let dropped = if interrupt {
            self.runner
                .interrupt(thread_id, session_id.as_deref(), pause)
        } else {
            self.runner.cancel(thread_id, pause)
        };
        dropped.into_iter().for_each(|message_id| {
            orchestration::publish(
                self,
                thread_id,
                AgentEvent::MessageQueued {
                    message_id,
                    queued: false,
                },
            );
        });
    }

    /// Takes back a message sent mid-turn that has not started yet, so it can be edited.
    pub fn remove_queued(&self, thread_id: &str, message_id: &str) -> Result<(), String> {
        let Some(thread) = self.store.thread(thread_id) else {
            return Err("Thread not found".into());
        };
        let Some(message) = thread.messages.iter().find(|m| m.id == message_id) else {
            // Makes startup cleanup safe to retry if an app refresh overlaps another cleanup.
            return Ok(());
        };
        if !message.queued {
            return Err("That message has already been sent".into());
        }
        let running = self.runner.is_running(thread_id);
        let withdrawn = self.runner.withdraw(thread_id, message_id);
        if running && !withdrawn {
            return Err("That message has already been sent".into());
        }
        orchestration::publish(
            self,
            thread_id,
            AgentEvent::MessageRemoved {
                message_id: message_id.into(),
            },
        );
        Ok(())
    }

    /// Updates a queued message without changing its position or id.
    pub fn edit_queued(
        &self,
        thread_id: &str,
        message_id: &str,
        text: String,
        attachments: Vec<Attachment>,
    ) -> Result<(), String> {
        let images = provider::load_images(&attachments)?;
        let thread = self.store.thread(thread_id).ok_or("Thread not found")?;
        let original = thread
            .messages
            .iter()
            .find(|m| m.id == message_id && m.queued)
            .ok_or("That message is no longer queued")?;
        if self.runner.is_running(thread_id)
            && !self.runner.edit_queued(
                thread_id,
                FollowUpInput {
                    message_id,
                    text: &text,
                    attachments: &attachments,
                    images: &images,
                },
            )
        {
            return Err("That message has already been promoted".into());
        }
        let mut edited = original.clone();
        edited.text = text;
        edited.attachments = attachments;
        self.store.update_thread(thread_id, |t| {
            if let Some(m) = t
                .messages
                .iter_mut()
                .find(|m| m.id == message_id && m.queued)
            {
                m.text = edited.text.clone();
                m.attachments = edited.attachments.clone();
            }
        })?;
        orchestration::publish(
            self,
            thread_id,
            AgentEvent::MessageEdited { message: edited },
        );
        Ok(())
    }

    /// Sends the thread's queued messages once its turn has ended, in the background.
    pub(crate) fn send_queued_later(self: &Arc<Self>, thread_id: &str) {
        let server = Arc::clone(self);
        let thread_id = thread_id.to_string();
        std::thread::spawn(move || {
            let queued = server
                .store
                .thread(&thread_id)
                .is_some_and(|t| t.messages.iter().any(|m| m.queued));
            if queued && let Err(e) = server.resume_queued(&thread_id) {
                log::warn!("failed to send queued messages: {e}");
            }
        });
    }

    /// Sends persisted queued messages in their original order, as one message: to the idle
    /// agent when one is up, as a normal message would go, otherwise in a new turn.
    pub fn resume_queued(self: &Arc<Self>, thread_id: &str) -> Result<(), String> {
        if self.runner.is_running(thread_id) {
            return Err("This turn is already running".into());
        }
        let thread = self.store.thread(thread_id).ok_or("Thread not found")?;
        let queued: Vec<_> = thread.messages.into_iter().filter(|m| m.queued).collect();
        if queued.is_empty() {
            return Err("There are no queued messages to resume".into());
        }
        let text = queued
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let attachments: Vec<_> = queued.iter().flat_map(|m| m.attachments.clone()).collect();
        let images = provider::load_images(&attachments)?;
        self.store.update_thread(thread_id, |t| {
            t.messages
                .iter_mut()
                .filter(|m| queued.iter().any(|q| q.id == m.id))
                .for_each(|m| m.queued = false);
        })?;
        queued.iter().for_each(|m| {
            orchestration::publish(
                self,
                thread_id,
                AgentEvent::MessageQueued {
                    message_id: m.id.clone(),
                    queued: false,
                },
            )
        });
        let first = &queued[0].id;
        let input = FollowUpInput {
            message_id: first,
            text: &text,
            attachments: &attachments,
            images: &images,
        };
        if !matches!(
            self.runner
                .steer(thread_id, input, thread.session_id.as_deref(), &[]),
            Delivery::NotRunning
        ) {
            self.capture_checkpoint(thread_id, first);
            return Ok(());
        }
        if let Err(error) = self.runner.start_turn(
            self,
            TurnRequest {
                thread_id: thread_id.into(),
                text,
                attachments,
                images,
            },
        ) {
            self.store.update_thread(thread_id, |t| {
                t.messages
                    .iter_mut()
                    .filter(|m| queued.iter().any(|q| q.id == m.id))
                    .for_each(|m| m.queued = true);
            })?;
            queued.iter().for_each(|m| {
                orchestration::publish(
                    self,
                    thread_id,
                    AgentEvent::MessageQueued {
                        message_id: m.id.clone(),
                        queued: true,
                    },
                )
            });
            return Err(error);
        }
        Ok(())
    }

    /// Promotes an existing queued message into the running turn.
    pub fn steer_queued(&self, thread_id: &str, message_id: &str) -> Result<(), String> {
        let thread = self.store.thread(thread_id).ok_or("Thread not found")?;
        if !thread.messages.iter().any(|m| m.id == message_id) {
            return Err("Message not found".into());
        }
        let subagents: Vec<String> = thread
            .messages
            .iter()
            .flat_map(|m| &m.tools)
            .filter(|c| {
                c.subagent.is_some() && c.parent.is_none() && c.status == StepStatus::Running
            })
            .map(|c| c.id.clone())
            .collect();
        if !self.runner.promote_queued(
            thread_id,
            message_id,
            thread.session_id.as_deref(),
            &subagents,
        ) {
            return Err("That message is no longer queued or could not be sent".into());
        }
        orchestration::publish(
            self,
            thread_id,
            AgentEvent::MessageSteered {
                message_id: message_id.into(),
            },
        );
        Ok(())
    }

    pub fn stop_subagent(&self, thread_id: &str, tool_id: &str) -> Result<(), String> {
        self.runner.stop_subagent(thread_id, tool_id)
    }

    /// Clears a chat: any running turn is cancelled, then its messages and agent session are
    /// deleted. The chat stays in the sidebar, empty.
    pub fn clear_thread(&self, thread_id: &str) -> Result<(), String> {
        // The agent process goes too, since it holds the session being deleted.
        self.stop_turn(thread_id, false);
        self.remove_checkpoints(thread_id);
        self.remove_api_session(thread_id);
        self.store.clear_thread(thread_id)
    }

    /// Deletes the saved conversation of an API task, which Patchwork keeps itself.
    fn remove_api_session(&self, thread_id: &str) {
        let Some(thread) = self.store.thread(thread_id) else {
            return;
        };
        if let (AgentKind::Api, Some(session)) = (thread.agent, &thread.session_id) {
            let file = provider::api_config::sessions_dir().join(format!("{session}.json"));
            let _ = std::fs::remove_file(file);
        }
    }

    /// Answers a pending approval in the thread's running turn.
    pub fn respond_approval(
        self: &Arc<Self>,
        thread_id: &str,
        approval_id: &str,
        allow: bool,
        answers: std::collections::HashMap<String, Vec<String>>,
        rule: Option<types::RuleRef>,
    ) -> Result<(), String> {
        self.runner.respond(
            self,
            thread_id,
            ApprovalDecision {
                approval_id,
                allow,
                answers,
                rule,
            },
        )
    }

    // ---- importing conversations ------------------------------------------

    /// Sessions `source` saved on disk that can be imported: ones whose folder
    /// still exists, that Patchwork did not run as a chat, and that are not already
    /// threads: an agent session in `known`, or an imported transcript in `imported`.
    /// Returns (found, importable).
    fn importable(
        &self,
        source: ImportSource,
        known: &[(AgentKind, String)],
        imported: &[String],
    ) -> (u32, u32, Vec<import::Session>) {
        let default_agent = self
            .store
            .settings()
            .agents
            .first()
            .copied()
            .unwrap_or(AgentKind::Claude);
        let all = import::sessions(source, default_agent);
        let subagents = all.iter().filter(|s| s.subagent).count() as u32;
        // Patchwork's own chats are not the user's imports, so they are not counted.
        let all: Vec<_> = all
            .into_iter()
            .filter(|s| {
                if s.subagent {
                    ilog!("{source:?}: skipped {}: sub-agent run", s.session_id);
                    return false;
                }
                if Path::new(&s.cwd).starts_with(&self.chats_dir) {
                    ilog!("{source:?}: skipped {}: a Patchwork chat", s.session_id);
                    return false;
                }
                true
            })
            .collect();
        let found = all.len() as u32;
        let new: Vec<_> = all
            .into_iter()
            .filter(|s| {
                let already = if s.imported {
                    imported.contains(&s.session_id)
                } else {
                    known
                        .iter()
                        .any(|(a, id)| *a == s.agent && *id == s.session_id)
                };
                if already {
                    ilog!("{source:?}: skipped {}: already in Patchwork", s.session_id);
                }
                !already
            })
            .collect();
        ilog!(
            "{source:?}: {found} found, {} new, {subagents} sub-agent runs",
            new.len()
        );
        (found, subagents, new)
    }

    /// Starts the import log's entries for a scan or import.
    fn start_import_log(&self, what: &str) {
        ilog!("{what}");
    }

    pub fn scan_imports(&self, source: ImportSource) -> ImportScan {
        self.start_import_log(&format!("{source:?}: scan started"));
        let (found, subagents, new) = self.importable(
            source,
            &self.store.session_keys(),
            &self.store.imported_sources(),
        );
        ImportScan {
            kind: source,
            found,
            new: new.len() as u32,
            subagents,
        }
    }

    /// Imports new sessions as threads in the project for their folder, adding
    /// the project if needed. A conversation found by more than one source (a
    /// T3 Code thread is also in the agent's own files) is imported once.
    /// Returns how many threads were added.
    pub fn import_threads(&self, sources: &[ImportSource]) -> Result<u32, String> {
        self.start_import_log(&format!("import started: {sources:?}"));
        let mut known = self.store.session_keys();
        let mut imported = self.store.imported_sources();
        let mut threads = Vec::new();
        for &source in sources {
            for session in self.importable(source, &known, &imported).2 {
                if session.imported {
                    if imported.contains(&session.session_id) {
                        ilog!(
                            "{source:?}: skipped {}: imported from another source",
                            session.session_id
                        );
                        continue;
                    }
                    imported.push(session.session_id.clone());
                } else {
                    if known
                        .iter()
                        .any(|(a, id)| *a == session.agent && *id == session.session_id)
                    {
                        ilog!(
                            "{source:?}: skipped {}: imported from another source",
                            session.session_id
                        );
                        continue;
                    }
                    known.push((session.agent, session.session_id.clone()));
                }
                let project_id = if import::is_chat_folder(Path::new(&session.cwd)) {
                    ilog!(
                        "{source:?}: importing {} as a chat ({})",
                        session.session_id,
                        session.cwd
                    );
                    None
                } else {
                    ilog!(
                        "{source:?}: importing {} into project {}",
                        session.session_id,
                        session.cwd
                    );
                    let project = workspace::make_project(&session.cwd)
                        .map_err(|e| e.to_string())
                        .and_then(|p| self.register_project(p, false))
                        .inspect_err(|e| {
                            ilog!("{source:?}: couldn't add project {}: {e}", session.cwd)
                        })?;
                    Some(project.id)
                };
                threads.push(import::to_thread(session, project_id));
            }
        }
        let count = threads.len() as u32;
        self.store
            .insert_threads(threads)
            .inspect_err(|e| ilog!("import failed saving threads: {e}"))?;
        ilog!("import finished: {count} threads added");
        Ok(count)
    }
}
