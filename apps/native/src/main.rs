//! Patchwork's native window: the same app as the desktop webview, drawn with GPUI over the same
//! server, folders and database.

mod add_project;
mod assets;
mod background_picker;
mod canvas;
mod chat;
mod highlight;
mod menu;
mod onboarding;
mod palette;
mod panel;
mod prompt;
mod settings;
mod sidebar;
mod sigils;
mod theme;
mod ui;
mod workspace;

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
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

use crate::theme::Theme;
use crate::workspace::Workspace;

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
        ToggleOutput
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
        if let Ok(mut file) = self.0.lock() {
            let _ = writeln!(file, "{time} [{level}] {}", record.args());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.0.lock() {
            let _ = file.flush();
        }
    }
}

fn start_logging(dirs: &Dirs) {
    let folder = patchwork_server::logging::session_dir(&dirs.logs);
    let opened = std::fs::create_dir_all(&folder).and_then(|_| {
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
        log::error!("panic: {info}");
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
                    log::info!("scheduled task started: {name}");
                }
                std::thread::sleep(SCHEDULED_TICK);
            }
        })
        .map(|_| ())
        .unwrap_or_else(|e| log::warn!("scheduled tasks will not run: {e}"));
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
    let text_system = cx.text_system().clone();
    cx.background_spawn(async move {
        for font in fonts {
            text_system.prewarm_fonts(&[font]);
        }
    })
    .detach();
}

fn home_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(fallback)))
}

fn main() {
    // The platform's folders, as the desktop app reads them, so both open the same data.
    let (Some(config), Some(data), Some(cache)) = (
        home_dir("XDG_CONFIG_HOME", ".config"),
        home_dir("XDG_DATA_HOME", ".local/share"),
        home_dir("XDG_CACHE_HOME", ".cache"),
    ) else {
        log::error!("Patchwork needs HOME to find its folders");
        return;
    };
    let dirs = Dirs::new(&config, &data, &cache);
    start_logging(&dirs);
    let (events, mut received) = mpsc::unbounded();
    let server = match Server::open(
        dirs,
        &data.join("dev.patchwork.ade"),
        Box::new(move |event| {
            let _ = events.unbounded_send(event);
        }),
    ) {
        Ok(server) => server,
        Err(e) => {
            log::error!("could not open Patchwork's data: {e}");
            return;
        }
    };
    start_scheduled_tasks(Arc::clone(&server));

    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            if let Err(e) = cx.text_system().add_fonts(assets::fonts()) {
                log::warn!("could not load the fonts: {e}");
            }
            prewarm_fonts(cx);
            // Grayscale, as the web view draws text (`-webkit-font-smoothing: antialiased`).
            cx.set_text_rendering_mode(TextRenderingMode::Grayscale);
            settings::shortcuts::bind_keys(&server, cx);
            cx.set_global(sigils::load(&server));
            cx.set_prompt_builder(prompt::build);
            cx.on_action(|_: &Quit, cx| cx.quit());
            let quitting = Arc::clone(&server);
            cx.on_app_quit(move |_| {
                quitting.stop_agents();
                quitting.terminals.kill_all();
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
                    log::error!("could not open the window: {e}");
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
            cx.activate(true);
        });
}
