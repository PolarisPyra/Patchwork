//! The characters that open the composer's menus: files, skills, agents and
//! commands. The user can type a different one for each, but what is sent always uses the
//! default, which is what the agents and the backend read. The changed ones are kept in the app
//! state under `sigils`.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use gpui::{App, Global};
use patchwork_server::Server;

const STATE_KEY: &str = "sigils";

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SigilKind {
    Files,
    Skills,
    Agents,
    Commands,
}

/// Each kind, its key in the saved state, and what its menu does.
pub const SIGIL_KINDS: [(SigilKind, &str, &str); 4] = [
    (SigilKind::Files, "files", "Mention a file or folder"),
    (SigilKind::Skills, "skills", "Use a skill"),
    (SigilKind::Agents, "agents", "Summon an agent"),
    (SigilKind::Commands, "commands", "Run a command"),
];

pub fn default_sigil(kind: SigilKind) -> char {
    match kind {
        SigilKind::Files => '@',
        SigilKind::Skills => '$',
        SigilKind::Agents => '&',
        SigilKind::Commands => '/',
    }
}

/// One symbol or punctuation mark: letters, digits and spaces are part of what is typed.
pub fn is_sigil(c: char) -> bool {
    !c.is_alphanumeric() && !c.is_whitespace() && !c.is_control()
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Sigils(HashMap<SigilKind, char>);

impl Global for Sigils {}

impl Default for Sigils {
    fn default() -> Self {
        Self(
            SIGIL_KINDS
                .iter()
                .map(|&(k, _, _)| (k, default_sigil(k)))
                .collect(),
        )
    }
}

impl Sigils {
    pub fn of(&self, kind: SigilKind) -> char {
        self.0
            .get(&kind)
            .copied()
            .unwrap_or_else(|| default_sigil(kind))
    }

    /// The kind `c` opens, if any.
    pub fn kind_of(&self, c: char) -> Option<SigilKind> {
        SIGIL_KINDS
            .iter()
            .map(|&(k, _, _)| k)
            .find(|&k| self.of(k) == c)
    }

    pub fn changed(&self) -> bool {
        *self != Self::default()
    }

    /// Why `c` cannot open `kind`'s menu, or `None` when it can.
    pub fn problem(&self, kind: SigilKind, c: char) -> Option<String> {
        if !is_sigil(c) {
            return Some("Use a single symbol, such as # or !".into());
        }
        SIGIL_KINDS
            .iter()
            .find(|&&(k, _, _)| k != kind && self.of(k) == c)
            .map(|(_, _, title)| format!("{c} already does: {title}"))
    }

    pub fn with(&self, kind: SigilKind, c: char) -> Self {
        let mut next = self.clone();
        next.0.insert(kind, c);
        next
    }

    /// `kind` back to its default; if another kind has taken that meanwhile, everything goes back.
    pub fn reset(&self, kind: SigilKind) -> Self {
        let next = self.with(kind, default_sigil(kind));
        if next.distinct() {
            next
        } else {
            Self::default()
        }
    }

    fn distinct(&self) -> bool {
        let mut chars: Vec<char> = SIGIL_KINDS.iter().map(|&(k, _, _)| self.of(k)).collect();
        chars.sort_unstable();
        chars.dedup();
        chars.len() == SIGIL_KINDS.len()
    }
}

/// The saved sigils. Two kinds sharing a character would be ambiguous, so a clash falls back to
/// the defaults.
pub fn load(server: &Server) -> Sigils {
    let saved: HashMap<String, String> = server
        .store
        .state(STATE_KEY)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let mut sigils = Sigils::default();
    for &(kind, key, _) in &SIGIL_KINDS {
        let mut chars = saved.get(key).map(|s| s.chars()).into_iter().flatten();
        if let (Some(c), None) = (chars.next(), chars.next())
            && is_sigil(c)
        {
            sigils.0.insert(kind, c);
        }
    }
    if sigils.distinct() {
        sigils
    } else {
        Sigils::default()
    }
}

/// Saves `sigils`, keeping only the ones changed, and makes them the ones the composer reads.
pub fn save(server: &Server, sigils: Sigils, cx: &mut App) {
    let changed: HashMap<&str, String> = SIGIL_KINDS
        .iter()
        .filter(|&&(k, _, _)| sigils.of(k) != default_sigil(k))
        .map(|&(k, key, _)| (key, sigils.of(k).to_string()))
        .collect();
    let json = serde_json::to_string(&changed).unwrap_or_else(|_| "{}".into());
    if let Err(e) = server.store.set_state(STATE_KEY, &json) {
        log::warn!(error:% = e; "could not save the sigils: {{error}}");
    }
    cx.set_global(sigils);
}
