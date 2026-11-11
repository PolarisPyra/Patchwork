//! Markdown styled as it is typed: bold, italic, code, struck-through text and headings take
//! their look, and the marks that make them stay visible, dimmed, so the text is still what is
//! sent.

// Rust guideline compliant 2026-11-10

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag};

use super::state::TextArea;

pub(super) const BOLD: u8 = 1;
pub(super) const ITALIC: u8 = 1 << 1;
pub(super) const CODE: u8 = 1 << 2;
pub(super) const STRIKE: u8 = 1 << 3;
/// A mark that makes the style, such as `**` or `#`, drawn dimmed.
pub(super) const MARK: u8 = 1 << 4;

/// The style of each byte of `text`, as a set of the flags above.
pub(super) fn styles(text: &str) -> Vec<u8> {
    let mut flags = vec![0_u8; text.len()];
    let mut set = |range: Range<usize>, flag: u8| {
        let range = range.start.min(text.len())..range.end.min(text.len());
        for f in &mut flags[range] {
            *f |= flag;
        }
    };
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        let source = &text[range.clone()];
        match event {
            Event::Start(Tag::Strong) => {
                set(range.clone(), BOLD);
                marks(source, range, 2, &mut set);
            }
            Event::Start(Tag::Emphasis) => {
                set(range.clone(), ITALIC);
                marks(source, range, 1, &mut set);
            }
            Event::Start(Tag::Strikethrough) => {
                set(range.clone(), STRIKE);
                let width = source.len() - source.trim_start_matches('~').len();
                marks(source, range, width, &mut set);
            }
            Event::Code(_) => {
                set(range.clone(), CODE);
                let width = source.len() - source.trim_start_matches('`').len();
                marks(source, range, width, &mut set);
            }
            Event::Start(Tag::CodeBlock(_)) => set(range, CODE),
            Event::Start(Tag::Heading { .. }) => {
                set(range.clone(), BOLD);
                let hashes = source.len() - source.trim_start_matches('#').len();
                set(range.start..range.start + hashes, MARK);
            }
            Event::Start(Tag::Item) => {
                set(range.start..range.start + item_marker(source), MARK);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                let mut at = range.start;
                for line in source.split_inclusive('\n') {
                    let quote = line.len() - line.trim_start().trim_start_matches('>').len();
                    set(at..at + quote, MARK);
                    at += line.len();
                }
            }
            _ => {}
        }
    }
    flags
}

/// Dims the `width` bytes of marks at each end of `source`, which starts at `range.start`.
fn marks(source: &str, range: Range<usize>, width: usize, set: &mut impl FnMut(Range<usize>, u8)) {
    if source.len() >= width * 2 {
        set(range.start..range.start + width, MARK);
        set(range.end - width..range.end, MARK);
    }
}

/// How long a list item's bullet or number is, with the space after it.
fn item_marker(source: &str) -> usize {
    let digits = source.len()
        - source
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .len();
    let rest = &source[digits..];
    let marker = match rest.chars().next() {
        Some('.' | ')') if digits > 0 => digits + 1,
        Some('-' | '*' | '+') if digits == 0 => 1,
        _ => return 0,
    };
    let spaces = source[marker..].len() - source[marker..].trim_start_matches(' ').len();
    marker + spaces
}

impl TextArea {
    /// The style of each byte, when the field styles Markdown and has text to style.
    pub(super) fn styles(&self) -> Option<Vec<u8>> {
        (self.markdown && !self.masked && !self.content.is_empty()).then(|| styles(&self.content))
    }
}
