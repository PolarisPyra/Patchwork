//! What a running device can be asked beyond running the app: turning it, a place on Earth, a
//! network speed, saved states, installing an APK, and pairing a phone over Wi-Fi. A device's
//! menu starts them, and the ones that need typing are forms drawn in place of the device lists.

// Rust guideline compliant 2026-10-09

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, App, ClickEvent, Entity, FontWeight, Role, SharedString, Task, Window, div,
    prelude::*, px,
};
use patchwork_server::android::{Device, NetworkSpeed, StateAction, WifiDevice};

use super::state::AndroidPanel;
use crate::ui::menu::MenuItem;
use crate::ui::text_area::{Submitted, TextArea};
use crate::ui::theme::{FONT_MONO, FS_SM, RADIUS, Theme, theme};
use crate::ui::{BtnKind, btn, button, icon, icon_button, input_box, spinner, tooltip};

/// How often the network is asked which phones announce themselves while pairing is open.
const FIND_EVERY: Duration = Duration::from_secs(2);

/// A form drawn in place of the device lists.
pub(super) enum Sheet {
    /// Where on Earth the emulator `serial` should say it is.
    Location {
        serial: String,
        latitude: Entity<TextArea>,
        longitude: Entity<TextArea>,
    },
    /// The states saved for the emulator `serial`. `names` is None until they are read.
    States {
        serial: String,
        names: Option<Vec<String>>,
        name: Entity<TextArea>,
    },
    /// Pairing a phone over Wi-Fi: the phones `found` on the network, and the address and code
    /// one shows.
    Pair {
        found: Vec<WifiDevice>,
        address: Entity<TextArea>,
        code: Entity<TextArea>,
        _find: Task<()>,
    },
}

/// A one-line field of a form, whose Enter submits the form.
fn field(
    placeholder: &'static str,
    label: &'static str,
    cx: &mut Context<AndroidPanel>,
) -> Entity<TextArea> {
    let t = *theme(cx);
    let field = cx.new(|cx| {
        let mut field = TextArea::new(placeholder, label, cx);
        field.single_line = true;
        field.text_color = t.text;
        field.placeholder_color = crate::ui::theme::input_placeholder();
        field.selection_color = t.accent.opacity(0.3);
        field
    });
    cx.subscribe(&field, |this, _, _: &Submitted, cx| this.submit_sheet(cx))
        .detach();
    field
}

fn focus(field: &Entity<TextArea>, window: &mut Window, cx: &mut App) {
    window.focus(&gpui::Focusable::focus_handle(field.read(cx), cx), cx);
}

impl AndroidPanel {
    /// Opens the menu of what the running `device` can be asked.
    pub(super) fn open_device_menu(
        &mut self,
        device: &Device,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let serial = device.serial.clone();
        let mut items = Vec::new();
        if device.emulator {
            items.push(
                MenuItem::new("Rotate", {
                    let (this, serial) = (this.clone(), serial.clone());
                    move |_, cx| {
                        let serial = serial.clone();
                        let _ = this.update(cx, |this, cx| {
                            let root = this.project.root.clone();
                            this.device_action("Rotating", None, cx, move |server| {
                                server.android_rotate(&root, &serial)
                            });
                        });
                    }
                })
                .icon("arrow-clockwise"),
            );
            items.push(
                MenuItem::new("Set location…", {
                    let (this, serial) = (this.clone(), serial.clone());
                    move |window, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.open_location(serial.clone(), window, cx);
                        });
                    }
                })
                .icon("globe"),
            );
            items.push(
                MenuItem::new("Saved states…", {
                    let (this, serial) = (this.clone(), serial.clone());
                    move |window, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.open_states(serial.clone(), window, cx);
                        });
                    }
                })
                .icon("camera"),
            );
            for (i, speed) in NetworkSpeed::ALL.into_iter().enumerate() {
                let (this, serial) = (this.clone(), serial.clone());
                let item = MenuItem::new(format!("Network: {}", speed.label()), move |_, cx| {
                    let serial = serial.clone();
                    let _ = this.update(cx, |this, cx| {
                        let root = this.project.root.clone();
                        this.device_action(
                            "Setting the network speed",
                            Some(format!("Network: {}", speed.label())),
                            cx,
                            move |server| server.android_set_network(&root, &serial, speed),
                        );
                    });
                });
                items.push(if i == 0 { item.separated() } else { item });
            }
        }
        let key = super::state::device_key(device);
        let install = MenuItem::new("Install APK…", move |_, cx| {
            let _ = this.update(cx, |this, cx| {
                this.select(key.clone(), cx);
                this.choose_apk(cx);
            });
        })
        .icon("export");
        items.push(if items.is_empty() {
            install
        } else {
            install.separated()
        });
        self.open_menu(&device_menu(&serial), items, e, window, cx);
    }

    fn open_location(&mut self, serial: String, window: &mut Window, cx: &mut Context<Self>) {
        let latitude = field("Latitude, as 48.8566", "Latitude", cx);
        let longitude = field("Longitude, as 2.3522", "Longitude", cx);
        focus(&latitude, window, cx);
        self.sheet = Some(Sheet::Location {
            serial,
            latitude,
            longitude,
        });
        cx.notify();
    }

    fn open_states(&mut self, serial: String, window: &mut Window, cx: &mut Context<Self>) {
        let name = field("Name of a new state", "Name of a new state", cx);
        focus(&name, window, cx);
        self.sheet = Some(Sheet::States {
            serial,
            names: None,
            name,
        });
        self.load_states(cx);
        cx.notify();
    }

    /// Reads the saved states again while their form is open.
    pub(super) fn load_states(&mut self, cx: &mut Context<Self>) {
        let Some(Sheet::States { serial, .. }) = &self.sheet else {
            return;
        };
        let (server, root) = (Arc::clone(&self.server), self.project.root.clone());
        let serial = serial.clone();
        let task = cx.background_spawn(async move { server.android_saved_states(&root, &serial) });
        cx.spawn(async move |this, cx| {
            let read = task.await;
            this.update(cx, |this, cx| {
                let Some(Sheet::States { names, .. }) = &mut this.sheet else {
                    return;
                };
                match read {
                    Ok(read) => *names = Some(read),
                    Err(e) => {
                        *names = Some(Vec::new());
                        this.failure = Some(e);
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    fn saved_state(&mut self, action: StateAction, name: String, cx: &mut Context<Self>) {
        let Some(Sheet::States { serial, .. }) = &self.sheet else {
            return;
        };
        let (serial, root) = (serial.clone(), self.project.root.clone());
        let (doing, done) = match action {
            StateAction::Save => ("Saving the state", format!("Saved {name}")),
            StateAction::Load => ("Loading the state", format!("Loaded {name}")),
            StateAction::Delete => ("Deleting the state", format!("Deleted {name}")),
        };
        self.device_action(doing, Some(done), cx, move |server| {
            server.android_saved_state(&root, &serial, action, &name)
        });
    }

    pub(super) fn open_pair(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let address = field("Address, as 192.168.1.20:37251", "Pairing address", cx);
        let code = field("Six-digit code", "Pairing code", cx);
        focus(&address, window, cx);
        let find = cx.spawn(async move |this, cx| {
            loop {
                let Ok((server, root)) = this.update(cx, |this, _| {
                    (Arc::clone(&this.server), this.project.root.clone())
                }) else {
                    break;
                };
                let read = cx
                    .background_spawn(async move { server.android_wifi_devices(&root) })
                    .await;
                let open = this.update(cx, |this, cx| {
                    let Some(Sheet::Pair { found, .. }) = &mut this.sheet else {
                        return false;
                    };
                    // A network that cannot be searched still pairs by a typed address.
                    if let Ok(read) = read
                        && *found != read
                    {
                        *found = read;
                        cx.notify();
                    }
                    true
                });
                if !open.unwrap_or(false) {
                    break;
                }
                cx.background_executor().timer(FIND_EVERY).await;
            }
        });
        self.sheet = Some(Sheet::Pair {
            found: Vec::new(),
            address,
            code,
            _find: find,
        });
        cx.notify();
    }

    /// Connects to a phone this computer is already paired with.
    fn connect(&mut self, address: String, cx: &mut Context<Self>) {
        let root = self.project.root.clone();
        self.sheet = None;
        self.device_action(
            "Connecting to the phone",
            Some(format!("Connected to {address}")),
            cx,
            move |server| server.android_connect(&root, &address),
        );
    }

    /// Does what the open form asks, with what is typed in it.
    pub(super) fn submit_sheet(&mut self, cx: &mut Context<Self>) {
        match &self.sheet {
            Some(Sheet::Location {
                serial,
                latitude,
                longitude,
            }) => {
                let read = |field: &Entity<TextArea>| field.read(cx).content.trim().parse::<f64>();
                let (Ok(latitude), Ok(longitude)) = (read(latitude), read(longitude)) else {
                    self.failure = Some(
                        "Enter the latitude and the longitude as numbers, as 48.8566 and 2.3522."
                            .into(),
                    );
                    return cx.notify();
                };
                let (serial, root) = (serial.clone(), self.project.root.clone());
                self.sheet = None;
                self.device_action(
                    "Setting the location",
                    Some(format!("Location: {latitude}, {longitude}")),
                    cx,
                    move |server| server.android_set_location(&root, &serial, latitude, longitude),
                );
            }
            Some(Sheet::States { name, .. }) => {
                let field = name.clone();
                let name = field.read(cx).content.trim().to_string();
                if name.is_empty() {
                    return;
                }
                field.update(cx, |field, cx| field.set_text("", cx));
                self.saved_state(StateAction::Save, name, cx);
            }
            Some(Sheet::Pair { address, code, .. }) => {
                let address = address.read(cx).content.trim().to_string();
                let code = code.read(cx).content.trim().to_string();
                let root = self.project.root.clone();
                self.pair(cx, move |server| {
                    server.android_pair(&root, &address, &code)
                });
            }
            None => {}
        }
    }

    /// Pairs off the UI thread; the form stays open when the phone refuses, to try again.
    fn pair(
        &mut self,
        cx: &mut Context<Self>,
        act: impl FnOnce(&patchwork_server::Server) -> Result<(), patchwork_server::Error>
        + Send
        + 'static,
    ) {
        self.working = Some("Pairing with the phone".into());
        self.failure = None;
        self.notice = None;
        let server = Arc::clone(&self.server);
        let task = cx.background_spawn(async move { act(&server) });
        cx.spawn(async move |this, cx| {
            let paired = task.await;
            this.update(cx, |this, cx| {
                this.working = None;
                match paired {
                    Ok(()) => {
                        this.sheet = None;
                        this.notice = Some("Paired with the phone".into());
                    }
                    Err(e) => this.failure = Some(e),
                }
                this.refresh(cx);
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }

    pub(super) fn close_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sheet = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The open form drawn in place of the device lists.
    pub(super) fn sheet_view(
        &self,
        t: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let sheet = self.sheet.as_ref()?;
        let input = |field: &Entity<TextArea>, cx: &App| {
            let focused = gpui::Focusable::focus_handle(field.read(cx), cx).is_focused(window);
            input_box(focused, t)
                .font_family(FONT_MONO)
                .child(div().flex_1().min_w_0().child(field.clone()))
        };
        let (title, action, body): (&'static str, &'static str, Vec<AnyElement>) = match sheet {
            Sheet::Location {
                latitude,
                longitude,
                ..
            } => (
                "Set location",
                "Set",
                vec![
                    input(latitude, cx).into_any_element(),
                    input(longitude, cx).into_any_element(),
                ],
            ),
            Sheet::States { names, name, .. } => (
                "Saved states",
                "Save",
                vec![
                    self.states_list(names.as_deref(), t, cx),
                    input(name, cx).into_any_element(),
                ],
            ),
            Sheet::Pair {
                found,
                address,
                code,
                ..
            } => (
                "Pair over Wi-Fi",
                "Pair",
                vec![
                    self.found_list(found, t, cx),
                    input(address, cx).into_any_element(),
                    input(code, cx).into_any_element(),
                ],
            ),
        };
        let busy = self.working.is_some();
        Some(
            div()
                .id("android-sheet")
                .role(Role::Form)
                .aria_label(title)
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .id("android-sheet-title")
                        .role(Role::Heading)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .children(body)
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            btn("android-sheet-close", "Close", BtnKind::Plain, false, t)
                                .child("Close")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.close_sheet(window, cx);
                                })),
                        )
                        .child(
                            btn("android-sheet-submit", action, BtnKind::Primary, busy, t)
                                .child(action)
                                .when(!busy, |b| {
                                    b.on_click(cx.listener(|this, _, _, cx| this.submit_sheet(cx)))
                                }),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The saved states, each with loading and deleting it.
    fn states_list(
        &self,
        names: Option<&[String]>,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(names) = names else {
            return div()
                .id("android-states-reading")
                .role(Role::Status)
                .aria_label("Reading the saved states")
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child(spinner(14., t.text_muted))
                .child("Reading the saved states")
                .into_any_element();
        };
        if names.is_empty() {
            return div()
                .text_color(t.text_muted)
                .child("No saved states yet.")
                .into_any_element();
        }
        div()
            .id("android-states")
            .role(Role::List)
            .aria_label("Saved states")
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(names.iter().enumerate().map(|(i, name)| {
                div()
                    .id(("android-state", i))
                    .role(Role::ListItem)
                    .aria_label(name.clone())
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .min_h(px(32.))
                    .px(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(name.clone())),
                    )
                    .child(
                        icon_button(
                            ("android-state-load", i),
                            format!("Load {name}"),
                            "arrow-counter-clockwise",
                            14.,
                            cx,
                        )
                        .tooltip(tooltip("Load"))
                        .on_click(cx.listener({
                            let name = name.clone();
                            move |this, _, _, cx| {
                                this.saved_state(StateAction::Load, name.clone(), cx);
                            }
                        })),
                    )
                    .child(
                        icon_button(
                            ("android-state-delete", i),
                            format!("Delete {name}"),
                            "trash",
                            14.,
                            cx,
                        )
                        .tooltip(tooltip("Delete"))
                        .on_click(cx.listener({
                            let name = name.clone();
                            move |this, _, _, cx| {
                                this.saved_state(StateAction::Delete, name.clone(), cx);
                            }
                        })),
                    )
            }))
            .into_any_element()
    }

    /// The phones found on the network: one showing a code fills in its address, one already
    /// paired connects.
    fn found_list(&self, found: &[WifiDevice], t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        if found.is_empty() {
            return div()
                .text_size(px(FS_SM))
                .text_color(t.text_muted)
                .child(
                    "No phones found yet. On the phone, open Developer options → Wireless \
                     debugging → Pair device with pairing code.",
                )
                .into_any_element();
        }
        let hover = t.bg_hover;
        div()
            .id("android-found")
            .role(Role::List)
            .aria_label("Phones found")
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(found.iter().enumerate().map(|(i, phone)| {
                let does = if phone.pairing { "Use" } else { "Connect" };
                let picked = phone.clone();
                button(
                    ("android-found-phone", i),
                    format!("{does}: {}, {}", phone.name, phone.address),
                )
                .min_h(px(32.))
                .gap(px(8.))
                .rounded(px(RADIUS))
                .px(px(8.))
                .hover(move |s| s.bg(hover))
                .child(icon("plugs-connected", 16.).text_color(t.text_muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(SharedString::from(phone.name.clone())),
                )
                .child(
                    div()
                        .flex_none()
                        .font_family(FONT_MONO)
                        .text_size(px(FS_SM))
                        .text_color(t.text_muted)
                        .child(SharedString::from(phone.address.clone())),
                )
                .child(div().flex_none().text_color(t.accent_text).child(does))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !picked.pairing {
                        return this.connect(picked.address.clone(), cx);
                    }
                    if let Some(Sheet::Pair { address, .. }) = &this.sheet {
                        address
                            .clone()
                            .update(cx, |field, cx| field.set_text(&picked.address, cx));
                    }
                    cx.notify();
                }))
            }))
            .into_any_element()
    }
}

/// What the menu of the device `serial` is open under.
pub(super) fn device_menu(serial: &str) -> String {
    format!("{serial} actions")
}
