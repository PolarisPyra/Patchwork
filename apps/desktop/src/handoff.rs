//! What a development build keeps across a restart by Oxidation, built on [`patchwork_oxidation_client`].

// Rust guideline compliant 2026-10-09

use std::path::Path;
use std::time::Duration;

use futures::channel::mpsc;
use gpui::{App, AppContext as _, Bounds, Context, Pixels, Window, WindowBounds, point, px, size};
pub use patchwork_oxidation_client::Command;
use patchwork_oxidation_client::ClientSession;
use serde::{Deserialize, Serialize};

use crate::workspace::Workspace;

/// How long handed-over terminal pseudo-terminals may take to arrive.
#[cfg(unix)]
const FDS_WITHIN: Duration = Duration::from_secs(2);

/// How often a restored scroll is set again, until the page is tall enough to hold it.
const RESTORE_EVERY: Duration = Duration::from_millis(50);

/// How many times a restored scroll is set before it is left where the page allows.
const RESTORE_TRIES: usize = 40;

/// What the last run saved.
#[derive(Debug, Default)]
pub struct Saved {
    /// Where the window was.
    pub window: Option<WindowBounds>,
    /// The Settings page that was open, and how far it was scrolled.
    pub settings: Option<(String, f32)>,
    /// Whether the panel was open on the terminal.
    pub terminal_shown: bool,
    /// The terminals the last run handed over, each with its pseudo-terminal.
    #[cfg(unix)]
    pub terminals: Vec<(patchwork_backend::terminal::Handover, std::os::fd::OwnedFd)>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedState {
    window: Option<SavedWindow>,
    settings: Option<SavedSettings>,
    #[serde(default)]
    terminal_shown: bool,
    #[cfg(unix)]
    #[serde(default)]
    terminals: Vec<patchwork_backend::terminal::Handover>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedWindow {
    kind: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct SavedSettings {
    page: String,
    #[serde(default)]
    scroll: f32,
}

/// Reads what the last run saved, and removes the file so a later launch starts fresh.
pub fn take() -> Saved {
    let Some(session) = ClientSession::connect_debug() else {
        return Saved::default();
    };
    let Some(state) = session.take_state::<PersistedState>() else {
        return Saved::default();
    };
    Saved {
        #[cfg(unix)]
        terminals: take_terminals(&session, state.terminals),
        window: state.window.as_ref().and_then(window_from),
        terminal_shown: state.terminal_shown,
        settings: state.settings.map(|s| (s.page, s.scroll)),
    }
}

#[cfg(unix)]
fn take_terminals(
    session: &ClientSession,
    handed: Vec<patchwork_backend::terminal::Handover>,
) -> Vec<(patchwork_backend::terminal::Handover, std::os::fd::OwnedFd)> {
    if handed.is_empty() {
        return Vec::new();
    }
    let fds = match session.take_fds(handed.len(), FDS_WITHIN) {
        Ok(fds) => fds,
        Err(e) => {
            log::warn!(error:% = e; "could not take the terminals over: {{error}}");
            return Vec::new();
        }
    };
    if fds.len() != handed.len() {
        log::warn!(
            terminals:% = handed.len(),
            fds:% = fds.len();
            "{{terminals}} terminals were handed over, with {{fds}} pseudo-terminals"
        );
        return Vec::new();
    }
    handed.into_iter().zip(fds).collect()
}

fn window_from(saved: &SavedWindow) -> Option<WindowBounds> {
    let bounds = Bounds::new(
        point(px(saved.x), px(saved.y)),
        size(px(saved.width), px(saved.height)),
    );
    // A window too small to use was saved by mistake; the default size is better.
    if bounds.size.width < px(200.) || bounds.size.height < px(150.) {
        return None;
    }
    match saved.kind.as_str() {
        "maximized" => Some(WindowBounds::Maximized(bounds)),
        "fullscreen" => Some(WindowBounds::Fullscreen(bounds)),
        _ => Some(WindowBounds::Windowed(bounds)),
    }
}

fn window_to(bounds: WindowBounds) -> SavedWindow {
    let (kind, b): (&str, Bounds<Pixels>) = match bounds {
        WindowBounds::Windowed(b) => ("windowed", b),
        WindowBounds::Maximized(b) => ("maximized", b),
        WindowBounds::Fullscreen(b) => ("fullscreen", b),
    };
    SavedWindow {
        kind: kind.to_owned(),
        x: f32::from(b.origin.x),
        y: f32::from(b.origin.y),
        width: f32::from(b.size.width),
        height: f32::from(b.size.height),
    }
}

/// Reads the reloader's commands, in a debug build that one started.
pub fn listen() -> Option<mpsc::UnboundedReceiver<Command>> {
    let session = ClientSession::connect_debug()?;
    let (send, received) = mpsc::unbounded();
    session.listen_with(move |command| send.unbounded_send(command).is_ok());
    Some(received)
}

/// Puts back what [`take`] read once the window is open: the terminal panel, and the Settings
/// page and its scroll.
pub fn restore(
    saved: Saved,
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if saved.terminal_shown {
        workspace.toggle_terminal(window, cx);
    }
    let Some((page, scroll)) = saved.settings else {
        return;
    };
    workspace.open_settings(Some(&page), window, cx);
    let Some(dialog) = workspace.settings_dialog(cx) else {
        return;
    };
    // An offset is clamped to the page drawn, and a page that loads its rows grows after its
    // first frames, so it is set again until it holds.
    let offset = point(px(0.), px(scroll));
    cx.spawn(async move |_, cx| {
        for _ in 0..RESTORE_TRIES {
            let held = dialog.update(cx, |d, cx| {
                let held = d.scroll_offset() == offset;
                if !held {
                    d.set_scroll_offset(offset, cx);
                }
                held
            });
            if held {
                break;
            }
            cx.background_executor().timer(RESTORE_EVERY).await;
        }
    })
    .detach();
}

/// Acts on a command from the reloader.
pub fn apply(
    workspace: &mut Workspace,
    command: Command,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    match command {
        Command::Quit => {
            save(workspace, window, cx, false);
            cx.quit();
        }
        Command::Restart => {
            save(workspace, window, cx, true);
            cx.quit();
        }
        Command::Reload(path) => reload(&path, cx),
        Command::Errors(errors) => {
            workspace.build_errors = Some(errors.into());
            cx.notify();
        }
        Command::Fixed => {
            workspace.build_errors = None;
            cx.notify();
        }
    }
}

fn save(workspace: &Workspace, window: &Window, cx: &App, hand_over_terminals: bool) {
    let Some(session) = ClientSession::connect_debug() else {
        return;
    };
    #[cfg(unix)]
    let terminals = if hand_over_terminals {
        hand_over(workspace, &session)
    } else {
        Vec::new()
    };
    #[cfg(not(unix))]
    let _ = hand_over_terminals;
    let settings = workspace.settings_dialog(cx).map(|dialog| {
        let dialog = dialog.read(cx);
        SavedSettings {
            page: dialog.page().to_owned(),
            scroll: f32::from(dialog.scroll_offset().y),
        }
    });
    let state = PersistedState {
        window: Some(window_to(window.window_bounds())),
        settings,
        terminal_shown: workspace.terminal_shown(cx),
        #[cfg(unix)]
        terminals,
    };
    if let Err(e) = session.save_state(&state) {
        log::warn!(error:% = e; "could not save the state for the restart: {{error}}");
    }
}

#[cfg(unix)]
fn hand_over(
    workspace: &Workspace,
    session: &ClientSession,
) -> Vec<patchwork_backend::terminal::Handover> {
    let (handed, fds): (Vec<_>, Vec<_>) = workspace.core.terminals.hand_over().into_iter().unzip();
    if handed.is_empty() {
        return Vec::new();
    }
    match session.hand_over_fds(&fds) {
        Ok(true) => handed,
        Ok(false) => Vec::new(),
        Err(e) => {
            log::warn!(error:% = e; "could not hand the terminals over: {{error}}");
            Vec::new()
        }
    }
}

/// Draws a changed SVG file anew, or writes the theme presets out again, off the main thread,
/// for the theme watcher to apply.
fn reload(path: &Path, cx: &mut App) {
    log::info!(path:% = path.display(); "reloading {{path}}");
    if path.extension().is_some_and(|e| e == "svg") {
        patchwork_ui_theme::brand::svgs_changed();
        cx.refresh_windows();
        return;
    }
    cx.background_spawn(async { patchwork_backend::themes::init() })
        .detach();
}
