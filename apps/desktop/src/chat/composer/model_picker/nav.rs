//! Where the picker's keys are: the rail tab whose models are listed, and the row under the keys,
//! stepped by the arrows and wrapping at either end.

// Rust guideline compliant 2026-10-09

use patchwork_backend::provider::AgentKind;

/// A tab on the picker's rail: the starred models, or one agent's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Favorites,
    Agent(AgentKind),
}

/// The tab `by` steps from `tab` along `tabs`, wrapping round; the first tab when `tab` is gone.
pub fn step_tab(tabs: &[Tab], tab: Tab, by: isize) -> Tab {
    let Some(at) = tabs.iter().position(|&t| t == tab) else {
        return tabs.first().copied().unwrap_or(tab);
    };
    tabs[wrap(tabs.len(), at, by)]
}

/// The row `by` steps from `index` in a list of `len`, wrapping round.
pub fn step_row(len: usize, index: usize, by: isize) -> usize {
    if len == 0 {
        return 0;
    }
    wrap(len, index.min(len - 1), by)
}

/// `at` moved `by` places round a ring of `len`, which must not be empty.
fn wrap(len: usize, at: usize, by: isize) -> usize {
    let len = isize::try_from(len).unwrap_or(isize::MAX);
    let at = isize::try_from(at).unwrap_or(0);
    usize::try_from((at + by).rem_euclid(len)).unwrap_or(0)
}
