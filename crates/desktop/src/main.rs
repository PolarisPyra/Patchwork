//! Patchwork's window, drawn with GPUI over the core, its folders and its database.

// A release build on Windows opens no console window beside the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![expect(
    clippy::unused_self,
    reason = "a view acts through its `Context<Self>`, so some of its methods never read the view itself"
)]

// Rust guideline compliant 2026-10-09

mod android;
mod quilts;
mod bots;
mod canvas;
mod chat;
mod onboarding;
mod orchestrator;
mod sidebar;
mod workspace;

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{
    App, Bounds, Window, WindowHandle, Font, FontStyle, FontWeight, TextRenderingMode,
    TitlebarOptions, WindowBounds, WindowOptions, font, prelude::*, px, size,
};
use patchwork_core::Core;
use patchwork_core::system::Dirs;

use patchwork_ui::{APP_TITLE, Quit, assets, prompt, sigils, theme};
use crate::workspace::Workspace;

/// The app's allocator, which is faster than the system one when laying out and shaping text.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// How often a scheduled task is looked for. A tick that finds nothing to start does no work.
const SCHEDULED_TICK: Duration = Duration::from_secs(30);

/// Runs the helper the first argument names and returns its exit code, or None to open the app.
/// Without this, every hook or relay an agent starts would open another Patchwork window.
fn run_helper() -> Option<i32> {
    use patchwork_core::{harness, permissions, provider, workspace};
    // Git and ssh start this binary to ask a sign-in question, with the question alone.
    if std::env::var_os(workspace::ASKPASS_ENV).is_some() && std::env::args().len() == 2 {
        return Some(workspace::run_askpass());
    }
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

/// Starts the tasks whose time has come, on a timer of its own, for the life of the app.
fn start_scheduled_tasks(core: Arc<Core>) {
    std::thread::Builder::new()
        .name("scheduled-tasks".into())
        .spawn(move || {
            loop {
                for name in core.run_due_scheduled_tasks() {
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

/// Applies each message `received` brings to the window's workspace, in order, until the
/// channel or the window closes.
fn forward<T: 'static>(
    window: WindowHandle<Workspace>,
    mut received: mpsc::UnboundedReceiver<T>,
    cx: &App,
    apply: impl Fn(&mut Workspace, T, &mut Window, &mut Context<Workspace>) + 'static,
) {
    cx.spawn(async move |cx| {
        while let Some(message) = received.next().await {
            let applied = window.update(cx, |workspace, window, cx| {
                apply(workspace, message, window, cx);
            });
            if applied.is_err() {
                break;
            }
        }
    })
    .detach();
}

fn main() {
    // Before anything holds a key: the app, and the helpers that are handed one.
    let dumps = patchwork_core::system::memory::deny_dumps();
    // Agents start this binary as a helper, without the window: a hook, a relay or a bridge.
    if let Some(code) = run_helper() {
        std::process::exit(code);
    }
    // Before the app's memory grows; the helpers above are left as they were.
    let pages = patchwork_core::system::memory::small_pages();
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
    if patchwork_core::system::instance::raise_running(&dirs.data) {
        return;
    }
    patchwork_core::logging::start_main_log(&dirs.logs);
    if let Err(e) = dumps {
        log::warn!(error:% = e; "a crash could write Patchwork's memory to a core file: {{error}}");
    }
    if let Err(e) = pages {
        log::warn!(error:% = e; "Patchwork's memory may be counted in huge pages: {{error}}");
    }
    let (events, received) = mpsc::unbounded();
    let core = match Core::open(
        &dirs,
        // A debug build keeps its chat folders apart from the released app's, as it does its data.
        &data.join(if cfg!(debug_assertions) {
            "dev.patchwork.ade-dev"
        } else {
            "dev.patchwork.ade"
        }),
        Box::new(move |event| {
            let _ = events.unbounded_send(event);
        }),
    ) {
        Ok(core) => core,
        Err(e) => {
            log::error!(error:% = e; "could not open Patchwork's data: {{error}}");
            return;
        }
    };
    start_scheduled_tasks(Arc::clone(&core));
    // A second launch asks on the relay's thread; the window is shown on the app's.
    let (raise, raised) = mpsc::unbounded::<()>();
    patchwork_core::provider::mcp::set_raise_handler(Box::new(move || {
        let _ = raise.unbounded_send(());
    }));
    // Git asks for a sign-in on the thread that runs it; the question is shown on the app's.
    let (ask, asked) = mpsc::unbounded();
    patchwork_core::workspace::set_credential_prompter(Box::new(move |request| {
        // With no window left to ask in, the request drops and the command fails.
        let _ = ask.unbounded_send(request);
    }));
    // A change in where updating stands is drawn by the window, like an agent's events.
    let (update_sender, update_events) = mpsc::unbounded::<()>();
    core.on_update_change(Box::new(move || {
        let _ = update_sender.unbounded_send(());
    }));
    core.start_update_checks();
    core.start_limit_resumes();
    core.start_worktree_cleanup();
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            if let Err(e) = cx.text_system().add_fonts(assets::fonts()) {
                log::warn!(error:% = e; "could not load the fonts: {{error}}");
            }
            prewarm_fonts(cx);
            cx.background_spawn(async { patchwork_ui::highlight::warm() })
                .detach();
            // Grayscale text, which reads lighter than subpixel on the dark theme.
            cx.set_text_rendering_mode(TextRenderingMode::Grayscale);
            patchwork_settings::bind_keys(&core, cx);
            cx.set_global(sigils::load(&core));
            patchwork_settings::show_app_icon(&core.settings(), cx);
            cx.set_prompt_builder(prompt::build);
            cx.on_action(|_: &Quit, cx| cx.quit());
            let quitting = Arc::clone(&core);
            cx.on_app_quit(move |_| {
                quitting.stop_agents();
                quitting.terminals.kill_all();
                patchwork_core::system::ssh::close_all();
                async {}
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(1600.), px(960.)), cx);
            let applied = core.applied_theme();
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(APP_TITLE.into()),
                        ..Default::default()
                    }),
                    app_id: Some("patchwork".into()),
                    ..Default::default()
                },
                |window, cx| {
                    theme::apply(applied, window.appearance(), cx);
                    cx.new(|cx| Workspace::new(Arc::clone(&core), window, cx))
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
            let resuming = Arc::clone(&core);
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
            // Patchwork notifies while its window is out of focus, and a click on a notification
            // opens its chat.
            let _ = window.update(cx, |_, window, cx| {
                let focused = Arc::clone(&core);
                focused.set_window_focused(window.is_window_active());
                focused.note_activity();
                cx.observe_window_activation(window, move |_, window, _| {
                    focused.set_window_focused(window.is_window_active());
                    focused.note_activity();
                })
                .detach();
            });
            // A vault that locks when Patchwork is left alone counts from the last key.
            let typed = Arc::clone(&core);
            cx.intercept_keystrokes(move |_, _, _| typed.note_activity())
                .detach();
            cx.on_system_notification_response(move |response, cx| {
                let thread_id = response
                    .tag
                    .split_once('|')
                    .map_or(response.tag.as_ref(), |(id, _)| id)
                    .to_string();
                let _ = window.update(cx, |workspace, window, cx| {
                    cx.activate(true);
                    window.activate_window();
                    workspace.open_thread(&thread_id, cx);
                });
            });
            forward(window, raised, cx, |_, (), window, cx| {
                log::info!("a second launch asked for this window");
                cx.activate(true);
                window.activate_window();
            });
            forward(window, asked, cx, |workspace, request, window, cx| {
                workspace.ask_credential(request, window, cx);
            });
            // Agent events arrive on the core's threads and are applied here, in order.
            forward(window, received, cx, Workspace::on_event);
            forward(window, update_events, cx, |workspace, (), _, cx| {
                workspace.on_update_change(cx);
            });
            cx.activate(true);
        });
}
