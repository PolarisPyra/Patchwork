//! Turns Android key and motion events into GPUI input.

// Rust guideline compliant 2026-10-09

use android_activity::AndroidApp;
use android_activity::input::{
    KeyAction, KeyEvent, KeyMapChar, Keycode, MetaState, MotionAction, MotionEvent,
};
use gpui::{
    KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, PlatformInput, Pixels, TouchEvent, TouchId,
    TouchPhase, point, px,
};

/// Converts a motion event into one touch event per pointer it changed.
///
/// `scale` turns the event's physical pixels into GPUI's logical pixels. `touch_base` keeps
/// [`TouchId`]s unique across gestures, since Android reuses pointer ids.
pub(crate) fn touches(event: &MotionEvent<'_>, scale: f32, touch_base: u64) -> Vec<TouchEvent> {
    let touch = |index: usize, phase: TouchPhase| {
        let pointer = event.pointer_at_index(index);
        let pressure = pointer.axis_value(android_activity::input::Axis::Pressure);
        TouchEvent {
            id: TouchId(touch_base.wrapping_add(u64::from(pointer.pointer_id().unsigned_abs()))),
            phase,
            position: point(px(pointer.x() / scale), px(pointer.y() / scale)),
            predicted_position: None,
            force: (pressure > 0.0).then(|| pressure.clamp(0.0, 1.0)),
        }
    };
    let all = |phase: TouchPhase| {
        (0..event.pointer_count())
            .map(|index| touch(index, phase))
            .collect()
    };
    match event.action() {
        MotionAction::Down | MotionAction::PointerDown => {
            vec![touch(event.pointer_index(), TouchPhase::Started)]
        }
        MotionAction::Up | MotionAction::PointerUp => {
            vec![touch(event.pointer_index(), TouchPhase::Ended)]
        }
        MotionAction::Move => all(TouchPhase::Moved),
        MotionAction::Cancel => all(TouchPhase::Cancelled),
        _ => Vec::new(),
    }
}

/// Converts a key event into a GPUI key press or release, or `None` for keys GPUI has no name for.
pub(crate) fn key(app: &AndroidApp, event: &KeyEvent<'_>) -> Option<PlatformInput> {
    let meta = event.meta_state();
    let modifiers = modifiers(meta);
    let key_char = character(app, event, meta);
    let key = match named_key(event.key_code()) {
        Some(name) => name.to_owned(),
        None => key_char.as_ref()?.to_lowercase(),
    };
    let keystroke = Keystroke {
        modifiers,
        key,
        key_char,
    };
    match event.action() {
        KeyAction::Down => Some(PlatformInput::KeyDown(KeyDownEvent {
            keystroke,
            is_held: event.repeat_count() > 0,
            prefer_character_input: false,
        })),
        KeyAction::Up => Some(PlatformInput::KeyUp(KeyUpEvent { keystroke })),
        _ => None,
    }
}

/// Reads the modifier keys held during an event.
pub(crate) fn modifiers(meta: MetaState) -> Modifiers {
    Modifiers {
        control: meta.ctrl_on(),
        alt: meta.alt_on(),
        shift: meta.shift_on(),
        platform: meta.meta_on(),
        function: meta.function_on(),
    }
}

/// Returns the text a key types, if any.
fn character(app: &AndroidApp, event: &KeyEvent<'_>, meta: MetaState) -> Option<String> {
    if matches!(
        event.key_code(),
        Keycode::Enter | Keycode::NumpadEnter | Keycode::Tab
    ) {
        return None;
    }
    let map = app.device_key_character_map(event.device_id()).ok()?;
    match map.get(event.key_code(), meta).ok()? {
        KeyMapChar::Unicode(ch) if !ch.is_control() => Some(ch.to_string()),
        _ => None,
    }
}

/// Names a key the way GPUI's keymaps spell it, for keys that do not type text.
fn named_key(code: Keycode) -> Option<&'static str> {
    Some(match code {
        Keycode::Enter | Keycode::NumpadEnter => "enter",
        Keycode::Del => "backspace",
        Keycode::ForwardDel => "delete",
        Keycode::Tab => "tab",
        Keycode::Escape => "escape",
        Keycode::Space => "space",
        Keycode::DpadLeft => "left",
        Keycode::DpadRight => "right",
        Keycode::DpadUp => "up",
        Keycode::DpadDown => "down",
        Keycode::MoveHome => "home",
        Keycode::MoveEnd => "end",
        Keycode::PageUp => "pageup",
        Keycode::PageDown => "pagedown",
        _ => return None,
    })
}

/// Converts a length in physical pixels to logical pixels.
pub(crate) fn logical(value: i32, scale: f32) -> Pixels {
    px(value as f32 / scale)
}
