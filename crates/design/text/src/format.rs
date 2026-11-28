//! How times, sizes, titles and paths are written, so text reads the same everywhere.

// Rust guideline compliant 2026-10-09

/// `title` as a file name, as `slugFileName` makes it.
#[must_use]
pub fn slug_file_name(title: &str, fallback: &str) -> String {
    let lower = title.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit())
        .filter(|w| !w.is_empty())
        .collect();
    let slug = words.join("-");
    format!("{}.md", if slug.is_empty() { fallback } else { &slug })
}

/// A thinking level as the pickers show it.
#[must_use]
pub fn effort_label(effort: &str) -> String {
    match effort {
        "low" => "Low".into(),
        "medium" => "Medium".into(),
        "high" => "High".into(),
        "xhigh" => "Extra high".into(),
        "max" => "Max".into(),
        "ultra" => "Ultra".into(),
        other => display_title(other),
    }
}

/// The first letter capitalised, as the task list shows a title.
#[must_use]
pub fn display_title(title: &str) -> String {
    let mut chars = title.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Milliseconds since the Unix epoch, as the times this formats are kept.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Compact age: "now", "5m", "3h", "2d".
#[must_use]
pub fn format_relative(ts: u64) -> String {
    let minutes = now_ms().saturating_sub(ts) / 60_000;
    match minutes {
        0 => "now".into(),
        1..60 => format!("{minutes}m"),
        _ if minutes / 60 < 24 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 60 / 24),
    }
}

/// "Nov 8, 2026, 10:21 PM", in local time, from seconds since the epoch.
#[must_use]
pub fn format_date(secs: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(secs, 0).single() {
        Some(d) => d.format("%b %-d, %Y, %-I:%M %p").to_string(),
        None => String::new(),
    }
}

/// "9:05 PM", in local time.
#[must_use]
pub fn format_clock(ts: u64) -> String {
    use chrono::{Local, TimeZone, Timelike};
    match Local.timestamp_millis_opt(ts as i64).single() {
        Some(d) => {
            let h = d.hour();
            let twelve = if h % 12 == 0 { 12 } else { h % 12 };
            let half = if h < 12 { "AM" } else { "PM" };
            format!("{twelve}:{:02} {half}", d.minute())
        }
        None => String::new(),
    }
}

/// "42s", "12m", "1.5h", "14h": a duration in a sentence.
#[must_use]
pub fn compact_elapsed(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        return format!("{s}s");
    }
    if s < 3600 {
        return format!("{}m", s / 60);
    }
    let h = s as f64 / 3600.;
    if h < 10. {
        let h = format!("{h:.1}");
        format!("{}h", h.strip_suffix(".0").unwrap_or(&h))
    } else {
        format!("{}h", s / 3600)
    }
}

/// "42s", "1m 05s", "2h 03m".
#[must_use]
pub fn full_elapsed(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    }
}

/// "812", "66.4k", "612k", "1.2M", "2.5B", "1.1T".
#[must_use]
pub fn format_tokens(n: u64) -> String {
    let trim = |s: String| s.strip_suffix(".0").map(str::to_string).unwrap_or(s);
    if n < 1000 {
        n.to_string()
    } else if n < 100_000 {
        trim(format!("{:.1}", n as f64 / 1000.)) + "k"
    } else if n < 1_000_000 {
        format!("{}k", (n as f64 / 1000.).round())
    } else if n < 999_950_000 {
        trim(format!("{:.1}", n as f64 / 1e6)) + "M"
    } else if n < 999_950_000_000 {
        trim(format!("{:.1}", n as f64 / 1e9)) + "B"
    } else {
        trim(format!("{:.1}", n as f64 / 1e12)) + "T"
    }
}

/// A path's last part (`baseName`).
#[must_use]
pub fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// A path's folder, or "" at the top (`dirName`).
#[must_use]
pub fn dir_name(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

/// "in 3h 12m", "in 2d 4h", "in 5m".
#[must_use]
pub fn resets_in(at: u64, now: u64) -> String {
    let mins = (at.saturating_sub(now) as f64 / 60000.).round() as u64;
    let (d, h, m) = (mins / 1440, mins % 1440 / 60, mins % 60);
    if d > 0 {
        format!("in {d}d {h}h")
    } else if h > 0 {
        format!("in {h}h {m}m")
    } else {
        format!("in {m}m")
    }
}
