//! The files, folders and pictures dropped on the pill, which go with the next request, and the
//! pill of chips above it that shows them.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    AnyElement, ClickEvent, Hsla, MouseButton, PathPromptOptions, Role, SharedString, Window,
    prelude::*, px, relative,
};

use super::render::{glass, ink};
use super::state::Orchestrator;
use crate::chat::user::attachment_chip;
use patchwork_ui::menu::{Menu, MenuItem};
use patchwork_ui::pill::pill;
use patchwork_ui::pill::SEND;
use patchwork_ui::theme::theme;
use patchwork_ui::{button, icon};

impl Orchestrator {
    /// The plus button at the start of the pill, which opens a menu to attach files or a folder.
    pub(super) fn attach_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.attach_menu.is_some();
        button("orchestrator-attach", "Attach")
            .size(px(SEND))
            .flex_none()
            .justify_center()
            .rounded_full()
            .aria_expanded(open)
            .text_color(ink(0.7))
            .when(open, |b| b.bg(ink(0.08)))
            .hover(|s| s.bg(ink(0.08)).text_color(ink(1.)))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(|this, e: &ClickEvent, window, cx| {
                this.toggle_attach_menu(e.position(), window, cx);
            }))
            .child(icon("plus", 16.))
            .into_any_element()
    }

    /// The plus button's menu, laid over the page where it opened.
    pub(super) fn attach_menu_view(&self) -> Option<AnyElement> {
        self.attach_menu
            .as_ref()
            .map(|(menu, at)| Menu::view(menu, *at))
    }

    fn toggle_attach_menu(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.attach_menu.take().is_some() {
            self.focus(window, cx);
            return cx.notify();
        }
        let this = cx.entity().downgrade();
        let item = |label: &'static str, name: &'static str, folders: bool| {
            let this = this.clone();
            MenuItem::new(label, move |_, cx| {
                let _ = this.update(cx, |this, cx| this.pick_attachments(folders, cx));
            })
            .icon(name)
        };
        let items = vec![
            item("Files", "paperclip", false),
            item("Folder", "folder", true),
        ];
        let menu = Menu::open_for(
            "Attach",
            items,
            window,
            cx,
            |this: &mut Self, window, cx| {
                this.attach_menu = None;
                this.focus(window, cx);
                cx.notify();
            },
        );
        self.attach_menu = Some((menu, at));
        cx.notify();
    }

    /// Asks for files, or for folders, and attaches what is picked.
    fn pick_attachments(&mut self, folders: bool, cx: &mut Context<Self>) {
        patchwork_ui::pick_paths(
            cx,
            PathPromptOptions {
                files: !folders,
                directories: folders,
                multiple: true,
                prompt: None,
            },
            move |this, paths, cx| this.attach(&paths, cx),
        );
    }

    /// Attaches the files and folders at `paths` to the request being written. They are read
    /// off the UI thread.
    pub(super) fn attach(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let paths: Vec<String> = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        // The core is read once this update is over, as a drop can land inside the
        // workspace's own update.
        let workspace = self.workspace.clone();
        cx.spawn(async move |this, cx| {
            let Ok(core) = workspace.read_with(cx, |w, _| Arc::clone(&w.core)) else {
                return;
            };
            let loaded = cx
                .background_spawn(async move { core.attach_paths(&paths) })
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

    /// The chips for what is attached, each with a button that takes it off, in a pill that sits
    /// above the request pill without moving it.
    pub(super) fn attachment_row(
        &self,
        shadow: Hsla,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.attachments.is_empty() {
            return None;
        }
        let t = theme(cx);
        let chips = self.attachments.iter().enumerate().map(|(i, a)| {
            // The pill around them is their frame, so a chip has none of its own.
            attachment_chip(format!("orchestrator-attachment-{i}"), a, t)
                .bg(gpui::transparent_black())
                .border_color(gpui::transparent_black())
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
        let row = pill("orchestrator-attachments", ink(0.10), glass(0.62), shadow)
            .role(Role::List)
            .aria_label("Attachments")
            .absolute()
            .left_0()
            .bottom(relative(1.))
            .max_w_full()
            .mb(px(8.))
            .p(px(6.))
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .children(chips);
        Some(row.into_any_element())
    }
}
