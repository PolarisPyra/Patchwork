//! The `fetch_url` tool: a page fetched and turned into text.

use std::io::Read;
use std::time::Duration;

const MAX_FETCH: usize = 300_000;

pub(super) fn fetch_url(url: &str) -> Result<String, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only http and https URLs can be fetched".into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("User-Agent", "Patchwork")
        .call()
        .map_err(|e| format!("{url}: {e}"))?;
    let status = response.status();
    let html = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.contains("html"));
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_FETCH as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let body = String::from_utf8_lossy(&bytes).into_owned();
    let text = if html { html_text(&body) } else { body };
    if !status.is_success() {
        return Err(format!(
            "{url} answered {status}\n\n{}",
            text.chars().take(2000).collect::<String>()
        ));
    }
    Ok(text)
}

/// The readable text of an HTML page: tags, scripts, and styles removed, blank runs folded.
fn html_text(html: &str) -> String {
    let mut out = String::new();
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while i < html.len() {
        let rest = &lower[i..];
        if rest.starts_with("<script") || rest.starts_with("<style") {
            let close = if rest.starts_with("<script") {
                "</script>"
            } else {
                "</style>"
            };
            i += rest.find(close).map_or(rest.len(), |end| end + close.len());
            continue;
        }
        if rest.starts_with('<') {
            let block = [
                "<p", "<br", "<div", "<li", "<h", "<tr", "</p", "</div", "</li",
            ]
            .iter()
            .any(|t| rest.starts_with(t));
            if block {
                out.push('\n');
            }
            i += rest.find('>').map_or(rest.len(), |end| end + 1);
            continue;
        }
        let next = rest.find('<').unwrap_or(rest.len());
        out.push_str(&html[i..i + next]);
        i += next;
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let mut lines = Vec::new();
    for line in decoded.lines().map(str::trim) {
        if line.is_empty() && lines.last().is_none_or(|l: &&str| l.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    lines.join("\n")
}
