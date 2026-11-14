//! The panel's rows: a virtual device with its status and controls, and a connected device.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, ClickEvent, ElementId, Role, SharedString, Stateful, Toggled, Window, div,
    prelude::*, px,
};
use patchwork_server::android::{self, Avd, Boot, Device, DeviceState};

use super::state::{AndroidPanel, Busy, Confirm, Run, device_key};
use super::tools::device_menu;
use crate::ui::menu::{Menu, MenuItem};
use crate::ui::theme::{FS_SM, radius, Theme, theme};
use crate::ui::{BtnKind, btn, button, icon, icon_button, spinner, tooltip};

/// A row that is one of several choices: a dot, filled when it is the one picked, then what its
/// owner adds.
pub(super) fn radio_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    t: &Theme,
) -> Stateful<gpui::Div> {
    let hover = t.bg_hover;
    button(id, label)
        .role(Role::RadioButton)
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .flex_none()
        .min_h(px(32.))
        .gap(px(8.))
        .rounded(px(radius()))
        .px(px(8.))
        .when(checked, |row| row.bg(t.bg_selected_muted))
        .hover(move |s| s.bg(hover))
        .child(
            div()
                .size(px(12.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(if checked { t.accent } else { t.border_strong })
                .when(checked, |dot| {
                    dot.child(div().size(px(6.)).rounded_full().bg(t.accent))
                }),
        )
}

/// What a device is doing, beside a spinner while it changes.
fn status(
    id: ElementId,
    text: impl Into<SharedString>,
    turning: bool,
    t: &Theme,
) -> Stateful<gpui::Div> {
    let text = text.into();
    div()
        .id(id)
        .role(Role::Status)
        .aria_label(text.clone())
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.))
        .text_size(px(FS_SM))
        .text_color(t.text_muted)
        .when(turning, |d| d.child(spinner(12., t.text_muted)))
        .child(text)
}

impl AndroidPanel {
    /// The button that builds the app and runs it on the device `key`, starting the device first
    /// when it is stopped; a spinner on the device it is being run on.
    fn run_button(
        &self,
        id: (&'static str, usize),
        key: &str,
        name: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = theme(cx);
        if self.run != Run::Idle && self.selected.as_deref() == Some(key) {
            return div()
                .id(id)
                .role(Role::Status)
                .aria_label(format!("Running the app on {name}"))
                .size(px(28.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .child(spinner(14., t.text_muted))
                .into_any_element();
        }
        let key = key.to_string();
        icon_button(id, format!("Run the app on {name}"), "play", 14., cx)
            .tooltip(tooltip("Run the app"))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.select(key.clone(), cx);
                this.run(cx);
            }))
            .into_any_element()
    }

    /// Opens the menu of a stopped virtual device: starting it without running the app, and the
    /// other ways to start it.
    fn open_avd_menu(
        &mut self,
        avd: &str,
        e: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let item =
            |label: &'static str, name: String, act: fn(&mut Self, &str, &mut Context<Self>)| {
                let this = this.clone();
                MenuItem::new(label, move |_, cx| {
                    let _ = this.update(cx, |this, cx| act(this, &name, cx));
                })
            };
        let items = vec![
            item("Start", avd.to_string(), |this, name, cx| {
                this.start(name, Boot::Quick, false, cx);
            })
            .icon("play"),
            item("Cold boot", avd.to_string(), |this, name, cx| {
                this.start(name, Boot::Cold, false, cx);
            })
            .icon("arrow-clockwise"),
            item("Wipe data…", avd.to_string(), |this, name, cx| {
                this.confirm = Some((name.to_string(), Confirm::Wipe));
                cx.notify();
            })
            .icon("arrow-counter-clockwise")
            .danger(),
            item("Delete…", avd.to_string(), |this, name, cx| {
                this.confirm = Some((name.to_string(), Confirm::Delete));
                cx.notify();
            })
            .icon("trash")
            .danger()
            .separated(),
        ];
        let menu = Menu::open_for(
            format!("{avd} actions"),
            items,
            window,
            cx,
            |this, window, cx| {
                this.menu = None;
                window.focus(&this.focus, cx);
                cx.notify();
            },
        );
        self.menu = Some((avd.to_string(), menu, e.position()));
        cx.notify();
    }

    /// The button that opens the menu of what the running `device` can be asked.
    fn more_button(
        &self,
        id: (&'static str, usize),
        device: &Device,
        name: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.menu_open(&device_menu(&device.serial));
        let device = device.clone();
        icon_button(id, format!("More for {name}"), "dots-three", 16., cx)
            .aria_expanded(open)
            .tooltip(tooltip("More"))
            .on_click(cx.listener(move |this, e, window, cx| {
                cx.stop_propagation();
                this.open_device_menu(&device, e, window, cx);
            }))
            .into_any_element()
    }

    pub(super) fn avd_row(
        &self,
        i: usize,
        avd: &Avd,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = avd.name.clone();
        let device = self.device_of(&avd.name);
        let busy = self.busy.get(&avd.name).copied();
        let checked = self.selected.as_ref() == Some(&avd.name);
        let version = match &avd.android_version {
            Some(version) => format!("API {} · Android {version}", avd.api),
            None => format!("API {}", avd.api),
        };
        let facts = format!("{version} · {} · {}", avd.abi, avd.variant);
        let state = match (busy, device) {
            (Some(Busy::Starting), _) => "Starting",
            (Some(Busy::Stopping), _) => "Stopping",
            (Some(Busy::Deleting), _) => "Deleting",
            (None, Some(d)) if d.state != DeviceState::Online => "Starting",
            (None, Some(_)) => "Running",
            (None, None) => "Stopped",
        };
        let turning = busy.is_some() || state == "Starting";
        let label = format!("{}, {facts}, {state}", avd.display_name);

        let mut controls: Vec<AnyElement> = Vec::new();
        match (busy, device) {
            (None, Some(d)) => {
                let (name, serial) = (name.clone(), d.serial.clone());
                if d.state == DeviceState::Online {
                    controls.push(self.run_button(("avd-run", i), &name, &avd.display_name, cx));
                }
                if d.state == DeviceState::Online {
                    controls.push(self.more_button(("avd-tools", i), d, &avd.display_name, cx));
                }
                controls.push(
                    icon_button(
                        ("avd-stop", i),
                        format!("Stop {}", avd.display_name),
                        "stop",
                        14.,
                        cx,
                    )
                    .tooltip(tooltip("Stop"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.stop(&name, serial.clone(), cx);
                    }))
                    .into_any_element(),
                );
            }
            (None, None) => {
                let more = name.clone();
                let open = self.menu.as_ref().is_some_and(|(of, _, _)| of == &avd.name);
                controls.push(self.run_button(("avd-run", i), &name, &avd.display_name, cx));
                controls.push(
                    icon_button(
                        ("avd-more", i),
                        format!("More for {}", avd.display_name),
                        "dots-three",
                        16.,
                        cx,
                    )
                    .aria_expanded(open)
                    .tooltip(tooltip("More"))
                    .on_click(cx.listener(move |this, e, window, cx| {
                        cx.stop_propagation();
                        this.open_avd_menu(&more, e, window, cx);
                    }))
                    .into_any_element(),
                );
            }
            // Nothing while it starts, stops or is deleted.
            (Some(_), _) => {}
        }
        let row = radio_row(("avd", i), label, checked, t)
            .child(icon("device-mobile", 16.).text_color(t.text_muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .py(px(4.))
                    .child(div().truncate().child(avd.display_name.clone()))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(FS_SM))
                            .text_color(t.text_muted)
                            .child(facts),
                    ),
            )
            .child(status(("avd-status", i).into(), state, turning, t))
            .children(controls)
            .on_click(cx.listener({
                let name = name.clone();
                move |this, _, _, cx| this.select(name.clone(), cx)
            }));
        let Some(asked) = self
            .confirm
            .as_ref()
            .filter(|(of, _)| of == &avd.name)
            .map(|(_, asked)| *asked)
        else {
            return row.into_any_element();
        };
        let (question, action) = match asked {
            Confirm::Wipe => ("Erase its apps and data, then start it?", "Wipe data"),
            Confirm::Delete => ("Delete this device, with its apps and data?", "Delete"),
        };
        let wipe = name.clone();
        div()
            .flex()
            .flex_col()
            .child(row)
            .child(
                div()
                    .id(("avd-wipe", i))
                    .role(Role::Group)
                    .aria_label(format!("{action}: {}", avd.display_name))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(28.))
                    .pr(px(8.))
                    .pb(px(6.))
                    .text_size(px(FS_SM))
                    .child(div().flex_1().min_w_0().child(question))
                    .child(
                        btn(("avd-wipe-cancel", i), "Cancel", BtnKind::Plain, false, t)
                            .child("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.confirm = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        btn(("avd-wipe-confirm", i), action, BtnKind::Plain, false, t)
                            .text_color(t.danger)
                            .child(action)
                            .on_click(cx.listener(move |this, _, _, cx| match asked {
                                Confirm::Wipe => this.start(&wipe, Boot::Wipe, false, cx),
                                Confirm::Delete => this.delete(&wipe, cx),
                            })),
                    ),
            )
            .into_any_element()
    }

    /// A phone or tablet, or an emulator whose virtual device is not one of the user's own.
    pub(super) fn device_row(
        &self,
        i: usize,
        device: &Device,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = device_key(device);
        let checked = self.selected.as_ref() == Some(&key);
        let name = device
            .avd
            .clone()
            .or_else(|| device.model.clone())
            .unwrap_or_else(|| device.serial.clone());
        let state = match &device.state {
            DeviceState::Online => "Connected".to_string(),
            DeviceState::Offline => "Offline".to_string(),
            DeviceState::Unauthorized => "Unauthorized".to_string(),
            DeviceState::NoPermissions => "No permission".to_string(),
            DeviceState::Other(state) => state.clone(),
        };
        let stopping = self.busy.get(&key) == Some(&Busy::Stopping);
        let state = if stopping {
            "Stopping".to_string()
        } else {
            state
        };
        let label = format!("{name}, {}, {state}", device.serial);
        let problem = android::problem(device)
            .filter(|_| !stopping)
            .map(|e| e.to_string());
        let run = (device.state == DeviceState::Online && problem.is_none() && !stopping)
            .then(|| self.run_button(("device-run", i), &key, &name, cx));
        let more = (device.state == DeviceState::Online && !stopping)
            .then(|| self.more_button(("device-tools", i), device, &name, cx));
        // An emulator can be asked to close; a phone is unplugged.
        let stop = (device.emulator && !stopping).then(|| {
            let (key, serial) = (key.clone(), device.serial.clone());
            icon_button(("device-stop", i), format!("Stop {name}"), "stop", 14., cx)
                .tooltip(tooltip("Stop"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.stop(&key, serial.clone(), cx);
                }))
        });
        let glyph = if device.emulator {
            "device-mobile"
        } else {
            "plug"
        };
        let row = radio_row(("device", i), label, checked, t)
            .child(icon(glyph, 16.).text_color(t.text_muted))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .py(px(4.))
                    .child(div().truncate().child(name))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(FS_SM))
                            .text_color(t.text_muted)
                            .child(device.serial.clone()),
                    ),
            )
            .child(
                status(("device-status", i).into(), state, stopping, t)
                    .when(problem.is_some(), |s| s.text_color(t.warning)),
            )
            .children(run)
            .children(more)
            .children(stop)
            .on_click(cx.listener(move |this, _, _, cx| this.select(key.clone(), cx)));
        match problem {
            None => row.into_any_element(),
            Some(problem) => div()
                .flex()
                .flex_col()
                .child(row)
                .child(
                    div()
                        .id(("device-problem", i))
                        .role(Role::Alert)
                        .pl(px(28.))
                        .pr(px(8.))
                        .pb(px(6.))
                        .text_size(px(FS_SM))
                        .text_color(t.text_muted)
                        .child(problem),
                )
                .into_any_element(),
        }
    }
}
