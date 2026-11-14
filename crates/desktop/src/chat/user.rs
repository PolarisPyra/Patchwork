//! A message the person sent and the chips of what was
//! attached to it. A message whose turn can be undone offers Restore, and
//! a very long one, such as a pasted log, shows its first lines until expanded.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    AnyElement, Div, ObjectFit, PromptLevel, SharedString, Stateful, Window, div, img, prelude::*,
    px, relative,
};
use patchwork_server::threads::{Attachment, AttachmentKind, Message};

use crate::ui::markdown::{FindIn, markdown_found};
use crate::ui::theme::{fs_sm, fs_xs, radius, radius_sm, Theme};
use crate::ui::{button, format_clock, icon, tooltip};
use crate::workspace::{Workspace, is_working};

/// User messages longer than this show their first lines until expanded: laying out all of one
/// is slow each time it is drawn.
const LONG_MESSAGE_CHARS: usize = 20_000;
const PREVIEW_LINES: usize = 40;

/// The box a sent image fits in. A smaller image shows at its own size, so it stays sharp.
const TILE_MAX_W: f32 = 260.;
const TILE_MAX_H: f32 = 180.;

/// The long messages expanded, the messages of the open thread whose turn can be undone, and the
/// sizes of its sent images.
#[derive(Default)]
pub struct UserState {
    expanded: HashSet<String>,
    /// The thread and whether it was working when its checkpoints were asked for.
    checked: Option<(String, bool)>,
    checkpoints: HashSet<String>,
    /// Each sent image's width and height by path, once read: `None` while it is read, or if it
    /// cannot be.
    image_sizes: HashMap<String, Option<(u32, u32)>>,
}

/// The text with the images written into it as `![name](</path>)` taken out; they show with the
/// other attachments.
pub fn without_image_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("![") {
        out.push_str(&rest[..start]);
        if let Some(end) = image_end(&rest[start..]) {
            rest = &rest[start + end..];
            // The space on one side of the image goes with it. Other spacing is the user's:
            // indented code, aligned tables.
            if out.ends_with(' ') || out.is_empty() {
                rest = rest.strip_prefix(' ').unwrap_or(rest);
            }
        } else {
            out.push_str("![");
            rest = &rest[start + 2..];
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// The length of the `![label](</absolute/path>)` that `text` starts with.
fn image_end(text: &str) -> Option<usize> {
    let mut chars = text.char_indices().skip(2);
    let mut close = None;
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => _ = chars.next(),
            ']' => {
                close = Some(i);
                break;
            }
            _ => {}
        }
    }
    let path = text[close? + 1..].strip_prefix("(</")?;
    let end = path.find(['<', '>', '\n'])?;
    path[end..]
        .starts_with(">)")
        .then(|| text.len() - path.len() + end + 2)
}

fn kind_name(kind: AttachmentKind) -> &'static str {
    match kind {
        AttachmentKind::Image => "image",
        AttachmentKind::File => "file",
        AttachmentKind::Folder => "folder",
    }
}

/// A dropped file, folder, or image, shown as a badge with its full path on hover. An image shows
/// its thumbnail, or its icon if it cannot be read.
pub fn attachment_chip(id: impl Into<SharedString>, a: &Attachment, t: &Theme) -> Stateful<Div> {
    let name = kind_name(a.kind);
    let muted = t.text_muted;
    let lead = if a.kind == AttachmentKind::Image {
        img(PathBuf::from(&a.path))
            .size(px(20.))
            .flex_none()
            .rounded(px(2.))
            .object_fit(ObjectFit::Cover)
            .with_fallback(move || {
                icon("image", 13.)
                    .flex_none()
                    .text_color(muted)
                    .into_any_element()
            })
            .into_any_element()
    } else {
        icon(name, 13.)
            .flex_none()
            .text_color(t.text_muted)
            .into_any_element()
    };
    div()
        .id(id.into())
        .flex()
        .flex_none()
        .max_w(px(280.))
        .items_center()
        .gap(px(6.))
        .rounded(px(radius()))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_panel)
        .py(px(2.))
        .pr(px(4.))
        .pl(px(8.))
        .text_size(px(fs_sm()))
        .text_color(t.text)
        .tooltip(tooltip(a.path.clone()))
        .child(lead)
        .child(div().min_w_0().truncate().child(a.name.clone()))
        .child(
            div()
                .flex_none()
                .text_size(px(fs_xs()))
                .text_color(t.text_subtle)
                .child(name.to_uppercase()),
        )
}

/// A sent image shown large, as a thumbnail; its chip if it cannot be read. With its `size` read,
/// it shrinks to fit the tile's box and is never enlarged; until then it is the box's height.
fn attachment_tile(id: String, a: &Attachment, size: Option<(u32, u32)>, t: &Theme) -> AnyElement {
    let chip = attachment_chip(id.clone(), a, t);
    let fallback = std::cell::RefCell::new(Some(chip));
    // Both sides are set: with only `max_h`, layout works the height out again from the clamped
    // width and draws a tall image past the space measured for it.
    let image = match size {
        Some((width, height)) if width > 0 && height > 0 => {
            let (width, height) = (width as f32, height as f32);
            let scale = (TILE_MAX_W / width).min(TILE_MAX_H / height).min(1.);
            img(PathBuf::from(&a.path))
                .w(px((width * scale).round()))
                .h(px((height * scale).round()))
        }
        _ => img(PathBuf::from(&a.path))
            .h(px(TILE_MAX_H))
            .max_w(px(TILE_MAX_W)),
    };
    div()
        .id(SharedString::from(id))
        .tooltip(tooltip(a.path.clone()))
        .child(
            image
                .rounded(px(radius()))
                .border_1()
                .border_color(t.border)
                .object_fit(ObjectFit::Cover)
                .with_fallback(move || {
                    fallback
                        .borrow_mut()
                        .take()
                        .map_or_else(|| div().into_any_element(), IntoElement::into_any_element)
                }),
        )
        .into_any_element()
}

impl Workspace {
    /// Reads the size of each image sent in the open thread off the main thread, from the file's
    /// header, once per path. A message whose image size arrives is measured again.
    pub fn load_image_sizes(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = self.thread.as_ref() else {
            return;
        };
        let mut paths = Vec::new();
        for m in &thread.messages {
            for a in &m.attachments {
                if a.kind == AttachmentKind::Image && !self.user.image_sizes.contains_key(&a.path) {
                    self.user.image_sizes.insert(a.path.clone(), None);
                    paths.push(a.path.clone());
                }
            }
        }
        if paths.is_empty() {
            return;
        }
        let task = cx.background_spawn(async move {
            paths
                .into_iter()
                .map(|path| {
                    let size = image::image_dimensions(&path)
                        .inspect_err(|e| {
                            log::warn!(
                                error:% = e;
                                "could not read the size of a sent image: {{error}}"
                            );
                        })
                        .ok();
                    (path, size)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let sizes = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(thread) = this.thread.as_ref() else {
                    return;
                };
                let changed: HashSet<&str> = thread
                    .messages
                    .iter()
                    .filter(|m| {
                        m.attachments
                            .iter()
                            .any(|a| sizes.iter().any(|(path, _)| *path == a.path))
                    })
                    .map(|m| m.id.as_str())
                    .collect();
                for (i, row) in this.transcript_rows.1.iter().enumerate() {
                    if changed.contains(row.id.as_str()) {
                        this.transcript_list.remeasure_items(i..i + 1);
                    }
                }
                this.user.image_sizes.extend(sizes);
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks off the main thread which messages of the open thread can be restored, again each
    /// time a reply starts or ends.
    pub fn load_checkpoints(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = self.thread.as_ref() else {
            return;
        };
        let key = (thread.id.clone(), is_working(thread));
        if self.user.checked.as_ref() == Some(&key) {
            return;
        }
        let has_project = thread.project_id.is_some();
        self.user.checked = Some(key.clone());
        self.user.checkpoints.clear();
        if !has_project {
            return;
        }
        let server = Arc::clone(&self.server);
        let id = key.0.clone();
        let task = cx.background_spawn(async move { server.checkpoints(&id) });
        cx.spawn(async move |this, cx| {
            let ids = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.user.checked.as_ref() == Some(&key) {
                    this.user.checkpoints = ids.into_iter().collect();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Restoring rewrites the project's files, so it asks first. The undone message goes back
    /// into an empty composer, to edit or send again.
    fn restore_checkpoint(
        &mut self,
        message_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((thread_id, text, attachments)) = self
            .thread
            .as_ref()
            .filter(|t| !is_working(t))
            .and_then(|t| {
                let m = t.messages.iter().find(|m| m.id == message_id)?;
                Some((t.id.clone(), m.text.clone(), m.attachments.clone()))
            })
        else {
            return;
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            "Restore checkpoint",
            Some(
                "Put the project's files back as they were before this message? Changes made \
                 since, by the agent or by you, are discarded, and this message and the ones \
                 after it are removed.",
            ),
            &["Restore", "Cancel"],
            cx,
        );
        let server = Arc::clone(&self.server);
        let message_id = message_id.to_string();
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let restored = cx
                .background_spawn(async move { server.restore_checkpoint(&thread_id, &message_id) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.error = None;
                match restored {
                    Ok(_) => {
                        this.user.checked = None;
                        this.composer
                            .update(cx, |c, cx| c.fill(&text, attachments, window, cx));
                    }
                    Err(e) => this.error = Some(e.to_string()),
                }
                this.reload(cx);
            });
        })
        .detach();
    }
}

pub fn user_message(
    w: &Workspace,
    m: &Message,
    find: Option<&FindIn>,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Div {
    let mut header = format!("You · {}", format_clock(m.created_at));
    if m.steer {
        header.push_str(" · Steer");
    }
    let full = without_image_markdown(&m.text);
    let full_len = full.len();
    let long = full_len > LONG_MESSAGE_CHARS;
    let expanded = w.user.expanded.contains(&m.id);
    let text = if long && !expanded {
        let lines: Vec<&str> = full.split('\n').collect();
        if lines.len() > PREVIEW_LINES {
            lines[..PREVIEW_LINES].join("\n")
        } else {
            let mut end = LONG_MESSAGE_CHARS;
            while !full.is_char_boundary(end) {
                end -= 1;
            }
            full[..end].to_string()
        }
    } else {
        full
    };
    let working = w.thread.as_ref().is_some_and(is_working);
    let (hover, fg) = (t.bg_hover, t.text);
    let action = |id: String, label: SharedString| {
        button(SharedString::from(id), label)
            .h(px(24.))
            .gap(px(4.))
            .rounded(px(radius_sm()))
            .px(px(6.))
            .text_size(px(fs_xs()))
            .text_color(t.text_muted)
            .hover(move |s| s.bg(hover).text_color(fg))
    };
    let group = SharedString::from(format!("user-{}", m.id));
    let restore = (!working && w.user.checkpoints.contains(&m.id)).then(|| {
        let message_id = m.id.clone();
        action(format!("restore-{}", m.id), "Restore".into())
            .opacity(0.)
            .group_hover(group.clone(), |s| s.opacity(1.))
            .focus(|s| s.opacity(1.))
            .tooltip(tooltip(
                "Put the project's files back as they were before this message",
            ))
            .child(icon("arrow-counter-clockwise", 13.))
            .child("Restore")
            .on_click(
                cx.listener(move |w, _, window, cx| w.restore_checkpoint(&message_id, window, cx)),
            )
    });
    let attachments = (!m.attachments.is_empty()).then(|| {
        div().flex().flex_wrap().justify_end().gap(px(6.)).children(
            m.attachments.iter().enumerate().map(|(i, a)| {
                let id = format!("attachment-{}-{i}", m.id);
                if a.kind == AttachmentKind::Image {
                    let size = w.user.image_sizes.get(&a.path).copied().flatten();
                    attachment_tile(id, a, size, t)
                } else {
                    attachment_chip(id, a, t).into_any_element()
                }
            }),
        )
    });
    let toggle = long.then(|| {
        let id = m.id.clone();
        let label = if expanded {
            "Show less".to_string()
        } else {
            format!(
                "Show full message ({}k characters)",
                (full_len as f32 / 1000.).round()
            )
        };
        action(format!("expand-{}", m.id), label.clone().into())
            .aria_expanded(expanded)
            .child(label)
            .on_click(cx.listener(move |w, _, _, cx| {
                if !w.user.expanded.remove(&id) {
                    w.user.expanded.insert(id.clone());
                }
                cx.notify();
            }))
    });
    div().flex().flex_col().items_end().child(
        div()
            .group(group)
            .max_w(relative(0.75))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(8.))
                    // As tall as Restore, so the message keeps its height when it comes or goes.
                    .min_h(px(24.))
                    .text_size(px(fs_xs()))
                    .text_color(t.text_subtle)
                    .children(restore)
                    .child(header),
            )
            .children(attachments)
            .when(full_len > 0, |d| {
                d.child(
                    div()
                        .rounded(px(16.))
                        .bg(t.bubble)
                        .px(px(16.))
                        .py(px(10.))
                        .line_height(relative(1.55))
                        .child(markdown_found(&text, format!("u-{}", m.id), true, find, cx)),
                )
            })
            .children(toggle),
    )
}
