//! Keys as chords: the modifiers held and the one key pressed with them, read from a stored key,
//! a search, or a fixed key's names, so all three can be compared.

// Rust guideline compliant 2026-10-09

/// The Ctrl modifier, which a stored key writes as `mod`.
pub(super) const CTRL: u8 = 1;
/// The Alt or Option modifier.
pub(super) const ALT: u8 = 2;
/// The Shift modifier.
pub(super) const SHIFT: u8 = 4;
/// The Super, Windows or Command modifier.
pub(super) const SUPER: u8 = 8;

/// Each modifier with the cap it lights, in the order a key's name lists them.
pub(super) const MODIFIERS: [(u8, &str); 4] = [
    (CTRL, "control"),
    (SUPER, "platform"),
    (ALT, "alt"),
    (SHIFT, "shift"),
];

/// Every key with a cap besides the modifiers, by the name GPUI gives it.
const KEYS: &[&str] = &[
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "`",
    "-",
    "=",
    "[",
    "]",
    "\\",
    ";",
    "'",
    ",",
    ".",
    "/",
    "space",
    "enter",
    "backspace",
    "tab",
    "escape",
    "capslock",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
    "insert",
    "delete",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "printscreen",
    "scrolllock",
    "pause",
    "menu",
];

/// Modifiers held, and the key pressed with them, if one is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub(super) struct Chord {
    /// [`CTRL`], [`ALT`], [`SHIFT`] and [`SUPER`], or'd together.
    pub(super) mods: u8,
    /// The key's cap name, or None for modifiers alone.
    pub(super) key: Option<&'static str>,
}

/// The cap a shifted symbol is typed on.
fn unshifted(symbol: &str) -> Option<&'static str> {
    Some(match symbol {
        "~" => "`",
        "!" => "1",
        "@" => "2",
        "#" => "3",
        "$" => "4",
        "%" => "5",
        "^" => "6",
        "&" => "7",
        "*" => "8",
        "(" => "9",
        ")" => "0",
        "_" => "-",
        "+" | "plus" => "=",
        "{" => "[",
        "}" => "]",
        "|" => "\\",
        ":" => ";",
        "\"" => "'",
        "<" => ",",
        ">" => ".",
        "?" => "/",
        _ => return None,
    })
}

/// The modifier a name stands for, as GPUI, a stored key or a person writes it.
fn modifier(name: &str) -> Option<u8> {
    Some(match name {
        "mod" | "ctrl" | "control" | "ctl" => CTRL,
        "alt" | "option" | "opt" => ALT,
        "shift" => SHIFT,
        "super" | "cmd" | "command" | "win" | "windows" | "meta" | "platform" => SUPER,
        _ => return None,
    })
}

/// The cap a key's name is on, and whether it takes Shift to type, for names as GPUI, a stored
/// key, a fixed key or a person writes them.
pub(super) fn cap_of(name: &str) -> Option<(&'static str, bool)> {
    let lower = name.to_lowercase();
    let name = match lower.as_str() {
        "esc" => "escape",
        "return" => "enter",
        "del" => "delete",
        "ins" => "insert",
        "pgup" | "page up" => "pageup",
        "pgdn" | "page down" => "pagedown",
        "spacebar" | " " => "space",
        "caps" | "caps lock" => "capslock",
        other => other,
    };
    if let Some(cap) = unshifted(name) {
        return Some((cap, true));
    }
    KEYS.iter().find(|k| **k == name).map(|k| (*k, false))
}

impl Chord {
    /// A stored key, "mod+shift+k".
    pub(super) fn from_stored(key: &str) -> Option<Self> {
        let mut chord = Self::default();
        for part in key.split('+') {
            if let Some(bit) = modifier(part) {
                chord.mods |= bit;
                continue;
            }
            let (cap, shifted) = cap_of(part)?;
            chord.key = Some(cap);
            if shifted {
                chord.mods |= SHIFT;
            }
        }
        Some(chord)
    }

    /// A search that names keys: "ctrl+c", "Ctrl C", "ctrl-shift-k", "esc". None when any word
    /// is not a key, or when it is one letter alone, which is more likely the start of a word.
    pub(super) fn from_query(query: &str) -> Option<Self> {
        let words: Vec<&str> = query
            .split(|c: char| c == '+' || c.is_whitespace())
            .flat_map(|w| {
                if w.len() > 1 && w.contains('-') {
                    w.split('-').collect()
                } else {
                    vec![w]
                }
            })
            .filter(|w| !w.is_empty())
            .collect();
        let mut chord = Self::default();
        for word in &words {
            if let Some(bit) = modifier(&word.to_lowercase()) {
                chord.mods |= bit;
                continue;
            }
            let (cap, shifted) = cap_of(word)?;
            if chord.key.is_some() {
                return None;
            }
            chord.key = Some(cap);
            if shifted {
                chord.mods |= SHIFT;
            }
        }
        let lone_letter = words.len() == 1 && words[0].chars().count() == 1;
        (!words.is_empty() && !lone_letter).then_some(chord)
    }

    /// A fixed key's names, `["Ctrl", "Shift", "C"]`. Keys named with no modifier are each a
    /// key of their own, as `["Up", "Down"]`; names that are no key, as a click, are left out.
    pub(super) fn from_names(names: &[&str]) -> Vec<Self> {
        let mods = names
            .iter()
            .filter_map(|n| modifier(&n.to_lowercase()))
            .fold(0, |all, bit| all | bit);
        let keys: Vec<(&'static str, bool)> = names
            .iter()
            .filter(|n| modifier(&n.to_lowercase()).is_none())
            .filter_map(|n| cap_of(n))
            .collect();
        let one = |(cap, shifted): (&'static str, bool)| Self {
            mods: if shifted { mods | SHIFT } else { mods },
            key: Some(cap),
        };
        match keys.as_slice() {
            [] if mods != 0 => vec![Self { mods, key: None }],
            [] => Vec::new(),
            [key] => vec![one(*key)],
            many if mods == 0 => many.iter().copied().map(one).collect(),
            [.., last] => vec![one(*last)],
        }
    }

    /// The chord's keys as the list shows them, `["Ctrl", "Shift", "K"]`.
    pub(super) fn parts(self) -> Vec<String> {
        let mut parts: Vec<String> = MODIFIERS
            .into_iter()
            .filter(|(bit, _)| self.mods & bit != 0)
            .map(|(_, cap)| modifier_label(cap).to_owned())
            .collect();
        if let Some(key) = self.key {
            parts.push(key_label(key));
        }
        parts
    }
}

/// A modifier cap's name as this platform writes it.
pub(super) fn modifier_label(cap: &str) -> &'static str {
    let mac = cfg!(target_os = "macos");
    match cap {
        "control" => "Ctrl",
        "alt" if mac => "Option",
        "alt" => "Alt",
        "shift" => "Shift",
        "platform" if mac => "Cmd",
        "platform" if cfg!(target_os = "windows") => "Win",
        "platform" => "Super",
        _ => "",
    }
}

/// A key cap's name as the list writes it: "K", "Esc", "`PgUp`", "F5".
pub(super) fn key_label(cap: &str) -> String {
    match cap {
        "escape" => "Esc".into(),
        "pageup" => "PgUp".into(),
        "pagedown" => "PgDn".into(),
        "capslock" => "Caps Lock".into(),
        "printscreen" => "PrtSc".into(),
        "scrolllock" => "ScrLk".into(),
        other => patchwork_ui::display_title(other),
    }
}
