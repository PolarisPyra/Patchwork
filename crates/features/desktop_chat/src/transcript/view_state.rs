//! What a transcript keeps while it shows: its scroll, the rows it was cut into, and what is open.

// Rust guideline compliant 2026-11-27

use std::collections::HashSet;

use gpui::{App, ListAlignment, ListState, Pixels, px};

use crate::transcript::message_list::{Pieces, Row};
use crate::transcript::reply::checks::ChecksState;
use crate::transcript::turn_rail::RailState;
use crate::transcript::user_message::UserState;

/// One chat's transcript as drawn: scroll, layout, and the folds and cards opened.
///
/// The workspace holds the open chat's; each pane out of focus holds its own chat's, and the two
/// trade places when the focus moves between panes.
pub struct TranscriptView {
    /// The transcript, drawn a screenful at a time.
    pub list: ListState,
    /// The chat the list last held, and the items it was cut into.
    pub rows: (Option<String>, Vec<Row>),
    /// The pieces each shown message was last cut into, so a frame cuts only what changed.
    pub pieces: Vec<Pieces>,
    /// The transcript's width last laid out.
    pub width: Pixels,
    /// Replies seen streaming, whose steps start open once they settle.
    pub streamed: HashSet<String>,
    /// Replies whose folded steps are open, and steps whose details are.
    pub open_folds: HashSet<String>,
    pub open_steps: HashSet<String>,
    /// Runs of subagents listed open, by their first call.
    pub open_agents: HashSet<String>,
    /// Review cards turned from how they first show, by reply.
    pub open_reviews: HashSet<String>,
    pub rail: RailState,
    pub user: UserState,
    pub checks: ChecksState,
}

impl TranscriptView {
    /// Nothing laid out, scrolled, or opened yet.
    pub fn new(cx: &mut App) -> Self {
        Self {
            // Every row is measured up front, or the scrollbar thumb jumps as rows scroll in.
            list: ListState::new(0, ListAlignment::Top, px(800.)).measure_all(),
            rows: (None, Vec::new()),
            pieces: Vec::new(),
            width: px(0.),
            streamed: HashSet::new(),
            open_folds: HashSet::new(),
            open_steps: HashSet::new(),
            open_agents: HashSet::new(),
            open_reviews: HashSet::new(),
            rail: RailState::new(cx),
            user: UserState::default(),
            checks: ChecksState::default(),
        }
    }
}
