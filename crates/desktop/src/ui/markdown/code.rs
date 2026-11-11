//! Code spans and blocks: file chips and highlighting.

// Rust guideline compliant 2026-10-09

use std::ops::Range;
use gpui::{SharedString, StyledText};
use crate::ui::theme::Theme;

/// Where a run of text holds chips, and the sigil of each: `/goal` as the first word, and each
/// `$skill`, `&agent` and `@path` (one with a `.` or `/`) after a space (`chipPieces`).
pub(super) fn chip_spans(text: &str, at_start: bool) -> Vec<(Range<usize>, char)> {
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
pub(super) fn highlighted(code: SharedString, lang: &str, t: &Theme) -> StyledText {
    let runs = crate::ui::highlight::highlight_code(&code, lang);
    StyledText::new(code).with_highlights(
        runs.iter()
            .map(|(range, token)| (range.clone(), token.style(t))),
    )
}
