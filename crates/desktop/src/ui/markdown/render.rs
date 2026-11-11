//! Markdown drawn as elements: the entry points, and laying the blocks out.

// Rust guideline compliant 2026-10-09

use super::inline::FindIn;
use super::inline::Inline;
use super::inline::Block;
use super::inline::Frame;
use std::ops::Range;
use gpui::{AnyElement, App, Div, SharedString, div, prelude::*, px, relative};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag};
use crate::ui::theme::{Theme, theme};

pub struct Markdown<'a> {
    pub(super) t: &'a Theme,
    /// Prefixes the ids of the code blocks' buttons.
    pub(super) key: SharedString,
    pub(super) code_blocks: usize,
    /// How many texts have been drawn, which with the key tells each from the others.
    pub(super) texts: usize,
    pub(super) stack: Vec<(Frame, Vec<Block>)>,
    pub(super) rows: Vec<Vec<AnyElement>>,
    pub(super) cells: Vec<AnyElement>,
    pub(super) inline: Inline,
    pub(super) code: Option<(String, String)>,
    pub(super) heading: Option<HeadingLevel>,
    /// Draws the composer's chips, as a user's message does; `/goal` counts only as the first word.
    pub(super) chips: bool,
    pub(super) at_start: bool,
    pub(super) find: Option<&'a FindIn>,
    /// The bullet or number of the list item just opened, which its first text copies.
    pub(super) item_prefix: Option<String>,
}

/// Lays `source` out as blocks, inheriting the surrounding text size and colour.
pub fn markdown(source: &str, key: impl Into<SharedString>, cx: &App) -> Div {
    render(source, key.into(), false, false, None, cx)
}

/// Markdown with what find in chat is looking for marked in its text. With `chips`, as a user's
/// message is, `/goal`, `$skill`, `&agent` and `@path` are drawn as the chips the composer showed.
pub fn markdown_found(
    source: &str,
    key: impl Into<SharedString>,
    chips: bool,
    find: Option<&FindIn>,
    cx: &App,
) -> Div {
    render(source, key.into(), chips, false, find, cx)
}

/// One of the parts `parts_of` cut a longer text into. A part after the first keeps the space the
/// block above it, in the part before, leaves under it.
pub fn markdown_part(
    source: &str,
    key: impl Into<SharedString>,
    after_cut: bool,
    find: Option<&FindIn>,
    cx: &App,
) -> Div {
    render(source, key.into(), false, after_cut, find, cx)
}

/// How far a list's items sit in from its bullets and numbers.
pub(super) const LIST_INDENT: f32 = 22.;

/// The bottom margin of the blocks `parts_of` cuts after.
pub(super) const CUT_MB: f32 = 8.;

pub(super) fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS
}

/// Where to cut a long text into parts of about `size` bytes or more, so each can be laid out on
/// its own: only between top-level blocks, and only after a paragraph, list, quote, table or code
/// block, which all leave `CUT_MB` under them, so the parts stack as the whole text would.
pub fn parts_of(text: &str, size: usize) -> Vec<Range<usize>> {
    let mut parts = Vec::new();
    let mut from = 0;
    if text.len() > size {
        let mut depth = 0usize;
        let mut cuttable = false;
        for (event, range) in Parser::new_ext(text, options()).into_offset_iter() {
            match event {
                Event::Start(tag) => {
                    if depth == 0 {
                        if cuttable && range.start - from >= size && !matches!(tag, Tag::HtmlBlock)
                        {
                            parts.push(from..range.start);
                            from = range.start;
                        }
                        cuttable = matches!(
                            tag,
                            Tag::Paragraph
                                | Tag::List(_)
                                | Tag::BlockQuote(_)
                                | Tag::Table(_)
                                | Tag::CodeBlock(_)
                        );
                    }
                    depth += 1;
                }
                Event::End(_) => depth = depth.saturating_sub(1),
                // A rule, or anything else between blocks.
                _ if depth == 0 => cuttable = false,
                _ => {}
            }
        }
    }
    parts.push(from..text.len());
    parts
}

pub(super) fn render(
    source: &str,
    key: SharedString,
    chips: bool,
    after_cut: bool,
    find: Option<&FindIn>,
    cx: &App,
) -> Div {
    let mut md = Markdown {
        t: theme(cx),
        key,
        code_blocks: 0,
        texts: 0,
        stack: vec![(Frame::Root, Vec::new())],
        rows: Vec::new(),
        cells: Vec::new(),
        inline: Inline::default(),
        code: None,
        heading: None,
        chips,
        at_start: true,
        find,
        item_prefix: None,
    };
    for event in Parser::new_ext(source, options()) {
        md.event(event);
    }
    md.flush_paragraph();
    let mut blocks = md.stack.pop().map(|(_, b)| b).unwrap_or_default();
    if after_cut && let Some(first) = blocks.first_mut() {
        first.mt = first.mt.max(CUT_MB);
    }
    stack_blocks(blocks).min_w_0().line_height(relative(1.6))
}

/// Blocks one under another, each pair spaced by the larger of their margins. The first keeps its
/// top margin, as a web page's does when it folds out of the block around it.
pub(super) fn stack_blocks(blocks: Vec<Block>) -> Div {
    let mut out = div().flex().flex_col().min_w_0();
    let mut previous_mb: Option<f32> = None;
    for block in blocks {
        let gap = match previous_mb {
            Some(mb) => mb.max(block.mt),
            None => block.mt,
        };
        out = out.child(div().mt(px(gap)).min_w_0().child(block.el));
        previous_mb = Some(block.mb);
    }
    out
}
