//! Just enough HTTP/1.1 for the server: reading one request, writing one answer, the files of
//! the web app, and the page a link from a QR code opens. Every connection carries one request
//! and is closed after its answer, so nothing here keeps connections alive.

// Rust guideline compliant 2026-10-09

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

/// The most a request's line and headers may hold; real ones are well under this.
const MAX_HEAD: usize = 16 * 1024;

/// The most a request's body may hold: a message with a pasted log fits, a runaway upload does
/// not.
pub(crate) const MAX_BODY: usize = 4 * 1024 * 1024;

/// One request as it came in.
#[derive(Debug, Default)]
pub(crate) struct Request {
    pub method: String,
    /// The path without its query, still percent-encoded.
    pub path: String,
    /// The query's pairs, decoded.
    pub query: Vec<(String, String)>,
    /// The headers, names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Bytes read past the head that are not a body, such as a WebSocket's first frame.
    pub rest: Vec<u8>,
}

impl Request {
    /// The value of the header `name` (lower case), when it came.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The value of the query parameter `name`, when it came.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// The token the request came in with: `Authorization: Bearer`, else `?token=`.
    pub fn token(&self) -> Option<&str> {
        self.header("authorization")
            .and_then(|h| h.strip_prefix("Bearer "))
            .or_else(|| self.param("token"))
            .map(str::trim)
            .filter(|t| !t.is_empty())
    }

    /// Whether this asks to become a WebSocket.
    pub fn is_upgrade(&self) -> bool {
        self.header("upgrade")
            .is_some_and(|u| u.eq_ignore_ascii_case("websocket"))
    }
}

/// Why a request could not be read.
#[derive(Debug)]
pub(crate) enum Bad {
    /// The connection closed or timed out before a whole request came.
    Gone,
    /// What came is not a request this server takes; the status to answer with.
    Status(u16, &'static str),
}

impl From<io::Error> for Bad {
    fn from(_: io::Error) -> Self {
        Self::Gone
    }
}

/// Reads one request from `stream`.
pub(crate) fn read_request(stream: &mut TcpStream) -> Result<Request, Bad> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = find(&buf, b"\r\n\r\n") {
            break at;
        }
        if buf.len() > MAX_HEAD {
            return Err(Bad::Status(431, "Request Header Fields Too Large"));
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err(Bad::Gone);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end])
        .map_err(|_not_utf8| Bad::Status(400, "Bad Request"))?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split(' ');
    let (Some(method), Some(target)) = (first.next(), first.next()) else {
        return Err(Bad::Status(400, "Bad Request"));
    };
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut request = Request {
        method: method.to_string(),
        path: path.to_string(),
        query: url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect(),
        headers,
        ..Request::default()
    };
    let mut rest = buf.split_off(head_end + 4);
    let length = match request.header("content-length") {
        None => 0,
        Some(n) => n
            .parse::<usize>()
            .map_err(|_not_a_number| Bad::Status(400, "Bad Request"))?,
    };
    if length > MAX_BODY {
        return Err(Bad::Status(413, "Content Too Large"));
    }
    if length > 0 {
        while rest.len() < length {
            let n = stream.read(&mut chunk)?;
            if n == 0 {
                return Err(Bad::Gone);
            }
            rest.extend_from_slice(&chunk[..n]);
        }
        request.rest = rest.split_off(length);
        request.body = rest;
    } else {
        request.rest = rest;
    }
    Ok(request)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Writes a whole answer and lets the connection close.
pub(crate) fn respond(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) {
    let mut out = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n",
        body.len()
    );
    for (name, value) in headers {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    // A peer that left before its answer needs nothing more.
    let _ = stream
        .write_all(out.as_bytes())
        .and_then(|()| stream.write_all(body))
        .and_then(|()| stream.flush());
}

/// Answers with JSON.
pub(crate) fn respond_json(stream: &mut TcpStream, status: u16, value: &impl serde::Serialize) {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        _ => "Error",
    };
    respond(
        stream,
        status,
        reason,
        &[("Content-Type", "application/json")],
        serde_json::to_string(value).unwrap_or_default().as_bytes(),
    );
}

/// Why a web file is not served.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// The path reaches outside the web folder.
    Outside,
    /// Neither the file nor the app's `index.html` is there.
    Missing,
}

/// Decodes `%xx` escapes in one path segment; `None` when one is malformed or the result is not
/// UTF-8.
fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The file under `root` that the request path `path` names. A path naming no file gets the
/// app's `index.html`, so the web app's own routes load it.
///
/// A path is refused when any segment, once decoded, is `..` or holds a separator, drive colon or
/// NUL, and when the file it reaches, symbolic links followed, is not under `root`.
pub(crate) fn web_file(root: &Path, path: &str) -> Result<PathBuf, Refused> {
    let root = root.canonicalize().map_err(|_gone| Refused::Missing)?;
    let mut file = root.clone();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        let segment = percent_decode(segment).ok_or(Refused::Outside)?;
        if segment == ".."
            || segment
                .chars()
                .any(|c| matches!(c, '/' | '\\' | ':' | '\0'))
        {
            return Err(Refused::Outside);
        }
        if segment != "." {
            file.push(segment);
        }
    }
    match file.canonicalize() {
        Ok(found) if !found.starts_with(&root) => Err(Refused::Outside),
        Ok(found) if found.is_file() => Ok(found),
        _ => {
            let index = root.join("index.html");
            if index.is_file() {
                Ok(index)
            } else {
                Err(Refused::Missing)
            }
        }
    }
}

/// The content type a web file is sent with.
pub(crate) fn mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// `text` made safe inside HTML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The headers every pairing page goes with: no script, no frame around it, nothing cached, and
/// no address sent on from a link in it, since the address can hold a code.
pub(crate) const PAGE_HEADERS: [(&str, &str); 5] = [
    ("Content-Type", "text/html; charset=utf-8"),
    (
        "Content-Security-Policy",
        "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'",
    ),
    ("Cache-Control", "no-store"),
    ("Referrer-Policy", "no-referrer"),
    ("X-Frame-Options", "DENY"),
];

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title}</title><style>\
         body{{font:16px system-ui,sans-serif;max-width:26rem;margin:4rem auto;padding:0 1.25rem;\
         color:#1f2328;background:#fff}}\
         @media (prefers-color-scheme:dark){{body{{color:#e6e6e6;background:#161616}}\
         input{{background:#222;color:#e6e6e6;border-color:#444}}}}\
         h1{{font-size:1.25rem}}label{{display:block;margin:1rem 0 .25rem}}\
         input{{width:100%;box-sizing:border-box;padding:.6rem;font:inherit;border:1px solid #ccc;\
         border-radius:8px}}button{{margin-top:1.25rem;padding:.6rem 1rem;font:inherit;\
         border-radius:8px;border:0;background:#2f6feb;color:#fff}}\
         code{{display:block;word-break:break-all;padding:.75rem;border-radius:8px;\
         background:rgba(127,127,127,.15)}}.error{{color:#d1242f}}\
         </style></head><body><h1>{title}</h1>{body}</body></html>",
        title = escape(title),
    )
}

/// The page a pairing link opens. It asks before pairing rather than pairing on sight, so a
/// link preview or prefetch cannot use up the code.
pub(crate) fn pair_form(code: Option<&str>, error: Option<&str>) -> String {
    let error = error
        .map(|e| format!("<p class=\"error\">{}</p>", escape(e)))
        .unwrap_or_default();
    let code_field = match code {
        Some(code) => format!(
            "<p>Pair this device with Patchwork using code <strong>{0}</strong>.</p>\
             <input type=\"hidden\" name=\"code\" value=\"{0}\">",
            escape(code)
        ),
        None => "<label for=\"code\">Pairing code</label>\
                 <input id=\"code\" name=\"code\" autocomplete=\"off\" placeholder=\"K7M-QX2\" required>"
            .to_string(),
    };
    page(
        "Pair with Patchwork",
        &format!(
            "{error}<form method=\"post\" action=\"/pair\">{code_field}\
             <label for=\"name\">Name this device</label>\
             <input id=\"name\" name=\"name\" maxlength=\"60\" placeholder=\"My phone\">\
             <button type=\"submit\">Pair</button></form>"
        ),
    )
}

/// The page shown once a device is paired and there is no web app to go on to: the token, to
/// paste into a client.
pub(crate) fn paired_page(name: &str, token: &str) -> String {
    page(
        "Paired",
        &format!(
            "<p>{} is paired. Its token is below; keep it private, as anyone with it can use \
             Patchwork as this device. Revoke it in Patchwork under Settings → Devices.</p>\
             <code>{}</code>",
            escape(name),
            escape(token)
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_escapes_and_refuses_broken_ones() {
        assert_eq!(percent_decode("a%20b").as_deref(), Some("a b"));
        assert_eq!(percent_decode("%2e%2E").as_deref(), Some(".."));
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("%2"), None);
    }

    #[test]
    fn pages_escape_what_they_show() {
        let html = pair_form(Some("<script>"), Some("\"x\""));
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }
}
