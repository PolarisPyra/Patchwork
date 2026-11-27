//! Settings → Devices: the server inside the app that phones and other computers reach, how to
//! pair one, and the devices already paired.
//!
//! Every call to the server's API may touch disk, so each runs off the UI thread through
//! [`SettingsDialog::run`], and the page draws what came back.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use gpui::{
    AnyElement, ClipboardItem, Div, FontWeight, Role, SharedString, Stateful, div, prelude::*, px,
};
use patchwork_backend::server::{Device, PairCode, ServerInfo, qr_modules};
use crate::settings::SettingsDialog;
use crate::settings::controls::{Section, alert, switch, token_row};
use patchwork_ui_theme::{Theme, font_mono, fs_sm, fs_xl, fs_xs, radius, theme};
use patchwork_ui_components::{BtnKind, btn};

/// How big one square of the QR code is drawn. Small enough that a version-10 code (57 squares)
/// stays inside 200px, as on the web, large enough for a phone camera to read off a screen.
const QR_MODULE: f32 = 3.;

/// How wide the QR code's box is when there is no code to draw, matching the web's 200px.
const QR_BOX: f32 = 200.;

/// How long "Copied" shows on a copy button before it reads "Copy" again.
const COPIED_FOR: Duration = Duration::from_millis(1500);

/// Every how many seconds the devices are read again while a pairing code is shown.
const RELOAD_EVERY: u32 = 3;

/// What the Devices page knows of the server and its devices.
#[derive(Default)]
pub struct DevicesState {
    /// Whether the first read has been asked for.
    started: bool,
    info: Option<ServerInfo>,
    /// The paired devices; None until read.
    devices: Option<Vec<Device>>,
    /// The pairing code made last, while it lasts.
    code: Option<PairCode>,
    /// How many devices were paired when the code was made; one more means it was claimed.
    paired_before: usize,
    /// The QR code's squares for the code's link, read when the code is made.
    qr: Option<Vec<Vec<bool>>>,
    /// A code is being made.
    pairing: bool,
    /// The device whose Revoke was clicked once and now asks to be sure.
    confirm: Option<String>,
    /// Which copy button says "Copied", by its id.
    copied: Option<SharedString>,
    error: Option<String>,
}

/// Now, in milliseconds since the Unix epoch, as the server's times are given.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// "4:59" for the time left until `expires_at_ms`.
fn time_left(expires_at_ms: i64) -> String {
    let secs = (expires_at_ms - now_ms()).max(0) / 1000;
    format!("{}:{:02}", secs / 60, secs % 60)
}

impl SettingsDialog {
    /// Reads the server's addresses and the paired devices again.
    fn load_devices(&mut self, cx: &mut Context<Self>) {
        self.devices.started = true;
        let core = Arc::clone(&self.core);
        self.run(
            move || (core.server_info(), core.devices()),
            |d, (info, devices), _| {
                d.devices.info = Some(info);
                match devices {
                    Ok(list) => {
                        // A code is good for one device, so once one more has paired it is spent.
                        if list.len() > d.devices.paired_before {
                            d.devices.code = None;
                            d.devices.qr = None;
                        }
                        d.devices.devices = Some(list);
                    }
                    Err(e) => d.devices.error = Some(e.to_string()),
                }
            },
            cx,
        );
    }

    /// Makes a pairing code, draws its link as a QR code, and counts down until it lapses.
    fn make_code(&mut self, cx: &mut Context<Self>) {
        self.devices.pairing = true;
        self.devices.error = None;
        let core = Arc::clone(&self.core);
        self.run(
            move || {
                core.pair_start(None).map(|code| {
                    let qr = qr_modules(&code.link);
                    (code, qr)
                })
            },
            |d, made, cx| {
                d.devices.pairing = false;
                match made {
                    Ok((made_code, made_qr)) => {
                        d.devices.paired_before = d.devices.devices.as_ref().map_or(0, Vec::len);
                        d.devices.code = Some(made_code);
                        d.devices.qr = made_qr;
                        d.tick_code(cx);
                    }
                    Err(e) => d.devices.error = Some(e.to_string()),
                }
            },
            cx,
        );
    }

    /// Redraws once a second while the pairing code lasts, so its countdown moves, reads the
    /// devices every few seconds so a device that pairs shows up, and drops the code once it has
    /// lapsed or been claimed.
    fn tick_code(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            for tick in 1_u32.. {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let alive = this.update(cx, |d, cx| {
                    if tick % RELOAD_EVERY == 0 {
                        d.load_devices(cx);
                    }
                    let live = d
                        .devices
                        .code
                        .as_ref()
                        .is_some_and(|c| c.expires_at_ms > now_ms());
                    if !live {
                        d.devices.code = None;
                        d.devices.qr = None;
                    }
                    cx.notify();
                    live
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Turns network access on or off; the server listens anew after a restart.
    fn set_network(&mut self, on: bool, cx: &mut Context<Self>) {
        self.devices.error = None;
        let core = Arc::clone(&self.core);
        self.run(
            move || core.set_server_network(on).map(|()| core.server_info()),
            |d, out, _| match out {
                Ok(info) => d.devices.info = Some(info),
                Err(e) => d.devices.error = Some(e.to_string()),
            },
            cx,
        );
    }

    /// Cuts the device `id` off: its token stops working at once.
    fn revoke(&mut self, id: String, cx: &mut Context<Self>) {
        self.devices.confirm = None;
        self.devices.error = None;
        let core = Arc::clone(&self.core);
        self.run(
            move || core.revoke_device(&id).and_then(|()| core.devices()),
            |d, out, _| match out {
                Ok(list) => d.devices.devices = Some(list),
                Err(e) => d.devices.error = Some(e.to_string()),
            },
            cx,
        );
    }

    /// Copies `text` and has the button `id` say so for a moment.
    fn copy(&mut self, id: SharedString, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.devices.copied = Some(id.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            let _ = this.update(cx, |d, cx| {
                if d.devices.copied.as_ref() == Some(&id) {
                    d.devices.copied = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// A small Copy button that copies `text`, reading "Copied" just after.
fn copy_button(
    d: &SettingsDialog,
    id: &str,
    label: &str,
    text: String,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let id = SharedString::from(format!("devices-copy-{id}"));
    let shown = if d.devices.copied.as_ref() == Some(&id) {
        "Copied"
    } else {
        label
    };
    btn(
        id.clone(),
        format!("{label} {text}"),
        BtnKind::Plain,
        false,
        t,
    )
    .flex_none()
    .child(shown.to_string())
    .on_click(cx.listener(move |d, _, _, cx| d.copy(id.clone(), text.clone(), cx)))
}

/// Text in the code font, cut short with an ellipsis when it does not fit.
fn code_text(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .min_w_0()
        .max_w(px(260.))
        .truncate()
        .font_family(font_mono())
        .text_size(px(fs_xs()))
        .text_color(t.text_muted)
        .child(text.into())
}

/// The QR code's squares, black on white so any camera reads it in either theme.
fn qr_code(modules: &[Vec<bool>]) -> Div {
    let black = gpui::black();
    div()
        .flex_none()
        .flex()
        .flex_col()
        .p(px(QR_MODULE * 4.))
        .rounded(px(radius()))
        .bg(gpui::white())
        .children(modules.iter().map(|row| {
            div().flex().children(
                row.iter()
                    .map(|&on| div().size(px(QR_MODULE)).when(on, |d| d.bg(black))),
            )
        }))
}

/// The box that stands in for the QR code, saying why there is none.
fn qr_placeholder(text: &'static str, t: &Theme) -> Div {
    div()
        .flex_none()
        .size(px(QR_BOX))
        .flex()
        .items_center()
        .justify_center()
        .p(px(16.))
        .rounded(px(radius()))
        .border_1()
        .border_dashed()
        .border_color(t.border_strong)
        .child(
            // Its own block, so the words wrap inside the box rather than run past it.
            div()
                .w_full()
                .text_center()
                .text_size(px(fs_sm()))
                .text_color(t.text_subtle)
                .child(text),
        )
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    let t = *theme(cx);
    if !d.devices.started {
        d.load_devices(cx);
    }
    let info = d.devices.info.clone().unwrap_or_default();
    let reading = d.devices.info.is_none();
    let network = info.network;

    // This computer.
    let address = if info.address.is_empty() {
        if reading { "…" } else { "Not running" }.to_string()
    } else {
        info.address.clone()
    };
    let address_row = token_row("Address", &t).child(code_text(address, &t));
    let token_row_el = token_row("Token", &t)
        .child(code_text(
            if info.token.is_empty() {
                "…".to_string()
            } else {
                info.token.clone()
            },
            &t,
        ))
        .when(!info.token.is_empty(), |r| {
            r.child(copy_button(d, "token", "Copy", info.token.clone(), &t, cx))
        });
    let network_row = token_row("Let devices on my network connect", &t).child(
        switch(
            "devices-network",
            "Let devices on my network connect",
            network,
            &t,
        )
        .on_click(cx.listener(move |d, _, _, cx| d.set_network(!network, cx))),
    );
    let restart = info.restart_needed.then(|| {
        div()
            .id("devices-restart")
            .role(Role::Status)
            .px(px(16.))
            .py(px(10.))
            .bg(t.warning_soft)
            .text_size(px(fs_sm()))
            .text_color(t.warning)
            .child("Restart Patchwork for the network setting to take effect.")
    });
    let server_error = info.error.clone().map(|e| {
        div()
            .id("devices-server-error")
            .role(Role::Alert)
            .px(px(16.))
            .py(px(10.))
            .bg(t.danger_soft)
            .text_size(px(fs_sm()))
            .text_color(t.danger)
            .child(format!("The server is not running: {e}"))
    });
    let this_computer = Section::new("This computer")
        .child(address_row)
        .child(token_row_el)
        .child(network_row)
        .children(restart)
        .children(server_error);

    // Pair a device.
    let code = d.devices.code.clone();
    let qr = match (&code, &d.devices.qr) {
        _ if !network => qr_placeholder("Turn on network access to pair a device.", &t),
        (Some(_), Some(modules)) => qr_code(modules),
        (Some(_), None) => qr_placeholder("The link is too long to draw. Copy it instead.", &t),
        (None, _) => qr_placeholder("Make a pairing code to show its QR code.", &t),
    };
    let addresses: Vec<_> = info
        .urls
        .iter()
        .enumerate()
        .map(|(i, url)| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().min_w_0().child(code_text(url.clone(), &t)))
                .child(copy_button(
                    d,
                    &format!("url-{i}"),
                    "Copy",
                    url.clone(),
                    &t,
                    cx,
                ))
        })
        .collect();
    let pairing = d.devices.pairing;
    let make = btn(
        "devices-make-code",
        "Make a pairing code",
        if code.is_some() {
            BtnKind::Plain
        } else {
            BtnKind::Primary
        },
        pairing,
        &t,
    )
    .flex_none()
    .child(if pairing {
        "Making a code…"
    } else if code.is_some() {
        "Make a new code"
    } else {
        "Make a pairing code"
    })
    .when(!pairing, |b| {
        b.on_click(cx.listener(|d, _, _, cx| d.make_code(cx)))
    });
    let shown_code = code.as_ref().map(|c| {
        div()
            .flex()
            .items_baseline()
            .gap(px(10.))
            .child(
                div()
                    .id("devices-code")
                    .role(Role::Status)
                    .aria_label(format!("Pairing code {}", c.code))
                    .rounded(px(radius()))
                    .bg(t.bg_selected_muted)
                    .px(px(10.))
                    .py(px(4.))
                    .font_family(font_mono())
                    .text_size(px(fs_xl()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.text)
                    .child(c.code.clone()),
            )
            .child(
                div()
                    .text_size(px(fs_sm()))
                    .text_color(t.text_subtle)
                    .child(format!("expires in {}", time_left(c.expires_at_ms))),
            )
    });
    let copy_link = code
        .as_ref()
        .map(|c| copy_button(d, "link", "Copy link", c.link.clone(), &t, cx));
    let pair = Section::new("Pair a device").child(
        div().flex().gap(px(16.)).p(px(16.)).child(qr).child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(12.))
                .when(!addresses.is_empty(), |col| {
                    col.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(fs_sm()))
                                    .text_color(t.text_subtle)
                                    .child("Addresses"),
                            )
                            .children(addresses),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .child(make)
                        .children(copy_link),
                )
                .children(shown_code),
        ),
    );

    // Paired devices.
    let confirm = d.devices.confirm.clone();
    let rows: Vec<AnyElement> = match &d.devices.devices {
        None => vec![
            div()
                .px(px(16.))
                .py(px(24.))
                .text_center()
                .text_color(t.text_subtle)
                .child("Reading the devices…")
                .into_any_element(),
        ],
        Some(list) if list.is_empty() => vec![
            div()
                .px(px(16.))
                .py(px(24.))
                .text_center()
                .text_color(t.text_subtle)
                .child("No devices yet. Pair one above.")
                .into_any_element(),
        ],
        Some(list) => list
            .iter()
            .map(|device| {
                let sure = confirm.as_deref() == Some(device.id.as_str());
                let id = device.id.clone();
                token_row(device.name.clone(), &t)
                    .child(
                        btn(
                            SharedString::from(format!("devices-revoke-{}", device.id)),
                            format!("Revoke {}", device.name),
                            BtnKind::Plain,
                            false,
                            &t,
                        )
                        .text_color(t.danger)
                        .child(if sure { "Cut it off?" } else { "Revoke" })
                        .on_click(cx.listener(move |d, _, _, cx| {
                            if sure {
                                d.revoke(id.clone(), cx);
                            } else {
                                d.devices.confirm = Some(id.clone());
                                cx.notify();
                            }
                        })),
                    )
                    .into_any_element()
            })
            .collect(),
    };
    let paired = Section::new("Paired devices").children(rows);

    div()
        .flex()
        .flex_col()
        .children(
            d.devices
                .error
                .clone()
                .map(|e| alert("devices-error", e, t.danger).pt_0().pb(px(12.))),
        )
        .child(this_computer)
        .child(pair)
        .child(paired)
        .into_any_element()
}
