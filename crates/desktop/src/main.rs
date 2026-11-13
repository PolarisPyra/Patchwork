//! Patchwork's window, drawn with GPUI over the server, its folders and its database.

// A release build on Windows opens no console window beside the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

// Rust guideline compliant 2026-10-09

mod android;
mod canvas;
mod chat;
mod onboarding;
mod panel;
mod settings;
mod sidebar;
mod ui;
mod workspace;

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    App, Bounds, Font, FontStyle, FontWeight, TextRenderingMode, TitlebarOptions, WindowBounds,
    WindowOptions, actions, font, prelude::*, px, size,
};
use patchwork_server::Server;
use patchwork_server::system::Dirs;

use crate::ui::theme::Theme;
use crate::ui::{assets, prompt, sigils, theme};
use crate::workspace::Workspace;

/// The app's allocator, which is faster than the system one when laying out and shaping text.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

actions!(
    patchwork,
    [
        FocusNext,
        FocusPrev,
        Quit,
        TogglePalette,
        NewChat,
        FindInChat,
        OpenSettings,
        ShowTasks,
        ShowChanges,
        ShowArchived,
        ToggleTerminal,
        NewTerminal,
        TogglePanel,
        ToggleOutput,
        DeleteChat
    ]
);

/// How often a scheduled task is looked for. A tick that finds nothing to start does no work.
const SCHEDULED_TICK: Duration = Duration::from_secs(30);

/// Writes the app's messages to `main.log` in this launch's log folder, as the desktop app does.
struct FileLog(Mutex<File>);

impl log::Log for FileLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        patchwork_server::logging::app_logs(metadata.level())
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            log::Level::Warn => "warning".to_string(),
            level => level.as_str().to_lowercase(),
        };
        let time = patchwork_server::logging::now_text();
        let mut properties = Properties(Vec::new());
        let _ = record.key_values().visit(&mut properties);
        let message =
            patchwork_server::logging::redact(&properties.fill(&record.args().to_string()));
        if let Ok(mut file) = self.0.lock() {
            let _ = writeln!(file, "{time} [{level}] {message}");
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.0.lock() {
            let _ = file.flush();
        }
    }
}

/// The named properties of a log record, written in place of their `{key}` in its message.
struct Properties(Vec<(String, String)>);

impl Properties {
    /// The message with each `{key}` it names replaced by that property's value.
    fn fill(&self, message: &str) -> String {
        let mut filled = String::with_capacity(message.len());
        let mut rest = message;
        while let Some(open) = rest.find('{') {
            filled.push_str(&rest[..open]);
            let named = rest[open + 1..].find('}').and_then(|close| {
                let key = &rest[open + 1..open + 1 + close];
                let value = self.0.iter().find(|(k, _)| k == key)?;
                Some((close, &value.1))
            });
            if let Some((close, value)) = named {
                filled.push_str(value);
                rest = &rest[open + close + 2..];
            } else {
                filled.push('{');
                rest = &rest[open + 1..];
            }
        }
        filled.push_str(rest);
        filled
    }
}

impl<'kvs> log::kv::VisitSource<'kvs> for Properties {
    fn visit_pair(
        &mut self,
        key: log::kv::Key<'kvs>,
        value: log::kv::Value<'kvs>,
    ) -> Result<(), log::kv::Error> {
        self.0.push((key.to_string(), value.to_string()));
        Ok(())
    }
}

/// Runs the helper the first argument names and returns its exit code, or None to open the app.
/// Without this, every hook or relay an agent starts would open another Patchwork window.
fn run_helper() -> Option<i32> {
    use patchwork_server::{harness, permissions, provider};
    let arg = std::env::args().nth(1)?;
    let run: fn() -> i32 = match arg.as_str() {
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
    };
    Some(run())
}

fn start_logging(dirs: &Dirs) {
    let folder = patchwork_server::logging::session_dir(&dirs.logs);
    let opened = std::fs::create_dir_all(&folder).and_then(|()| {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(folder.join("main.log"))
    });
    if let Ok(file) = opened
        && log::set_boxed_logger(Box::new(FileLog(Mutex::new(file)))).is_ok()
    {
        log::set_max_level(log::LevelFilter::Trace);
    }
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!(info:%; "panic: {{info}}");
        report(info);
    }));
}

/// Starts the tasks whose time has come, on a timer of its own, for the life of the app.
fn start_scheduled_tasks(server: Arc<Server>) {
    std::thread::Builder::new()
        .name("scheduled-tasks".into())
        .spawn(move || {
            loop {
                for name in server.run_due_scheduled_tasks() {
                    log::info!(name:%; "scheduled task started: {{name}}");
                }
                std::thread::sleep(SCHEDULED_TICK);
            }
        })
        .map_or_else(
            |e| log::warn!(error:% = e; "scheduled tasks will not run: {{error}}"),
            |_| (),
        );
}

/// Finds each font's fallbacks before any text needs them. The first line in a weight or style
/// not seen yet otherwise reads every installed font while a frame waits, which with a few
/// thousand fonts stalls a scroll for a frame or two. One font at a time, so a frame can shape
/// text in between.
fn prewarm_fonts(cx: &App) {
    let mut fonts = Vec::new();
    for family in [theme::FONT_UI, theme::FONT_MONO] {
        for weight in [
            FontWeight::NORMAL,
            FontWeight::MEDIUM,
            FontWeight::SEMIBOLD,
            FontWeight::BOLD,
        ] {
            for style in [FontStyle::Normal, FontStyle::Italic] {
                fonts.push(Font {
                    weight,
                    style,
                    ..font(family)
                });
            }
        }
    }
    let text_system = Arc::clone(cx.text_system());
    cx.background_spawn(async move {
        for font in fonts {
            text_system.prewarm_fonts(&[font]);
        }
    })
    .detach();
}

fn main() {
    // Agents start this binary as a helper, without the window: a hook, a relay or a bridge.
    if let Some(code) = run_helper() {
        std::process::exit(code);
    }
    // The platform's folders: the XDG ones on Linux, `Library` on macOS, and `AppData` on
    // Windows, the ones earlier versions of Patchwork used.
    let (Some(config), Some(data), Some(cache)) = (
        dirs::config_dir(),
        dirs::data_local_dir(),
        dirs::cache_dir(),
    ) else {
        log::error!("Patchwork could not find the folders to keep its data in");
        return;
    };
    let dirs = Dirs::new(&config, &data, &cache);
    // Opened a second time, the app shows the window it already has instead of another.
    if patchwork_server::system::instance::raise_running(&dirs.data) {
        return;
    }
    start_logging(&dirs);
    let (events, mut received) = mpsc::unbounded();
    let server = match Server::open(
        &dirs,
        &data.join("dev.patchwork.ade"),
        Box::new(move |event| {
            let _ = events.unbounded_send(event);
        }),
    ) {
        Ok(server) => server,
        Err(e) => {
            log::error!(error:% = e; "could not open Patchwork's data: {{error}}");
            return;
        }
    };
    start_scheduled_tasks(Arc::clone(&server));
    // A second launch asks on the relay's thread; the window is shown on the app's.
    let (raise, mut raised) = mpsc::unbounded::<()>();
    patchwork_server::provider::mcp::set_raise_handler(Box::new(move || {
        let _ = raise.unbounded_send(());
    }));
    // A change in where updating stands is drawn by the window, like an agent's events.
    let (update_sender, mut update_events) = mpsc::unbounded();
    server.on_update_change(Box::new(move || {
        let _ = update_sender.unbounded_send(());
    }));
    server.start_update_checks();
    server.start_limit_resumes();

    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            if let Err(e) = cx.text_system().add_fonts(assets::fonts()) {
                log::warn!(error:% = e; "could not load the fonts: {{error}}");
            }
            prewarm_fonts(cx);
            cx.background_spawn(async { ui::highlight::warm() })
                .detach();
            // Grayscale text, which reads lighter than subpixel on the dark theme.
            cx.set_text_rendering_mode(TextRenderingMode::Grayscale);
            settings::shortcuts::bind_keys(&server, cx);
            cx.set_global(sigils::load(&server));
            cx.set_prompt_builder(prompt::build);
            cx.on_action(|_: &Quit, cx| cx.quit());
            let quitting = Arc::clone(&server);
            cx.on_app_quit(move |_| {
                quitting.stop_agents();
                quitting.terminals.kill_all();
                patchwork_server::system::remote::close_all();
                async {}
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(1600.), px(960.)), cx);
            let setting = server.store.settings().theme;
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Patchwork".into()),
                        ..Default::default()
                    }),
                    app_id: Some("patchwork".into()),
                    ..Default::default()
                },
                |window, cx| {
                    cx.set_global(Theme::for_setting(setting, window.appearance()));
                    cx.new(|cx| Workspace::new(Arc::clone(&server), window, cx))
                },
            );
            let window = match opened {
                Ok(window) => window,
                Err(e) => {
                    log::error!(error:% = e; "could not open the window: {{error}}");
                    cx.quit();
                    return;
                }
            };
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            // Turns the last session's closing cut off carry on, now there is a window to show them.
            // Resumed, or with Settings set to ask, counted for the card that asks.
            let resuming = Arc::clone(&server);
            let waiting = cx.background_spawn(async move { resuming.resume_at_launch() });
            cx.spawn(async move |cx| {
                let waiting = waiting.await;
                if waiting > 0 {
                    let _ = window.update(cx, |workspace, _, cx| {
                        workspace.set_interrupted(waiting, cx);
                    });
                }
            })
            .detach();
            cx.spawn(async move |cx| {
                while raised.next().await.is_some() {
                    log::info!("a second launch asked for this window");
                    let shown = window.update(cx, |_, window, cx| {
                        cx.activate(true);
                        window.activate_window();
                    });
                    if shown.is_err() {
                        break;
                    }
                }
            })
            .detach();
            // Agent events arrive on the server's threads and are applied here, in order.
            cx.spawn(async move |cx| {
                while let Some(envelope) = received.next().await {
                    let applied =
                        window.update(cx, |workspace, _, cx| workspace.on_event(envelope, cx));
                    if applied.is_err() {
                        break;
                    }
                }
            })
            .detach();
            cx.spawn(async move |cx| {
                while update_events.next().await.is_some() {
                    let applied =
                        window.update(cx, |workspace, _, cx| workspace.on_update_change(cx));
                    if applied.is_err() {
                        break;
                    }
                }
            })
            .detach();
            cx.activate(true);
        });
}
