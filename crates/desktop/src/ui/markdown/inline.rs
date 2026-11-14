//! The pieces a paragraph is built from as it is read: its text runs, what a search found in it, and the block it is inside.

// Rust guideline compliant 2026-10-09

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use gpui::{AnyElement, Div, HighlightStyle, Hsla, SharedString, StyledText, TextLayout};
use crate::ui::theme::FONT_MONO;

/// One block with the margins CSS gives it; adjacent margins collapse into the larger one.
pub(super) struct Block {
    pub(super) el: AnyElement,
    pub(super) mt: f32,
    pub(super) mb: f32,
}

/// Text being gathered for one paragraph, heading, item or cell, with its spans.
#[derive(Default)]
pub(super) struct Inline {
    pub(super) text: String,
    pub(super) highlights: Vec<(Range<usize>, HighlightStyle)>,
    pub(super) mono: Vec<Range<usize>>,
    /// Sigil chips, drawn as rounded shapes behind the text rather than as a run background.
    pub(super) chips: Vec<(Range<usize>, Hsla)>,
    /// Links a click opens, each with its address.
    pub(super) links: Vec<(Range<usize>, SharedString)>,
    /// The open link: where it started and its address.
    pub(super) link: Option<(usize, String)>,
    /// Open spans: where each started and how it looks.
    open: Vec<(usize, HighlightStyle)>,
}

impl Inline {
    pub(super) fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub(super) fn open(&mut self, style: HighlightStyle) {
        self.open.push((self.text.len(), style));
    }

    pub(super) fn close(&mut self) {
        if let Some((start, style)) = self.open.pop()
            && start < self.text.len()
        {
            self.highlights.push((start..self.text.len(), style));
        }
    }

    /// The gathered text as one wrapping run, its spans merged where they overlap.
    pub(super) fn take(&mut self) -> StyledText {
        let inline = std::mem::take(self);
        let text = SharedString::from(inline.text.trim_end().to_string());
        let len = text.len();
        let mut bounds: Vec<usize> = vec![0, len];
        for (r, _) in &inline.highlights {
            bounds.extend([r.start.min(len), r.end.min(len)]);
        }
        bounds.sort_unstable();
        bounds.dedup();
        let mut highlights = Vec::new();
        for pair in bounds.windows(2) {
            let piece = pair[0]..pair[1];
            let mut style = HighlightStyle::default();
            let mut any = false;
            for (r, s) in &inline.highlights {
                if r.start <= piece.start && piece.end <= r.end {
                    style = style.highlight(*s);
                    any = true;
                }
            }
            if any && !piece.is_empty() {
                highlights.push((piece, style));
            }
        }
        // Sorted and apart, as the font overrides must be: a chip and the space beside it are
        // pushed out of order, and two chips can share one space.
        let mut mono = inline.mono;
        mono.sort_by_key(|r| (r.start, r.end));
        mono.dedup();
        let mono: Vec<_> = mono
            .into_iter()
            .filter(|r| r.end <= len)
            .map(|r| (r, SharedString::from(FONT_MONO)))
            .collect();
        StyledText::new(text)
            .with_highlights(highlights)
            .with_font_family_overrides(mono)
    }
}

/// What find in chat marks in one message: the query, already lowercased, which of the message's
/// matches is the current one, and how many have been marked so far across its texts.
pub struct FindIn {
    pub needle: String,
    pub current: Option<usize>,
    pub seen: Cell<usize>,
    /// Where the current match was drawn, so the transcript can scroll to it.
    pub target: Rc<RefCell<Option<(TextLayout, usize)>>>,
}

/// Where `needle`, already lowercased, starts in `text`, without overlaps and ignoring case.
/// Only ASCII is folded, so the offsets stay those of `text`.
pub fn find_in(text: &str, needle: &str) -> Vec<usize> {
    if needle.trim().is_empty() {
        return Vec::new();
    }
    let haystack = text.to_ascii_lowercase();
    let mut at = 0;
    let mut found = Vec::new();
    while let Some(i) = haystack[at..].find(needle) {
        found.push(at + i);
        at += i + needle.len();
    }
    found
}

/// Whether a click may hand `url` to the browser or mail client.
pub(super) fn opens_outside(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["https://", "http://", "mailto:"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

/// Where bare `http://` and `https://` addresses sit in `text`, without the punctuation that ends
/// the sentence around them or a closing bracket they did not open.
pub(super) fn urls_in(text: &str) -> Vec<Range<usize>> {
    let mut urls = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find("http") {
        let start = from + i;
        let rest = &text[start..];
        let boundary = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let scheme = ["https://", "http://"]
            .into_iter()
            .find(|s| rest.starts_with(s));
        let Some(scheme) = scheme.filter(|_| boundary) else {
            from = start + "http".len();
            continue;
        };
        let mut end = rest
            .find(|c: char| c.is_whitespace() || "<>\"`".contains(c))
            .unwrap_or(rest.len());
        loop {
            let url = &rest[..end];
            let Some(last) = url.chars().next_back() else {
                break;
            };
            let unopened = |open: char| url.matches(last).count() > url.matches(open).count();
            let trim = ".,;:!?'*_".contains(last)
                || (last == ')' && unopened('('))
                || (last == ']' && unopened('['));
            if !trim {
                break;
            }
            end -= last.len_utf8();
        }
        if end > scheme.len() {
            urls.push(start..start + end);
        }
        from = start + end.max(scheme.len());
    }
    urls
}

/// A container blocks are gathered into while it is open.
pub(super) enum Frame {
    Root,
    Quote,
    List { start: Option<u64>, items: Vec<Div> },
    Item,
    Table,
    Row { head: bool },
}
