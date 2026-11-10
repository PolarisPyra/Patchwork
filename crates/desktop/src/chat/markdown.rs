//! Markdown as the chat draws it: paragraphs, headings,
//! lists, quotes, rules, tables and fenced code, with bold, italic, struck, linked and code spans.

// Rust guideline compliant 2026-10-09

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AnyElement, App, ClipboardItem, Div, FontStyle, FontWeight, HighlightStyle, SharedString,
    StrikethroughStyle, StyledText, TextLayout, UnderlineStyle, div, prelude::*, px, relative,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::ui::theme::{FONT_MONO, FS_LG, FS_MD, FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme, theme};
use crate::ui::{button, icon};

/// One block with the margins CSS gives it; adjacent margins collapse into the larger one.
struct Block {
    el: AnyElement,
    mt: f32,
    mb: f32,
}

/// Text being gathered for one paragraph, heading, item or cell, with its spans.
#[derive(Default)]
struct Inline {
    text: String,
    highlights: Vec<(Range<usize>, HighlightStyle)>,
    mono: Vec<Range<usize>>,
    /// Open spans: where each started and how it looks.
    open: Vec<(usize, HighlightStyle)>,
}

impl Inline {
    fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    fn open(&mut self, style: HighlightStyle) {
        self.open.push((self.text.len(), style));
    }

    fn close(&mut self) {
        if let Some((start, style)) = self.open.pop()
            && start < self.text.len()
        {
            self.highlights.push((start..self.text.len(), style));
        }
    }

    /// The gathered text as one wrapping run, its spans merged where they overlap.
    fn take(&mut self) -> StyledText {
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
        let mono: Vec<_> = inline
            .mono
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

/// A container blocks are gathered into while it is open.
enum Frame {
    Root,
    Quote,
    List { start: Option<u64>, items: Vec<Div> },
    Item,
    Table,
    Row { head: bool },
}

pub struct Markdown<'a> {
    t: &'a Theme,
    /// Prefixes the ids of the code blocks' buttons.
    key: SharedString,
    code_blocks: usize,
    stack: Vec<(Frame, Vec<Block>)>,
    rows: Vec<Vec<AnyElement>>,
    cells: Vec<AnyElement>,
    inline: Inline,
    code: Option<(String, String)>,
    heading: Option<HeadingLevel>,
    /// Draws the composer's chips, as a user's message does; `/goal` counts only as the first word.
    chips: bool,
    at_start: bool,
    find: Option<&'a FindIn>,
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

/// The bottom margin of the blocks `parts_of` cuts after.
const CUT_MB: f32 = 8.;

fn options() -> Options {
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

fn render(
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
        stack: vec![(Frame::Root, Vec::new())],
        rows: Vec::new(),
        cells: Vec::new(),
        inline: Inline::default(),
        code: None,
        heading: None,
        chips,
        at_start: true,
        find,
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
fn stack_blocks(blocks: Vec<Block>) -> Div {
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

impl Markdown<'_> {
    /// The open run as text, with find's matches marked: the current one solid, the rest faint.
    fn take_inline(&mut self) -> StyledText {
        let Some(find) = self.find else {
            return self.inline.take();
        };
        let t = self.t;
        let len = self.inline.text.trim_end().len();
        let mut current = None;
        for start in find_in(&self.inline.text, &find.needle) {
            let range = start..start + find.needle.len();
            if range.end > len {
                break;
            }
            let nth = find.seen.get();
            find.seen.set(nth + 1);
            let style = if find.current == Some(nth) {
                current = Some(start);
                HighlightStyle {
                    background_color: Some(t.warning),
                    color: Some(t.bg_editor),
                    ..Default::default()
                }
            } else {
                HighlightStyle {
                    background_color: Some(t.warning.opacity(0.28)),
                    ..Default::default()
                }
            };
            self.inline.highlights.push((range, style));
        }
        let text = self.inline.take();
        if let Some(start) = current {
            *find.target.borrow_mut() = Some((text.layout().clone(), start));
        }
        text
    }

    /// Adds text to the open run, marking its chips when this message draws them.
    fn text(&mut self, text: &str) {
        let base = self.inline.text.len();
        self.inline.text.push_str(text);
        if !self.chips {
            return;
        }
        let t = self.t;
        for (range, kind) in chip_spans(text, self.at_start) {
            let background = match kind {
                '/' => t.accent.opacity(0.3),
                '$' => t.success.opacity(0.25),
                '&' => t.warning.opacity(0.25),
                _ => t.bg_selected_muted,
            };
            let range = base + range.start..base + range.end;
            if kind != '@' {
                self.inline.mono.push(range.clone());
            }
            self.inline.highlights.push((
                range,
                HighlightStyle {
                    background_color: Some(background),
                    color: Some(if kind == '@' { t.accent_text } else { t.text }),
                    ..Default::default()
                },
            ));
        }
        if !text.trim().is_empty() {
            self.at_start = false;
        }
    }

    fn push(&mut self, el: impl IntoElement, mt: f32, mb: f32) {
        if let Some((_, blocks)) = self.stack.last_mut() {
            blocks.push(Block {
                el: el.into_any_element(),
                mt,
                mb,
            });
        }
    }

    /// Text outside any paragraph, as a tight list item holds, becomes one.
    fn flush_paragraph(&mut self) {
        if self.inline.is_empty() {
            self.inline = Inline::default();
        } else {
            let text = self.take_inline();
            self.push(div().child(text), 0., 8.);
        }
    }

    fn event(&mut self, event: Event) {
        let t = self.t;
        if let Some((_, code)) = &mut self.code {
            match event {
                Event::Text(text) => code.push_str(&text),
                Event::End(TagEnd::CodeBlock) => self.end_code(),
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => self.flush_paragraph(),
                Tag::Heading { level, .. } => {
                    self.flush_paragraph();
                    self.heading = Some(level);
                }
                Tag::BlockQuote(_) => {
                    self.flush_paragraph();
                    self.stack.push((Frame::Quote, Vec::new()));
                }
                Tag::CodeBlock(kind) => {
                    self.flush_paragraph();
                    let lang = match kind {
                        CodeBlockKind::Fenced(info) => {
                            info.split_whitespace().next().unwrap_or("").to_string()
                        }
                        CodeBlockKind::Indented => String::new(),
                    };
                    self.code = Some((lang, String::new()));
                }
                Tag::List(start) => {
                    self.flush_paragraph();
                    self.stack.push((
                        Frame::List {
                            start,
                            items: Vec::new(),
                        },
                        Vec::new(),
                    ));
                }
                Tag::Item => self.stack.push((Frame::Item, Vec::new())),
                Tag::Table(_) => {
                    self.flush_paragraph();
                    self.rows.clear();
                    self.stack.push((Frame::Table, Vec::new()));
                }
                Tag::TableHead => self.stack.push((Frame::Row { head: true }, Vec::new())),
                Tag::TableRow => self.stack.push((Frame::Row { head: false }, Vec::new())),
                Tag::TableCell => self.inline = Inline::default(),
                Tag::Emphasis => self.inline.open(HighlightStyle {
                    font_style: Some(FontStyle::Italic),
                    ..Default::default()
                }),
                Tag::Strong => self.inline.open(HighlightStyle {
                    font_weight: Some(FontWeight::SEMIBOLD),
                    ..Default::default()
                }),
                Tag::Strikethrough => self.inline.open(HighlightStyle {
                    strikethrough: Some(StrikethroughStyle {
                        thickness: px(1.),
                        color: None,
                    }),
                    ..Default::default()
                }),
                Tag::Link { .. } => self.inline.open(HighlightStyle {
                    color: Some(t.accent_text),
                    underline: Some(UnderlineStyle {
                        thickness: px(1.),
                        color: Some(t.accent_text),
                        wavy: false,
                    }),
                    ..Default::default()
                }),
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Paragraph => self.flush_paragraph(),
                TagEnd::Heading(_) => self.end_heading(),
                TagEnd::BlockQuote(_) => {
                    self.flush_paragraph();
                    if let Some((_, blocks)) = self.stack.pop() {
                        let quote = div()
                            // The 40px at each side a web page gives a quote of its own.
                            .mx(px(40.))
                            .border_l_2()
                            .border_color(t.border)
                            .pl(px(12.))
                            .text_color(t.text_muted)
                            .child(stack_blocks(blocks));
                        self.push(quote, 8., 8.);
                    }
                }
                TagEnd::Item => {
                    self.flush_paragraph();
                    if let Some((_, blocks)) = self.stack.pop()
                        && let Some((Frame::List { items, .. }, _)) = self.stack.last_mut()
                    {
                        items.push(stack_blocks(blocks));
                    }
                }
                TagEnd::List(_) => {
                    if let Some((Frame::List { start, items }, _)) = self.stack.pop() {
                        self.end_list(start, items);
                    }
                }
                TagEnd::TableCell => {
                    let text = self.take_inline();
                    self.cells.push(text.into_any_element());
                }
                TagEnd::TableHead | TagEnd::TableRow => {
                    if let Some((Frame::Row { head }, _)) = self.stack.pop() {
                        let cells = std::mem::take(&mut self.cells);
                        let row = cells
                            .into_iter()
                            .map(|cell| {
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .border_1()
                                    .border_color(t.border)
                                    .px(px(8.))
                                    .py(px(4.))
                                    .when(head, |c| c.font_weight(FontWeight::SEMIBOLD))
                                    .child(cell)
                                    .into_any_element()
                            })
                            .collect();
                        self.rows.push(row);
                    }
                }
                TagEnd::Table => {
                    self.stack.pop();
                    let rows = std::mem::take(&mut self.rows);
                    let table = div()
                        .flex()
                        .flex_col()
                        .text_size(px(FS_SM))
                        .children(rows.into_iter().map(|cells| div().flex().children(cells)));
                    self.push(table, 8., 8.);
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                    self.inline.close();
                }
                _ => {}
            },
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                let start = self.inline.text.len();
                self.inline.text.push_str(&code);
                let range = start..self.inline.text.len();
                self.inline.mono.push(range.clone());
                self.inline.highlights.push((
                    range,
                    HighlightStyle {
                        background_color: Some(t.bg_selected_muted),
                        ..Default::default()
                    },
                ));
            }
            Event::SoftBreak => self.inline.text.push(' '),
            Event::HardBreak => self.inline.text.push('\n'),
            Event::Rule => {
                self.flush_paragraph();
                self.push(div().h(px(1.)).bg(t.border), 12., 12.);
            }
            Event::TaskListMarker(done) => {
                self.inline.text.push_str(if done { "☑ " } else { "☐ " });
            }
            Event::Html(html) | Event::InlineHtml(html) => self.inline.text.push_str(&html),
            _ => {}
        }
    }

    fn end_heading(&mut self) {
        let level = self.heading.take();
        let text = self.take_inline();
        let size = if level == Some(HeadingLevel::H1) {
            FS_LG
        } else {
            FS_MD
        };
        self.push(
            div()
                .text_size(px(size))
                .font_weight(FontWeight::SEMIBOLD)
                .child(text),
            12.,
            6.,
        );
    }

    fn end_list(&mut self, start: Option<u64>, items: Vec<Div>) {
        let t = self.t;
        let list = div()
            .flex()
            .flex_col()
            .pl(px(22.))
            .children(items.into_iter().enumerate().map(|(i, item)| {
                let marker = match start {
                    Some(n) => div()
                        .absolute()
                        .left(px(-22.))
                        .w(px(18.))
                        .flex()
                        .justify_end()
                        .child(format!("{}.", n + i as u64)),
                    None => div()
                        .absolute()
                        .left(px(-15.))
                        .child("•")
                        .text_color(t.text),
                };
                div().relative().min_w_0().child(marker).child(item)
            }));
        self.push(list, 0., 8.);
    }

    fn end_code(&mut self) {
        let Some((lang, code)) = self.code.take() else {
            return;
        };
        // A choices block is shown as a panel above the composer, not as code.
        if lang == "choices" {
            return;
        }
        let t = self.t;
        let mut code = code;
        if code.ends_with('\n') {
            code.pop();
        }
        let code = SharedString::from(code);
        self.code_blocks += 1;
        let id = SharedString::from(format!("{}-code-{}", self.key, self.code_blocks));
        let (hover, fg) = (t.bg_hover, t.text);
        let copy = code.clone();
        let text = highlighted(code, &lang, t);
        let block = div()
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(t.border)
                    .py(px(2.))
                    .pr(px(4.))
                    .pl(px(10.))
                    .text_size(px(FS_XS))
                    .text_color(t.text_subtle)
                    .child(div().min_w_0().truncate().child(if lang.is_empty() {
                        "text".into()
                    } else {
                        lang
                    }))
                    .child(
                        button(id, "Copy code")
                            .h(px(24.))
                            .px(px(6.))
                            .rounded(px(RADIUS_SM))
                            .text_color(t.text_muted)
                            .hover(move |s| s.bg(hover).text_color(fg))
                            .child(icon("copy", 13.))
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.to_string()));
                            }),
                    ),
            )
            .child(
                div()
                    .px(px(12.))
                    .py(px(10.))
                    .text_size(px(FS_SM))
                    .line_height(relative(1.5))
                    .font_family(FONT_MONO)
                    .child(text),
            );
        self.push(block, 8., 8.);
    }
}

/// Where a run of text holds chips, and the sigil of each: `/goal` as the first word, and each
/// `$skill`, `&agent` and `@path` (one with a `.` or `/`) after a space (`chipPieces`).
fn chip_spans(text: &str, at_start: bool) -> Vec<(Range<usize>, char)> {
    let mut spans = Vec::new();
    let lead = text.len() - text.trim_start().len();
    if at_start && text[lead..].starts_with("/goal") {
        let end = lead + "/goal".len();
        if text[end..].chars().next().is_none_or(char::is_whitespace) {
            spans.push((lead..end, '/'));
        }
    }
    let mut word_start = None;
    for (i, c) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        match (c.is_whitespace(), word_start) {
            (false, None) => word_start = Some(i),
            (true, Some(start)) => {
                word_start = None;
                let word = text[start..i].trim_end_matches(['.', ',', '!', '?', ';', ':', ')']);
                let Some(sigil) = word.chars().next().filter(|s| "$&@".contains(*s)) else {
                    continue;
                };
                let body = &word[1..];
                let valid = match sigil {
                    '$' => body.starts_with(|c: char| c.is_ascii_lowercase()),
                    '&' => body.starts_with(|c: char| c.is_ascii_alphabetic()),
                    _ => body.contains(['.', '/']),
                };
                let body_ok = body
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_:./~".contains(c));
                if valid && body_ok && spans.iter().all(|(r, _)| r.end <= start) {
                    spans.push((start..start + word.len(), sigil));
                }
            }
            _ => {}
        }
    }
    spans
}

/// A code block's text coloured for its language, when one is named and known.
fn highlighted(code: SharedString, lang: &str, t: &Theme) -> StyledText {
    let runs = crate::ui::highlight::highlight_code(&code, lang);
    StyledText::new(code).with_highlights(
        runs.iter()
            .map(|(range, token)| (range.clone(), token.style(t))),
    )
}
