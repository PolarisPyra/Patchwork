//! Keys pressed while a command's key is being recorded. They are watched before the window's
//! bindings see them, so a key recorded never runs its command; every other key goes on as it
//! would have, to the text field or the command.

// Rust guideline compliant 2026-10-09

use gpui::{Context, KeystrokeEvent, Window};

use super::bindings::{key_from_keystroke, key_problem, keys_for};
use super::list::Spot;
use crate::settings::SettingsDialog;

impl SettingsDialog {
    /// Starts watching keys pressed in `window`, for recording.
    pub(crate) fn watch_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let ours = window.window_handle().window_id();
        let keys = cx.intercept_keystrokes(move |e, window, cx| {
            if window.window_handle().window_id() != ours {
                return;
            }
            let taken = this
                .update(cx, |d, cx| d.key_pressed(e, cx))
                .unwrap_or(false);
            if taken {
                cx.stop_propagation();
            }
        });
        self.shortcuts.watches = vec![keys];
    }

    /// A key down: true when it is recorded, or refused, and goes no further.
    fn key_pressed(&mut self, e: &KeystrokeEvent, cx: &mut Context<Self>) -> bool {
        if self.section != "shortcuts" {
            return false;
        }
        let s = &mut self.shortcuts;
        let Some(id) = s.recording else {
            return false;
        };
        let k = &e.keystroke;
        cx.notify();
        if k.key == "escape" && !k.modifiers.modified() {
            s.recording = None;
            s.refused = None;
            return true;
        }
        // A modifier alone waits for the key it goes with.
        let Some(key) = key_from_keystroke(k) else {
            return true;
        };
        if let Some(problem) = key_problem(id, &key) {
            s.refused = Some(problem);
            return true;
        }
        let mut keys = keys_for(
            s.overrides
                .as_ref()
                .unwrap_or(&std::collections::HashMap::default()),
            id,
        );
        if keys.contains(&key) {
            s.refused = Some("That is already one of its keys".to_owned());
            return true;
        }
        let at = match s.slot {
            Some(at) if at < keys.len() => {
                keys[at] = key;
                at
            }
            _ => {
                keys.push(key);
                keys.len() - 1
            }
        };
        s.recording = None;
        s.refused = None;
        s.spot = Spot::Command(id);
        s.shown_binding = at;
        self.set_keys(id, &keys, cx);
        true
    }
}
