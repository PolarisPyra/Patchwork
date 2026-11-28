//! The hot reload bar in the toolbar, shown while `cargo run desktop` runs the app under dx: what
//! dx is doing, and buttons to apply changes (F5), pause hot reload (F7) and rebuild and restart
//! the app (Shift+F5). Saves apply by themselves; F5 is for one that was missed, or held by a
//! pause.
//!
//! `scripts/dx-serve.py` runs dx and reports what it is doing over the Unix socket it names in
//! `PATCHWORK_DX_CONTROL`; the shortcuts go back over the same socket, and it types them into dx.

// Rust guideline compliant 2026-10-09

use gpui::component::Sizable as _;
use gpui::component::spinner::Spinner;
use std::io::Write as _;
use std::time::Duration;

use gpui::{AnyElement, App, Global, Role, Task, div, prelude::*, px};
use patchwork_design_components::button;
use patchwork_design_icons::{IconName, icon};
use patchwork_design_theme::{Theme, fs_xs, radius};

/// The bar's height, a step under the toolbar's buttons.
const HEIGHT: f32 = 28.;
/// The size of the bar's buttons.
const BUTTON: f32 = 22.;
/// How long the bar says "Hot-patched" after a patch applies.
const PATCHED_FOR: Duration = Duration::from_secs(2);

#[cfg(unix)]
type Control = std::os::unix::net::UnixStream;
#[cfg(not(unix))]
type Control = std::fs::File;

/// What dx is doing, as `dx-serve.py` last said.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Build {
    Starting,
    Building,
    Failed,
    Ready,
}

/// The connection to `dx-serve.py`, and what it last said.
struct DevServer {
    control: Option<Control>,
    /// Whether the app was started by `dx-serve.py`, so the bar stays up after dx stops.
    started_by_dx: bool,
    build: Build,
    /// Whether hot reload is paused, holding saves until F5 applies them or it resumes.
    paused: bool,
    /// Whether the bar still shows the last patch.
    patched: bool,
    /// Ends the "Hot-patched" flash, restarted by each patch.
    patched_timer: Option<Task<()>>,
    _reader: Option<Task<()>>,
}

impl Global for DevServer {}

/// Connects to `dx-serve.py` when it started the app, and takes F5, Shift+F5 and F7 for dx.
pub(crate) fn init(cx: &mut App) {
    let control = connect();
    let reader = control.as_ref().and_then(|c| read_state(c, cx));
    let has_control = control.is_some();
    cx.set_global(DevServer {
        control,
        started_by_dx: has_control,
        build: Build::Starting,
        paused: false,
        patched: false,
        patched_timer: None,
        _reader: reader,
    });
    if has_control {
        cx.intercept_keystrokes(|e, _, cx| {
            let k = &e.keystroke;
            let m = k.modifiers;
            let shift_only = m.shift && !(m.control || m.alt || m.platform || m.function);
            let command = match (k.key.as_str(), m.modified(), shift_only) {
                ("f5", false, _) => "apply",
                ("f5", _, true) => "restart",
                ("f7", false, _) => "toggle-paused",
                _ => return,
            };
            send(command, cx);
            cx.stop_propagation();
        })
        .detach();
    }
}

#[cfg(unix)]
fn connect() -> Option<Control> {
    let path = std::env::var_os("PATCHWORK_DX_CONTROL")?;
    Control::connect(&path)
        .inspect_err(|e| log::warn!(error:% = e; "could not reach dx-serve.py: {{error}}"))
        .ok()
}

#[cfg(not(unix))]
fn connect() -> Option<Control> {
    None
}

/// Follows the `paused` and `watching` lines `dx-serve.py` sends.
#[cfg(unix)]
fn read_state(control: &Control, cx: &mut App) -> Option<Task<()>> {
    use futures::StreamExt as _;
    use std::io::BufRead as _;

    let reader = control.try_clone().ok()?;
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    std::thread::Builder::new()
        .name("dx-serve".into())
        .spawn(move || {
            for line in std::io::BufReader::new(reader).lines() {
                let Ok(line) = line else { break };
                if tx.unbounded_send(line).is_err() {
                    break;
                }
            }
        })
        .ok()?;
    Some(cx.spawn(async move |cx| {
        while let Some(line) = rx.next().await {
            cx.update(|cx| {
                take_line(line.trim(), cx);
                cx.refresh_windows();
            });
        }
        cx.update(|cx| {
            cx.global_mut::<DevServer>().control = None;
            cx.refresh_windows();
        });
    }))
}

/// Takes one line from `dx-serve.py`: `paused`, `watching`, `build <state>` or `patched`.
#[cfg(unix)]
fn take_line(line: &str, cx: &mut App) {
    let dev = cx.global_mut::<DevServer>();
    match line {
        "paused" => dev.paused = true,
        "watching" => dev.paused = false,
        "build starting" => dev.build = Build::Starting,
        "build building" => dev.build = Build::Building,
        "build failed" => dev.build = Build::Failed,
        "build ready" => dev.build = Build::Ready,
        "patched" => {
            let timer = cx.spawn(async move |cx| {
                cx.background_executor().timer(PATCHED_FOR).await;
                cx.update(|cx| {
                    cx.global_mut::<DevServer>().patched = false;
                    cx.refresh_windows();
                });
            });
            let dev = cx.global_mut::<DevServer>();
            dev.patched = true;
            dev.patched_timer = Some(timer);
        }
        _ => {}
    }
}

#[cfg(not(unix))]
fn read_state(_: &Control, _: &mut App) -> Option<Task<()>> {
    None
}

/// Has `dx-serve.py` type the shortcut for `command` into dx.
fn send(command: &str, cx: &mut App) {
    let dev = cx.global_mut::<DevServer>();
    let Some(control) = dev.control.as_mut() else {
        return;
    };
    if let Err(e) = writeln!(control, "{command}") {
        log::warn!(error:% = e; "could not reach dx-serve.py: {{error}}");
        dev.control = None;
        cx.refresh_windows();
    }
}

/// The bar, while `dx-serve.py` runs the app.
pub(crate) fn bar(t: &Theme, cx: &App) -> Option<AnyElement> {
    let dev = cx.try_global::<DevServer>()?;
    if !dev.started_by_dx {
        return None;
    }
    let busy = dev.control.is_some() && matches!(dev.build, Build::Starting | Build::Building);
    let (detail, dot, text) = match dev.build {
        _ if dev.control.is_none() => ("dx stopped", t.text_subtle, t.text_muted),
        Build::Starting => ("Starting", t.accent, t.text_muted),
        Build::Building => ("Building", t.accent, t.text_muted),
        Build::Failed => ("Build failed", t.danger, t.danger),
        Build::Ready if dev.patched => ("Hot-patched", t.success, t.text_muted),
        _ if dev.paused => ("Paused", t.warning, t.warning),
        Build::Ready => ("Ready", t.success, t.text_muted),
    };
    let indicator = div()
        .size(px(10.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(if busy {
            Spinner::new()
                .with_size(gpui::px(10.))
                .color(t.text_muted)
                .into_any_element()
        } else {
            div().size(px(6.)).rounded_full().bg(dot).into_any_element()
        });
    let controls = dev.control.is_some().then(|| {
        let paused = dev.paused;
        let (pause_label, pause_icon) = if paused {
            ("Resume hot reload (F7)", IconName::Play)
        } else {
            ("Pause hot reload (F7)", IconName::Pause)
        };
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(div().w(px(1.)).h(px(14.)).mr(px(4.)).bg(t.border))
            .child(
                ghost("dx-apply", "Apply changes (F5)", IconName::Check, false, t)
                    .on_click(|_, _, cx| send("apply", cx)),
            )
            .child(
                ghost("dx-pause", pause_label, pause_icon, paused, t)
                    .on_click(|_, _, cx| send("toggle-paused", cx)),
            )
            .child(
                ghost(
                    "dx-restart",
                    "Rebuild and restart (Shift+F5)",
                    IconName::ArrowClockwise,
                    false,
                    t,
                )
                .on_click(|_, _, cx| send("restart", cx)),
            )
    });
    Some(
        div()
            .id("hot-reload")
            .role(Role::Status)
            .aria_label(format!("Hot reload: {detail}"))
            .h(px(HEIGHT))
            .pl(px(10.))
            .pr(px(if controls.is_some() { 3. } else { 10. }))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .child(indicator)
            .child(
                div()
                    .max_w(px(220.))
                    .truncate()
                    .text_size(px(fs_xs()))
                    .line_height(px(16.))
                    .text_color(text)
                    .child(detail),
            )
            .children(controls)
            .into_any_element(),
    )
}

/// One of the bar's icon buttons; `lit` colours it while its setting is off.
fn ghost(
    id: &'static str,
    label: &'static str,
    name: IconName,
    lit: bool,
    t: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let (hover, fg) = (t.bg_hover, t.text);
    button(id, label)
        .size(px(BUTTON))
        .justify_center()
        .rounded(px(radius() - 2.))
        .text_color(if lit { t.warning } else { t.text_subtle })
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, 14.))
}
