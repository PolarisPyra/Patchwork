//! `DevicesView`: the machines projects can live on, this computer and every SSH host in
//! `~/.ssh/config`, each with the icon of what it is. Picking one opens Add Project on it.

// Rust guideline compliant 2026-10-09

use gpui::{EventEmitter, FontWeight, Role, SharedString, Window, div, prelude::*, px};
use patchwork_server::system::browse::{self, Machine};
use patchwork_server::system::device::{self, Kind};

use super::{FS_SM, FS_XS, empty_row};
use crate::ui::theme::{RADIUS_SM, theme};
use crate::ui::{button, icon, icon_button, spinner};

/// A device was picked: the SSH host's alias, or none for this computer.
pub struct OpenDevice(pub Option<String>);

pub struct DevicesView {
    /// The machines, once read, each with its kind once it answered.
    devices: Option<Vec<(Machine, Option<Kind>)>>,
    /// Counts the loads started, so an older load's answers are dropped.
    run: usize,
}

impl EventEmitter<OpenDevice> for DevicesView {}

impl DevicesView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            devices: None,
            run: 0,
        };
        this.load(cx);
        this
    }

    /// The machines, once read, each with its kind once it answered.
    pub fn devices(&self) -> Option<&[(Machine, Option<Kind>)]> {
        self.devices.as_deref()
    }

    /// Reads the machines, then asks each what it is, off the UI thread. A host that is slow to
    /// answer holds up only its own icon.
    fn load(&mut self, cx: &mut Context<Self>) {
        self.run += 1;
        let run = self.run;
        self.devices = None;
        let task = cx.background_spawn(async move { browse::machines() });
        cx.spawn(async move |this, cx| {
            let machines = task.await;
            this.update(cx, |this, cx| {
                if this.run != run {
                    return;
                }
                for (i, machine) in machines.iter().enumerate() {
                    let machine = machine.clone();
                    let task = cx.background_spawn(async move { device::kind(&machine) });
                    cx.spawn(async move |this, cx| {
                        let kind = task.await;
                        this.update(cx, |this, cx| {
                            if this.run != run {
                                return;
                            }
                            if let Some((_, slot)) =
                                this.devices.as_mut().and_then(|d| d.get_mut(i))
                            {
                                *slot = Some(kind);
                                cx.notify();
                            }
                        })
                    })
                    .detach();
                }
                this.devices = Some(machines.into_iter().map(|m| (m, None)).collect());
                cx.notify();
            })
        })
        .detach();
        cx.notify();
    }
}

/// The icon for a kind of machine.
pub(crate) fn kind_icon(kind: Kind) -> &'static str {
    match kind {
        Kind::Desktop => "desktop",
        Kind::Laptop => "laptop",
        Kind::Other => "hard-drives",
    }
}

impl Render for DevicesView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let (fg, hover) = (t.text, t.tree_hover);
        let heading = div()
            .h(px(32.))
            .mt(px(6.))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .pl(px(12.))
            .pr(px(8.))
            .text_size(px(FS_SM))
            .font_weight(FontWeight::SEMIBOLD)
            .child("Devices")
            .child(
                icon_button("refresh-devices", "Refresh", "arrows-clockwise", 18., cx)
                    .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
            );
        let rows = self
            .devices
            .iter()
            .flatten()
            .enumerate()
            .map(|(i, (m, kind))| {
                let name = m.name.clone();
                let host = (!m.local).then(|| m.name.clone());
                let mark = match kind {
                    Some(kind) => icon(kind_icon(*kind), 16.).into_any_element(),
                    None => spinner(14., t.text_muted).into_any_element(),
                };
                button(SharedString::from(format!("device-{i}")), name.clone())
                    .role(Role::ListItem)
                    .h(px(28.))
                    .gap(px(8.))
                    .mx(px(4.))
                    .px(px(8.))
                    .rounded(px(RADIUS_SM))
                    .text_size(px(FS_SM))
                    .text_color(fg)
                    .hover(move |s| s.bg(hover))
                    .child(
                        div()
                            .w(px(16.))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .text_color(t.text_muted)
                            .child(mark),
                    )
                    .child(div().min_w_0().flex_1().truncate().child(name))
                    .when(m.local, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(px(FS_XS))
                                .text_color(t.text_subtle)
                                .child("This computer"),
                        )
                    })
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(OpenDevice(host.clone()))))
            });
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(heading)
            .child(
                div()
                    .id("devices")
                    .role(Role::List)
                    .aria_label("Devices")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .when(self.devices.is_none(), |d| {
                        d.child(
                            div()
                                .id("devices-loading")
                                .px(px(12.))
                                .role(Role::Status)
                                .aria_label("Loading devices")
                                .child(spinner(14., t.text_muted)),
                        )
                    })
                    .children(rows)
                    .when(self.devices.as_ref().is_some_and(|d| d.len() == 1), |d| {
                        d.child(
                            div()
                                .px(px(6.))
                                .child(empty_row("No SSH hosts in ~/.ssh/config", &t)),
                        )
                    }),
            )
    }
}
