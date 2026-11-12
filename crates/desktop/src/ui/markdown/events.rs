//! Reading the parser's events into blocks: text, headings, lists, tables and code.

// Rust guideline compliant 2026-10-09

use super::code::highlighted;
use super::render::stack_blocks;
use super::inline::Frame;
use super::inline::Inline;
use super::inline::Block;
use super::code::chip_spans;
use super::inline::find_in;
use super::render::LIST_INDENT;
use super::render::Markdown;
use gpui::{
    ClipboardItem, Div, FontStyle, FontWeight, HighlightStyle, SharedString, StrikethroughStyle,
    StyledText, UnderlineStyle, div, prelude::*, px, relative,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Tag, TagEnd};
use crate::ui::theme::{FONT_MONO, FS_LG, FS_MD, FS_SM, FS_XS, RADIUS, RADIUS_SM};
use crate::ui::selection::{Selectable, selectable, text_id};
use crate::ui::{button, icon};

impl Markdown<'_> {
    /// The open run as text, with find's matches marked: the current one solid, the rest faint.
    /// `text` as one a drag can mark.
    fn selectable(&mut self, text: StyledText) -> Selectable {
        self.texts += 1;
        let text = selectable(text_id(&self.key, self.texts), text);
        match self.item_prefix.take() {
            Some(prefix) => text.prefix(prefix, px(LIST_INDENT)),
            None => text,
        }
    }

    fn take_inline(&mut self) -> Selectable {
        let chips = std::mem::take(&mut self.inline.chips);
        let Some(find) = self.find else {
            let text = self.inline.take();
            return self.selectable(text).chips(chips);
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
        self.selectable(text).chips(chips)
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
            // The spaces either side are drawn in the monospace font, about twice as wide, so the
            // chip's padding keeps clear of the words beside it. The text itself is unchanged.
            let full = self.inline.text.as_bytes();
            let mut spaces = Vec::new();
            if range.start > 0 && full[range.start - 1] == b' ' {
                spaces.push(range.start - 1..range.start);
            }
            if full.get(range.end) == Some(&b' ') {
                spaces.push(range.end..range.end + 1);
            }
            for space in spaces {
                // A run of its own: a font is only changed for whole runs.
                self.inline
                    .highlights
                    .push((space.clone(), HighlightStyle::default()));
                self.inline.mono.push(space);
            }
            self.inline.chips.push((range.clone(), background));
            self.inline.highlights.push((
                range,
                HighlightStyle {
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
    pub(super) fn flush_paragraph(&mut self) {
        if self.inline.is_empty() {
            self.inline = Inline::default();
        } else {
            let text = self.take_inline();
            self.push(div().child(text), 0., 8.);
        }
    }

    pub(super) fn event(&mut self, event: Event) {
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
                Tag::Item => {
                    // Copied as it is drawn: `- ` or `3. `, indented two spaces per list it is in.
                    let depth = self
                        .stack
                        .iter()
                        .filter(|(frame, _)| matches!(frame, Frame::List { .. }))
                        .count();
                    if let Some((Frame::List { start, items }, _)) = self.stack.last() {
                        let marker = match start {
                            Some(n) => format!("{}. ", n + items.len() as u64),
                            None => "- ".to_string(),
                        };
                        self.item_prefix =
                            Some("  ".repeat(depth.saturating_sub(1)) + marker.as_str());
                    }
                    self.stack.push((Frame::Item, Vec::new()));
                }
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
                    // An empty item's marker stays with it.
                    self.item_prefix = None;
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
                    let text = if self.cells.is_empty() {
                        text
                    } else {
                        text.after_tab()
                    };
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
                // Drawn behind the text like a chip, so a selection over it still shows.
                self.inline.chips.push((range, t.bg_selected_muted));
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
        let list = div().flex().flex_col().pl(px(LIST_INDENT)).children(
            items.into_iter().enumerate().map(|(i, item)| {
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
            }),
        );
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
        let text = self.selectable(text);
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
