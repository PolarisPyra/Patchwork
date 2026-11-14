//! Settings → Devices: the hosts in `~/.ssh/config`, which the Add Project
//! dialog offers as machines. Test logs in with keys only, so it shows whether Patchwork could
//! reach a host unattended. Add Device appends a host to the config, which ssh then reads.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};

use gpui::{AnyElement, Focusable, FontWeight, SharedString, Window, div, prelude::*, px};
use patchwork_server::system::ssh::{self, Host, Test};

use crate::settings::SettingsDialog;
use crate::settings::controls::{FieldKind, alert};
use crate::ui::theme::{font_mono, FS_SM, FS_XS, radius_sm, theme};
use crate::ui::{BtnKind, btn, icon, spinner};

/// The add form's fields share this prefix, so closing it forgets them.
const PREFIX: &str = "ssh-form-";

#[derive(Default)]
pub struct SshState {
    hosts: Option<Vec<Host>>,
    asked: bool,
    /// The last result of each host's test, by name.
    tests: HashMap<String, Test>,
    testing: HashSet<String>,
    error: Option<String>,
    /// The device being added, while its form is open.
    adding: Option<NewDevice>,
}

/// What the add form holds.
#[derive(Default)]
struct NewDevice {
    name: String,
    host: String,
    user: String,
    busy: bool,
    error: Option<String>,
}

/// `user@host:port`, leaving out what the config does not set.
fn address(h: &Host) -> String {
    let host = h.hostname.as_deref().unwrap_or(&h.name);
    let user = h.user.as_ref().map(|u| format!("{u}@")).unwrap_or_default();
    let port = h.port.map(|p| format!(":{p}")).unwrap_or_default();
    format!("{user}{host}{port}")
}

impl SettingsDialog {
    fn test_host(&mut self, name: String, cx: &mut Context<Self>) {
        self.ssh.testing.insert(name.clone());
        let host = name.clone();
        self.run(
            move || ssh::test(&host),
            move |d, out, _| {
                d.ssh.testing.remove(&name);
                let result = out.unwrap_or_else(|e| Test {
                    ok: false,
                    latency_ms: None,
                    error: Some(e.to_string()),
                });
                d.ssh.tests.insert(name, result);
            },
            cx,
        );
    }

    fn open_add_device(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.drop_fields(PREFIX);
        self.ssh.adding = Some(NewDevice::default());
        let _ = render(self, window, cx);
        if let Some(area) = self.field_area(&format!("{PREFIX}name")) {
            window.focus(&area.read(cx).focus_handle(cx), cx);
        }
        cx.notify();
    }

    fn close_add_device(&mut self, cx: &mut Context<Self>) {
        self.ssh.adding = None;
        self.drop_fields(PREFIX);
        cx.notify();
    }

    /// Escape closes the add form, unless it is saving. False when it is not open.
    pub(in crate::settings) fn cancel_add_device(&mut self, cx: &mut Context<Self>) -> bool {
        match &self.ssh.adding {
            None => return false,
            Some(draft) if draft.busy => {}
            Some(_) => self.close_add_device(cx),
        }
        true
    }

    fn save_device(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.ssh.adding.as_mut() else {
            return;
        };
        if draft.busy || draft.host.trim().is_empty() {
            return;
        }
        let (name, host, user) = (draft.name.clone(), draft.host.clone(), draft.user.clone());
        draft.busy = true;
        draft.error = None;
        self.run(
            move || ssh::add_host(&name, &host, &user).map(|()| ssh::hosts()),
            |d, out, cx| match out {
                Ok(hosts) => {
                    d.ssh.hosts = Some(hosts);
                    d.close_add_device(cx);
                }
                Err(e) => {
                    if let Some(draft) = &mut d.ssh.adding {
                        draft.busy = false;
                        draft.error = Some(e.to_string());
                    }
                }
            },
            cx,
        );
    }
}

/// A field with its label above it.
fn labeled(label: &'static str, field: gpui::Div, muted: gpui::Hsla) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(FS_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child(label),
        )
        .child(field)
}

/// The add form, or the button that opens it.
fn add_device(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    let Some(draft) = d.ssh.adding.as_ref() else {
        return div()
            .py(px(10.))
            .flex()
            .child(
                btn("ssh-add-device", "Add device", BtnKind::Plain, false, &t)
                    .gap(px(6.))
                    .child(icon("plus", 14.))
                    .child("Add device")
                    .on_click(cx.listener(|d, _, window, cx| d.open_add_device(window, cx))),
            )
            .into_any_element();
    };
    let (name, host, user, busy, error) = (
        draft.name.clone(),
        draft.host.clone(),
        draft.user.clone(),
        draft.busy,
        draft.error.clone(),
    );
    let kind = FieldKind {
        live: true,
        mono: true,
        ..Default::default()
    };
    let set = |f: fn(&mut NewDevice, String)| {
        move |d: &mut SettingsDialog, text: String, cx: &mut Context<SettingsDialog>| {
            if let Some(draft) = &mut d.ssh.adding {
                f(draft, text);
            }
            cx.notify();
        }
    };
    let name_key = format!("{PREFIX}name");
    let host_key = format!("{PREFIX}host");
    let user_key = format!("{PREFIX}user");
    let name_field = d.field(
        name_key.clone(),
        "Name",
        &name,
        "nas",
        kind,
        set(|draft, text| draft.name = text),
        window,
        cx,
    );
    let host_field = d.field(
        host_key.clone(),
        "Address",
        &host,
        "192.168.1.20",
        kind,
        set(|draft, text| draft.host = text),
        window,
        cx,
    );
    let user_field = d.field(
        user_key.clone(),
        "User",
        &user,
        "",
        kind,
        set(|draft, text| draft.user = text),
        window,
        cx,
    );
    for key in [&name_key, &host_key, &user_key] {
        d.on_submit(key, |d, _, cx| d.save_device(cx));
    }
    let can_save = !busy && !host.trim().is_empty();
    div()
        .id("ssh-add-form")
        .role(gpui::Role::Group)
        .aria_label("Add device")
        .flex()
        .flex_col()
        .gap(px(12.))
        .py(px(10.))
        .child(labeled("Name", name_field, t.text_muted))
        .child(labeled("Address", host_field, t.text_muted))
        .child(labeled("User", user_field, t.text_muted))
        .children(error.map(|e| alert("ssh-form-error", e, t.danger).pt_0()))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .border_t_1()
                .border_color(t.border)
                .pt(px(12.))
                .child(
                    btn("ssh-form-save", "Save", BtnKind::Primary, !can_save, &t)
                        .child(if busy { "Saving…" } else { "Save" })
                        .when(can_save, |b| {
                            b.on_click(cx.listener(|d, _, _, cx| d.save_device(cx)))
                        }),
                )
                .child(
                    btn("ssh-form-cancel", "Cancel", BtnKind::Plain, busy, &t)
                        .child("Cancel")
                        .when(!busy, |b| {
                            b.on_click(cx.listener(|d, _, _, cx| d.close_add_device(cx)))
                        }),
                ),
        )
        .into_any_element()
}

pub fn render(
    d: &mut SettingsDialog,
    window: &mut Window,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let t = *theme(cx);
    if !std::mem::replace(&mut d.ssh.asked, true) {
        d.run(ssh::hosts, |d, hosts, _| d.ssh.hosts = Some(hosts), cx);
    }
    let state = &d.ssh;
    let rows = state.hosts.iter().flatten().map(|h| {
        let result = state.tests.get(&h.name);
        let busy = state.testing.contains(&h.name);
        let failed = result.is_some_and(|r| !r.ok);
        let line = match result {
            Some(r) if r.ok => format!("{} · {} ms", address(h), r.latency_ms.unwrap_or(0)),
            Some(r) => r.error.clone().unwrap_or_else(|| "Failed".into()),
            None => address(h),
        };
        let name = h.name.clone();
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .border_b_1()
            .border_color(t.border)
            .py(px(10.))
            .child(
                div()
                    .flex_none()
                    .text_color(t.text_muted)
                    .child(icon("hard-drives", 20.)),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(h.name.clone()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("ssh-line-{}", h.name)))
                            .when(result.is_some(), |d| d.role(gpui::Role::Status))
                            .truncate()
                            .text_size(px(FS_SM))
                            .map(|d| {
                                if failed {
                                    d.text_color(t.danger)
                                } else {
                                    d.font_family(font_mono()).text_color(t.text_muted)
                                }
                            })
                            .child(line),
                    ),
            )
            .children(result.map(|r| {
                let color = if r.ok { t.success } else { t.danger };
                div()
                    .flex_none()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .px(px(6.))
                    .rounded(px(radius_sm()))
                    .bg(color.opacity(0.14))
                    .text_size(px(FS_XS))
                    .text_color(color)
                    .child(if r.ok { "Connected" } else { "Failed" })
            }))
            .child(
                btn(
                    SharedString::from(format!("ssh-test-{}", h.name)),
                    format!("Test {}", h.name),
                    BtnKind::Plain,
                    busy,
                    &t,
                )
                .gap(px(6.))
                .when(busy, |b| b.child(spinner(13., t.text_muted)))
                .child("Test")
                .when(!busy, |b| {
                    b.on_click(cx.listener(move |d, _, _, cx| d.test_host(name.clone(), cx)))
                }),
            )
    });
    let rows: Vec<_> = rows.collect();
    let loading = state.hosts.is_none() && state.error.is_none();
    let none = state.hosts.as_ref().is_some_and(std::vec::Vec::is_empty);
    let error = state.error.clone();
    let add = add_device(d, window, cx);

    div()
        .flex()
        .flex_col()
        .when(loading, |col| {
            col.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .py(px(10.))
                    .text_color(t.text_muted)
                    .child(spinner(14., t.text_muted))
                    .child("Reading ~/.ssh/config…"),
            )
        })
        .when(none, |col| {
            col.child(
                div()
                    .border_b_1()
                    .border_color(t.border)
                    .py(px(10.))
                    .text_color(t.text_muted)
                    .child("No hosts in ~/.ssh/config"),
            )
        })
        .children(rows)
        .child(add)
        .children(error.map(|e| alert("ssh-error", e, t.danger)))
        .into_any_element()
}
