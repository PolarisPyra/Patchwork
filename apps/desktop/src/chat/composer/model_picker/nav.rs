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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_wrap_at_both_ends() {
        assert_eq!(step_row(3, 2, 1), 0);
        assert_eq!(step_row(3, 0, -1), 2);
        assert_eq!(step_row(3, 1, 1), 2);
    }

    #[test]
    fn a_row_past_a_shrunk_list_steps_from_its_end() {
        assert_eq!(step_row(2, 7, -1), 0);
        assert_eq!(step_row(0, 4, 1), 0);
    }

    #[test]
    fn tabs_wrap_and_a_missing_tab_falls_back_to_the_first() {
        let tabs = [
            Tab::Favorites,
            Tab::Agent(AgentKind::Claude),
            Tab::Agent(AgentKind::Codex),
        ];
        assert_eq!(
            step_tab(&tabs, Tab::Favorites, -1),
            Tab::Agent(AgentKind::Codex)
        );
        assert_eq!(
            step_tab(&tabs, Tab::Agent(AgentKind::Claude), 1),
            Tab::Agent(AgentKind::Codex)
        );
        assert_eq!(
            step_tab(&tabs[1..], Tab::Favorites, 1),
            Tab::Agent(AgentKind::Claude)
        );
    }
}
