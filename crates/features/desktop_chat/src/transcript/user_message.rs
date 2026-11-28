//! A message the person sent and the chips of what was
//! attached to it. A message whose turn can be undone offers Restore, and
//! a very long one, such as a pasted log, shows its first lines until expanded.

// Rust guideline compliant 2026-11-27

use crate::prelude::*;
use patchwork_client::AppState;
use patchwork_views_chat::attachment_chip;
use patchwork_views_logos::platform_logo;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    AnyElement, Div, ObjectFit, PromptLevel, SharedString, Window, div, img, prelude::*, px,
    relative,
};
use patchwork_client::local::core::chats::{Attachment, AttachmentKind, Message};

use patchwork_design_text::{markdown::FindIn, markdown::markdown_found};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{fs_xs, radius, radius_sm, Theme};
use patchwork_design_components::{button, tooltip};
use patchwork_design_text::format_clock;
use patchwork_design_icons::icon;
use patchwork_views_chat::image_markdown::without_image_markdown;
use crate::window::{is_working};

/// User messages longer than this show their first lines until expanded: laying out all of one
/// is slow each time it is drawn.
const LONG_MESSAGE_CHARS: usize = 20_000;
const PREVIEW_LINES: usize = 40;

/// The box a sent image fits in. A smaller image shows at its own size, so it stays sharp.
const TILE_MAX_W: f32 = 260.;
const TILE_MAX_H: f32 = 180.;

/// The long messages expanded, the messages of the open chat whose turn can be undone, and the
/// sizes of its sent images.
#[derive(Default)]
pub struct UserState {
    expanded: HashSet<String>,
    /// The chat and whether it was working when its checkpoints were asked for.
    checked: Option<(String, bool)>,
    checkpoints: HashSet<String>,
    /// Each sent image's width and height by path, once read: `None` while it is read, or if it
    /// cannot be.
    image_sizes: HashMap<String, Option<(u32, u32)>>,
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

/// The window's user message methods, for any [`ChatWindow`].
pub trait UserMessageExt: ChatWindow {
    /// Reads the size of each image sent in the open chat off the main thread, from the file's
    /// header, once per path. A message whose image size arrives is measured again.
    fn load_image_sizes(&mut self, cx: &mut Context<Self>) {
        let ui: &mut crate::ChatUi = self;
        let Some(chat) = ui.chat.as_ref() else {
            return;
        };
        let mut paths = Vec::new();
        for m in &chat.messages {
            for a in &m.attachments {
                if a.kind == AttachmentKind::Image
                    && !ui.transcript.user.image_sizes.contains_key(&a.path)
                {
                    ui.transcript.user.image_sizes.insert(a.path.clone(), None);
                    paths.push(a.path.clone());
                }
            }
        }
        if paths.is_empty() {
            return;
        }
        patchwork_design_tasks::off_thread(
            cx,
            move || {
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
            },
            move |this, sizes, cx| {
                let Some(chat) = this.chat.as_ref() else {
                    return;
                };
                let changed: HashSet<&str> = chat
                    .messages
                    .iter()
                    .filter(|m| {
                        m.attachments
                            .iter()
                            .any(|a| sizes.iter().any(|(path, _)| *path == a.path))
                    })
                    .map(|m| m.id.as_str())
                    .collect();
                for (i, row) in this.transcript.rows.1.iter().enumerate() {
                    if changed.contains(row.id.as_str()) {
                        this.transcript.list.remeasure_items(i..i + 1);
                    }
                }
                this.transcript.user.image_sizes.extend(sizes);
                cx.notify();
            },
        );
    }

    /// Asks off the main thread which messages of the open chat can be restored, again each
    /// time a reply starts or ends.
    fn load_checkpoints(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.as_ref() else {
            return;
        };
        let key = (chat.id.clone(), is_working(chat));
        if self.transcript.user.checked.as_ref() == Some(&key) {
            return;
        }
        let has_project = chat.project_id.is_some();
        self.transcript.user.checked = Some(key.clone());
        self.transcript.user.checkpoints.clear();
        if !has_project {
            return;
        }
        let core = Arc::clone(&self.core);
        let id = key.0.clone();
        patchwork_design_tasks::off_thread(
            cx,
            move || core.checkpoints(&id),
            move |this, ids, cx| {
                if this.transcript.user.checked.as_ref() == Some(&key) {
                    this.transcript.user.checkpoints = ids.into_iter().collect();
                    cx.notify();
                }
            },
        );
    }

    /// Restoring rewrites the project's files, so it asks first. The undone message goes back
    /// into an empty composer, to edit or send again.
    fn restore_checkpoint(
        &mut self,
        message_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((chat_id, text, attachments)) =
            self.chat.as_ref().filter(|t| !is_working(t)).and_then(|t| {
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
                 since, by the agent or by you, are discarded, files created since are deleted, \
                 and this message and the ones after it are removed.",
            ),
            &["Restore", "Cancel"],
            cx,
        );
        let core = Arc::clone(&self.core);
        let message_id = message_id.to_string();
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let restored = cx
                .background_spawn(async move { core.restore_checkpoint(&chat_id, &message_id) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                AppState::set_error(cx, None);
                match restored {
                    Ok(_) => {
                        this.transcript.user.checked = None;
                        this.composer
                            .update(cx, |c, cx| c.fill(&text, attachments, window, cx));
                    }
                    Err(e) => AppState::set_error(cx, Some(e.to_string())),
                }
                this.reload(cx);
            });
        })
        .detach();
    }
}

impl<W: ChatWindow> UserMessageExt for W {}

pub fn user_message<W: ChatWindow>(
    w: &W,
    m: &Message,
    find: Option<&FindIn>,
    t: &Theme,
    cx: &mut Context<W>,
) -> Div {
    let mut header = match m.via {
        Some(platform) => format!(
            "You on {} · {}",
            platform.label(),
            format_clock(m.created_at)
        ),
        None => format!("You · {}", format_clock(m.created_at)),
    };
    // The words already say where it came from, so the logo is left out of the accessibility tree.
    let via = m.via.map(|platform| platform_logo(platform, 12.));
    if m.steer {
        header.push_str(" · Steer");
    }
    let full = without_image_markdown(&m.text);
    let full_len = full.len();
    let long = full_len > LONG_MESSAGE_CHARS;
    let expanded = w.transcript.user.expanded.contains(&m.id);
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
    let working = w.chat.as_ref().is_some_and(is_working);
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
    let restore = (!working && w.transcript.user.checkpoints.contains(&m.id)).then(|| {
        let message_id = m.id.clone();
        action(format!("restore-{}", m.id), "Restore".into())
            .opacity(0.)
            .group_hover(group.clone(), |s| s.opacity(1.))
            .focus(|s| s.opacity(1.))
            .tooltip(tooltip(
                "Put the project's files back as they were before this message",
            ))
            .child(icon(IconName::ArrowUUpLeft, 13.))
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
                    let size = w
                        .transcript
                        .user
                        .image_sizes
                        .get(&a.path)
                        .copied()
                        .flatten();
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
                if !w.transcript.user.expanded.remove(&id) {
                    w.transcript.user.expanded.insert(id.clone());
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
                    .children(via)
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
