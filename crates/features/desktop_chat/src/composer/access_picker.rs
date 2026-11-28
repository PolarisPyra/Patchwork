//! The permissions menu: one mode per row, the active one checked, in the app's context-menu
//! look.

// Rust guideline compliant 2026-10-09

use gpui::Context;
use patchwork_design_components::menu::MenuItem;
use patchwork_design_icons::IconName;
use patchwork_model::chats::AccessMode;

use super::Composer;
use super::menus::Pick;

pub(super) const ACCESS_MODES: [(AccessMode, &str, IconName); 4] = [
    (AccessMode::Supervised, "Supervised", IconName::LockSimple),
    (
        AccessMode::AutoAcceptEdits,
        "Auto-accept edits",
        IconName::NotePencil,
    ),
    (AccessMode::Auto, "Auto", IconName::Robot),
    (AccessMode::FullAccess, "Full access", IconName::LockOpen),
];

#[must_use]
pub fn access_of(mode: AccessMode) -> (&'static str, IconName) {
    ACCESS_MODES
        .iter()
        .find(|(m, ..)| *m == mode)
        .map_or(("Full access", IconName::LockOpen), |(_, label, icon)| {
            (*label, *icon)
        })
}

impl Composer {
    /// The permissions menu's rows, with `current` checked.
    pub(super) fn access_items(&self, current: AccessMode, cx: &Context<Self>) -> Vec<MenuItem> {
        ACCESS_MODES
            .iter()
            .map(|&(mode, label, glyph)| {
                let composer = cx.entity().downgrade();
                MenuItem::new(label, move |_, cx| {
                    let _ = composer.update(cx, |c, cx| {
                        c.commit(Pick::Access(mode), cx);
                        cx.notify();
                    });
                })
                .icon(glyph)
                .radio(current == mode)
            })
            .collect()
    }
}
