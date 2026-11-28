//! The images a sent message's text holds as Markdown, which show with its attachments instead.

// Rust guideline compliant 2026-10-09

/// The text with the images written into it as `![name](</path>)` taken out; they show with the
/// other attachments.
#[must_use]
pub fn without_image_markdown(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("![") {
        out.push_str(&rest[..start]);
        if let Some(end) = image_end(&rest[start..]) {
            rest = &rest[start + end..];
            // The space on one side of the image goes with it. Other spacing is the user's:
            // indented code, aligned tables.
            if out.ends_with(' ') || out.is_empty() {
                rest = rest.strip_prefix(' ').unwrap_or(rest);
            }
        } else {
            out.push_str("![");
            rest = &rest[start + 2..];
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// The length of the `![label](</absolute/path>)` that `text` starts with.
fn image_end(text: &str) -> Option<usize> {
    let mut chars = text.char_indices().skip(2);
    let mut close = None;
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => _ = chars.next(),
            ']' => {
                close = Some(i);
                break;
            }
            _ => {}
        }
    }
    let path = text[close? + 1..].strip_prefix("(</")?;
    let end = path.find(['<', '>', '\n'])?;
    path[end..]
        .starts_with(">)")
        .then(|| text.len() - path.len() + end + 2)
}
