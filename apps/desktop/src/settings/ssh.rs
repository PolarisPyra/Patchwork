//! Settings → SSH (`system/SshPane.tsx`): the hosts in `~/.ssh/config`, which the Add Project
//! dialog offers as machines. Test logs in with keys only, so it shows whether Patchwork could
//! reach a host unattended. The config is the user's own: it is opened in their editor, and
//! Patchwork never writes it.

use std::collections::{HashMap, HashSet};

use gpui::{AnyElement, Context, FontWeight, SharedString, div, prelude::*, px};
use patchwork_server::system::ssh::{self, SshHost, SshTest};

use crate::settings::SettingsDialog;
use crate::settings::controls::alert;
use crate::ui::theme::{FONT_MONO, FS_SM, FS_XS, RADIUS_SM, theme};
use crate::ui::{BtnKind, btn, icon, spinner};

#[derive(Default)]
pub struct SshState {
    hosts: Option<Vec<SshHost>>,
    asked: bool,
    /// The last result of each host's test, by name.
    tests: HashMap<String, SshTest>,
    testing: HashSet<String>,
    error: Option<String>,
}

/// `user@host:port`, leaving out what the config does not set.
fn address(h: &SshHost) -> String {
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
                let result = out.unwrap_or_else(|e| SshTest {
                    ok: false,
                    latency_ms: None,
                    error: Some(e),
                });
                d.ssh.tests.insert(name, result);
            },
            cx,
        );
    }

    fn open_ssh_config(&mut self, cx: &mut Context<Self>) {
        self.ssh.error = None;
        self.run(
            || {
                let path = ssh::config_path().ok_or("No home folder")?;
                if !path.is_file() {
                    return Err(format!("{} does not exist", path.display()));
                }
                let dir = path
                    .parent()
                    .map(|d| d.to_string_lossy().into_owned())
                    .unwrap_or_default();
                patchwork_server::workspace::open_in_editor(&dir, "config", None)
                    .map_err(|e| e.to_string())
            },
            |d, out, _| d.ssh.error = out.err(),
            cx,
        );
    }
}

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
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
                                    d.font_family(FONT_MONO).text_color(t.text_muted)
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
                    .rounded(px(RADIUS_SM))
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
    let none = state.hosts.as_ref().is_some_and(|h| h.is_empty());
    let error = state.error.clone();

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
        .child(
            div().py(px(10.)).flex().child(
                btn(
                    "ssh-open-config",
                    "Open ~/.ssh/config",
                    BtnKind::Plain,
                    false,
                    &t,
                )
                .child("Open ~/.ssh/config")
                .on_click(cx.listener(|d, _, _, cx| d.open_ssh_config(cx))),
            ),
        )
        .children(error.map(|e| alert("ssh-error", e, t.danger)))
        .into_any_element()
}
