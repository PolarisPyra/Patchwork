//! The chip a dropped file, folder or picture is shown as, in a message and before it is sent.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use gpui::{Div, ObjectFit, SharedString, Stateful, div, img, prelude::*, px};
use patchwork_model::chats::{Attachment, AttachmentKind};
use patchwork_design_components::tooltip;
use patchwork_design_icons::{IconName, icon};
use patchwork_design_theme::{Theme, fs_sm, fs_xs, radius};

fn kind_name(kind: AttachmentKind) -> IconName {
    match kind {
        AttachmentKind::Image => IconName::Image,
        AttachmentKind::File => IconName::File,
        AttachmentKind::Folder => IconName::Folder,
    }
}

/// Whether the file at `path` is a picture the app can draw, though agents may not take it as one,
/// such as an SVG, so its chip still shows a thumbnail.
fn drawable(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, ext)| {
        let ext = ext.to_ascii_lowercase();
        gpui::Img::extensions().contains(&ext.as_str())
    })
}

/// A dropped file, folder, or image, shown as a badge with its full path on hover. A picture
/// shows its thumbnail, or its icon if it cannot be read.
pub fn attachment_chip(id: impl Into<SharedString>, a: &Attachment, t: &Theme) -> Stateful<Div> {
    let name = kind_name(a.kind);
    let muted = t.text_muted;
    let lead = if a.kind == AttachmentKind::Image || drawable(&a.path) {
        img(PathBuf::from(&a.path))
            .size(px(20.))
            .flex_none()
            .rounded(px(2.))
            .object_fit(ObjectFit::Cover)
            .with_fallback(move || {
                icon(IconName::Image, 13.)
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
                .child(name.kebab_name().to_uppercase()),
        )
}
