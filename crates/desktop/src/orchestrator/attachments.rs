//! The files, folders and pictures dropped on the pill, which go with the next request, and the
//! row of chips under the pill that shows them.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{AnyElement, Role, SharedString, div, prelude::*, px};

use super::render::{PILL_W, ink};
use super::state::Orchestrator;
use crate::chat::user::attachment_chip;
use crate::ui::theme::theme;
use crate::ui::{button, icon};

impl Orchestrator {
    /// Attaches the files and folders at `paths` to the request being written. They are read
    /// off the UI thread.
    pub(super) fn attach(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let paths: Vec<String> = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        // The server is read once this update is over, as a drop can land inside the
        // workspace's own update.
        let workspace = self.workspace.clone();
        cx.spawn(async move |this, cx| {
            let Ok(server) = workspace.read_with(cx, |w, _| Arc::clone(&w.server)) else {
                return;
            };
            let loaded = cx
                .background_spawn(async move { server.attach_paths(&paths) })
                .await;
            let _ = this.update(cx, |this, cx| {
                for attachment in loaded {
                    if !this.attachments.iter().any(|a| a.path == attachment.path) {
                        this.attachments.push(attachment);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The chips for what is attached, each with a button that takes it off.
    pub(super) fn attachment_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.attachments.is_empty() {
            return None;
        }
        let t = theme(cx);
        let chips = self.attachments.iter().enumerate().map(|(i, a)| {
            attachment_chip(format!("orchestrator-attachment-{i}"), a, t)
                .bg(ink(0.08))
                .border_color(ink(0.14))
                .text_color(ink(0.85))
                .child(
                    button(
                        SharedString::from(format!("orchestrator-remove-{i}")),
                        format!("Remove {}", a.name),
                    )
                    .size(px(16.))
                    .justify_center()
                    .rounded_full()
                    .text_color(ink(0.6))
                    .hover(|s| s.bg(ink(0.12)).text_color(ink(1.)))
                    .child(icon("x", 10.))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if i < this.attachments.len() {
                            this.attachments.remove(i);
                        }
                        // The button pressed is gone; the keys carry on in the field.
                        this.focus(window, cx);
                        cx.notify();
                    })),
                )
        });
        let row = div()
            .id("orchestrator-attachments")
            .role(Role::List)
            .aria_label("Attachments")
            .w_full()
            .max_w(px(PILL_W))
            .mt(px(6.))
            .px(px(12.))
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .children(chips);
        Some(row.into_any_element())
    }
}
