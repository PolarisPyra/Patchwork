//! What the terminal sends a shell for a key or the mouse, as xterm sends it, and the colour
//! rule xterm draws its text by.

use gpui::{Hsla, Keystroke, Modifiers, Rgba};
use vt100::{MouseProtocolEncoding as Encoding, MouseProtocolMode as Mode};

/// The bytes a terminal sends for a key, or None for a key it sends nothing for.
pub fn key_bytes(ks: &Keystroke, app_cursor: bool) -> Option<String> {
    let m = &ks.modifiers;
    let modifier = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.control);
    let cursor = |end: char| {
        Some(if modifier > 1 {
            format!("\x1b[1;{modifier}{end}")
        } else if app_cursor {
            format!("\x1bO{end}")
        } else {
            format!("\x1b[{end}")
        })
    };
    let tilde = |n: u8| {
        Some(if modifier > 1 {
            format!("\x1b[{n};{modifier}~")
        } else {
            format!("\x1b[{n}~")
        })
    };
    let alt = |s: &str| {
        if m.alt {
            format!("\x1b{s}")
        } else {
            s.to_string()
        }
    };
    match ks.key.as_str() {
        "enter" => Some(alt("\r")),
        "backspace" => Some(alt(if m.control { "\x08" } else { "\x7f" })),
        "tab" => Some(if m.shift {
            "\x1b[Z".into()
        } else {
            "\t".into()
        }),
        "escape" => Some("\x1b".into()),
        "up" => cursor('A'),
        "down" => cursor('B'),
        "right" => cursor('C'),
        "left" => cursor('D'),
        "home" => cursor('H'),
        "end" => cursor('F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "f1" => Some("\x1bOP".into()),
        "f2" => Some("\x1bOQ".into()),
        "f3" => Some("\x1bOR".into()),
        "f4" => Some("\x1bOS".into()),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        "space" if m.control => Some("\0".into()),
        key if m.control && !m.platform => {
            let mut chars = key.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            let code = match c.to_ascii_lowercase() {
                c @ 'a'..='z' => c as u8 - b'a' + 1,
                '@' | '2' => 0,
                '[' | '3' => 27,
                '\\' | '4' => 28,
                ']' | '5' => 29,
                '^' | '6' => 30,
                '_' | '-' | '7' => 31,
                '?' | '8' => 127,
                _ => return None,
            };
            Some(alt(&(code as char).to_string()))
        }
        _ if m.platform => None,
        key => {
            let text = ks.key_char.clone().or_else(|| {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Some(c.to_string()),
                    _ => None,
                }
            })?;
            Some(alt(&text))
        }
    }
}

/// What the mouse did, for a program on the screen that asked to hear of it. A button is 0 for
/// the left, 1 for the middle, 2 for the right.
#[derive(Clone, Copy, PartialEq)]
pub enum Mouse {
    Press(u8),
    Release(u8),
    Drag(u8),
    Move,
    WheelUp,
    WheelDown,
}

/// The report of `event` at `cell` (row, column), or None when the program on `screen` has not
/// asked for it.
pub fn mouse_bytes(
    screen: &vt100::Screen,
    event: Mouse,
    cell: (u16, u16),
    m: &Modifiers,
) -> Option<String> {
    let mode = screen.mouse_protocol_mode();
    let wanted = match event {
        Mouse::Press(_) | Mouse::WheelUp | Mouse::WheelDown => mode != Mode::None,
        Mouse::Release(_) => !matches!(mode, Mode::None | Mode::Press),
        Mouse::Drag(_) => matches!(mode, Mode::ButtonMotion | Mode::AnyMotion),
        Mouse::Move => mode == Mode::AnyMotion,
    };
    if !wanted {
        return None;
    }
    let encoding = screen.mouse_protocol_encoding();
    let button = match event {
        Mouse::Press(b) => b,
        // Only SGR says which button came up.
        Mouse::Release(b) if encoding == Encoding::Sgr => b,
        Mouse::Release(_) => 3,
        Mouse::Drag(b) => b + 32,
        Mouse::Move => 3 + 32,
        Mouse::WheelUp => 64,
        Mouse::WheelDown => 65,
    };
    let code = u32::from(button)
        + 4 * u32::from(m.shift)
        + 8 * u32::from(m.alt)
        + 16 * u32::from(m.control);
    let (x, y) = (u32::from(cell.1) + 1, u32::from(cell.0) + 1);
    match encoding {
        Encoding::Sgr => {
            let end = if matches!(event, Mouse::Release(_)) {
                'm'
            } else {
                'M'
            };
            Some(format!("\x1b[<{code};{x};{y}{end}"))
        }
        // Each value is one character past 32. The plain encoding stops where a byte would no
        // longer be ASCII, as the shell is written text; UTF-8 goes on to 2015.
        Encoding::Default | Encoding::Utf8 => {
            let most = if encoding == Encoding::Utf8 { 2015 } else { 95 };
            let char_of = |v: u32| (v <= most).then(|| char::from_u32(v + 32)).flatten();
            Some(format!(
                "\x1b[M{}{}{}",
                char_of(code)?,
                char_of(x)?,
                char_of(y)?
            ))
        }
    }
}

/// xterm's `minimumContrastRatio`: `fg` lightened or darkened in steps of a tenth until it stands
/// out from `bg` by `ratio`, whichever way gets closer when neither reaches it.
pub fn ensure_contrast(fg: Hsla, bg: Hsla, ratio: f32) -> Hsla {
    let (f, b) = (Rgba::from(fg), Rgba::from(bg));
    let bg_l = luminance([b.r, b.g, b.b]);
    let fg_rgb = [f.r, f.g, f.b];
    if contrast(luminance(fg_rgb), bg_l) >= ratio {
        return fg;
    }
    let darker = step_until(fg_rgb, bg_l, ratio, |c| c - (c * 255. * 0.1).ceil() / 255.);
    let lighter = step_until(fg_rgb, bg_l, ratio, |c| {
        c + ((1. - c) * 255. * 0.1).ceil() / 255.
    });
    let ratio_of = |c: [f32; 3]| contrast(luminance(c), bg_l);
    let (first, second) = if luminance(fg_rgb) < bg_l {
        (darker, lighter)
    } else {
        (lighter, darker)
    };
    let best = if ratio_of(first) >= ratio || ratio_of(first) > ratio_of(second) {
        first
    } else {
        second
    };
    Rgba {
        r: best[0],
        g: best[1],
        b: best[2],
        a: f.a,
    }
    .into()
}

fn step_until(mut c: [f32; 3], bg_l: f32, ratio: f32, step: impl Fn(f32) -> f32) -> [f32; 3] {
    // Each step moves every channel at least one level, so 256 always ends it.
    for _ in 0..256 {
        let moved = c.map(|v| step(v).clamp(0., 1.));
        if contrast(luminance(c), bg_l) >= ratio || moved == c {
            break;
        }
        c = moved;
    }
    c
}

fn luminance(c: [f32; 3]) -> f32 {
    let lin = |v: f32| {
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

fn contrast(a: f32, b: f32) -> f32 {
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}
