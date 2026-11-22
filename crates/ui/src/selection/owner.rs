//! Whose selection is the latest, of the chat's, the text fields' and the terminal's, and the
//! primary selection a middle click pastes.

// Rust guideline compliant 2026-10-09

use gpui::{App, ClipboardItem, EntityId};

use super::state::Selection;

/// Which selection is the latest: only one is kept at a time, so Ctrl+C copies the one last made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Owner {
    #[default]
    None,
    /// The chat's marked text.
    Chat,
    /// A text field, by its entity.
    Field(EntityId),
    /// The terminal.
    Terminal,
}

impl gpui::Global for Owner {}

/// Makes `owner`'s selection the latest. The others drop theirs: a field or the terminal hears
/// of it by observing [`Owner`], and the chat's marks go here.
pub fn claim(owner: Owner, cx: &mut App) {
    if cx.try_global::<Owner>() == Some(&owner) {
        return;
    }
    if owner != Owner::Chat {
        let selection = cx.default_global::<Selection>();
        if !selection.marked.is_empty() || selection.all {
            selection.unmark();
            cx.refresh_windows();
        }
    }
    cx.set_global(owner);
}

/// Whose selection is the latest.
pub fn owner(cx: &App) -> Owner {
    cx.try_global::<Owner>().copied().unwrap_or_default()
}

/// Whether the chat has anything marked.
pub fn has_marks(cx: &mut App) -> bool {
    let selection = cx.default_global::<Selection>();
    !selection.plain.is_empty()
}

/// Puts `text` in the primary selection, which a middle click pastes, where the system has one.
pub fn to_primary(text: &str, cx: &mut App) {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    if !text.is_empty() {
        cx.write_to_primary(ClipboardItem::new_string(text.to_string()));
    }
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    let _ = (text, cx);
}

/// The text in the primary selection, where the system has one.
pub fn from_primary(cx: &mut App) -> Option<String> {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    return cx.read_from_primary().and_then(|item| item.text());
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    {
        let _ = cx;
        None
    }
}
