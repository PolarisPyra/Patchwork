//! An artifact's content as a whole web page the browser draws.
//!
//! The typed artifacts (docs, slides, sheets, designs, whiteboards) get their pages from
//! `typed`.
//! A page and an SVG are drawn as they are, Markdown is turned into HTML here, and code is shown
//! as text. A Mermaid diagram and a React component are drawn by scripts the page loads from
//! public package hosts, so those two need the network. A React component is compiled in the
//! page, and its imports load from esm.sh, sharing one React.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;

use pulldown_cmark::{Options, Parser, html};

use super::typed;
use super::types::ArtifactKind;

/// The styles of the pages Patchwork wraps around content: Markdown, code, SVG and diagrams.
const STYLE: &str = "\
body{margin:0;font:15px/1.6 system-ui,sans-serif;color:#1f2328;background:#fff}\
main{max-width:860px;margin:0 auto;padding:32px 24px}\
main.center{max-width:none;min-height:100vh;box-sizing:border-box;display:grid;place-items:center}\
pre{background:#f6f8fa;padding:12px 16px;border-radius:6px;overflow:auto}\
code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:13px}\
table{border-collapse:collapse}td,th{border:1px solid #d0d7de;padding:6px 10px}\
img,svg{max-width:100%;height:auto}";

/// Mermaid, as an ES module, at a version fixed so a diagram draws the same every time.
const MERMAID: &str = "https://cdn.jsdelivr.net/npm/mermaid@11.4.1/dist/mermaid.esm.min.mjs";

/// Babel, which compiles a component's JSX and TypeScript in the page.
const BABEL: &str = "https://cdn.jsdelivr.net/npm/@babel/standalone@7.26.4/babel.min.js";

/// Tailwind's in-page build, since components are usually styled with its classes.
const TAILWIND: &str = "https://cdn.tailwindcss.com/3.4.16";

/// The React every component and its imports share.
const REACT: &str = "https://esm.sh/react@18.3.1";
const REACT_DOM: &str = "https://esm.sh/react-dom@18.3.1";

/// Compiles the component, points its bare imports at esm.sh, and draws its default export.
/// Shows any error in the page.
const REACT_RUNNER: &str = r#"
const show = (e) => {
  const pre = document.createElement('pre');
  pre.style.cssText = 'color:#b42318;white-space:pre-wrap;padding:16px;margin:0;font:13px ui-monospace,monospace';
  pre.textContent = String((e && e.stack) || e);
  document.body.replaceChildren(pre);
  console.error(e);
};
window.addEventListener('error', (e) => show(e.error || e.message));
try {
  let code = Babel.transform(source, {
    filename: 'artifact.tsx',
    presets: [['react', { runtime: 'automatic' }], ['typescript', { isTSX: true, allExtensions: true }]],
  }).code;
  const shared = (s) => /^react(-dom)?(\/|$)/.test(s) || s.includes(':');
  code = code.replace(/(\bfrom\s*|\bimport\s*\(\s*|\bimport\s+)(["'])([^"'./][^"']*)\2/g,
    (m, before, q, s) => shared(s) ? m : `${before}${q}https://esm.sh/${s}?external=react,react-dom${q}`);
  const module = await import(URL.createObjectURL(new Blob([code], { type: 'text/javascript' })));
  const App = module.default ?? Object.values(module).find((v) => typeof v === 'function');
  if (!App) throw new Error('The component has no default export to draw.');
  const { createElement } = await import('react');
  const { createRoot } = await import('react-dom/client');
  createRoot(document.getElementById('root')).render(createElement(App));
} catch (e) {
  show(e);
}
"#;

/// The page that shows `content` of `kind`, titled `title`.
#[must_use]
pub fn page(kind: ArtifactKind, content: &str, title: &str) -> String {
    let title = escape(title);
    match kind {
        ArtifactKind::Svg => wrapped(&title, "center", content, ""),
        ArtifactKind::Markdown => {
            let mut body = String::new();
            html::push_html(&mut body, Parser::new_ext(content, Options::all()));
            wrapped(&title, "", &body, "")
        }
        ArtifactKind::Code => wrapped(
            &title,
            "",
            &format!("<pre><code>{}</code></pre>", escape(content)),
            "",
        ),
        ArtifactKind::Mermaid => wrapped(
            &title,
            "center",
            &format!(
                "<pre class=\"mermaid\" style=\"background:none\">{}</pre>",
                escape(content)
            ),
            &format!(
                "<script type=\"module\">import mermaid from '{MERMAID}';\
                 mermaid.initialize({{startOnLoad:true}});</script>"
            ),
        ),
        ArtifactKind::React => react(&title, content),
        ArtifactKind::Html | ArtifactKind::Data => based(content),
        ArtifactKind::Doc => typed::doc(&title, content),
        ArtifactKind::Slides => typed::slides(&title, content),
        ArtifactKind::Sheet => typed::sheet(&title, content),
        ArtifactKind::Design => typed::design(&title, content),
        ArtifactKind::Whiteboard => typed::whiteboard(&title, content),
    }
}

/// The base a web page starts from: its controls take the page's font and colour instead of the
/// browser's own. `:where` gives each rule no weight, so any rule of the page's own wins.
const BASE: &str = "<style>:where(button,input,select,textarea){font:inherit;color:inherit}\
:where(button){cursor:pointer}</style>";

/// `content`, a web page or part of one, with [`BASE`] first in its head, never before its
/// doctype, which would put the browser in quirks mode.
fn based(content: &str) -> String {
    let lower = content.to_ascii_lowercase();
    // The end of the first tag named `tag`, so `<head` is not taken for `<header`.
    let after = |tag: &str| {
        lower.match_indices(tag).find_map(|(at, _)| {
            let rest = &lower[at + tag.len()..];
            rest.starts_with(|c: char| c == '>' || c.is_ascii_whitespace())
                .then(|| rest.find('>').map(|end| at + tag.len() + end + 1))
                .flatten()
        })
    };
    let at = after("<head")
        .or_else(|| after("<html"))
        .or_else(|| after("<!doctype"))
        .unwrap_or(0);
    format!("{}{BASE}{}", &content[..at], &content[at..])
}

/// `body` in a page with Patchwork's styles, in a `main` of `class`, followed by `scripts`.
fn wrapped(title: &str, class: &str, body: &str, scripts: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{title}</title><style>{STYLE}</style></head>\
         <body><main class=\"{class}\">{body}</main>{scripts}</body></html>"
    )
}

/// The page that compiles and draws a React component.
fn react(title: &str, source: &str) -> String {
    let imports = serde_json::json!({"imports": {
        "react": REACT,
        "react/": format!("{REACT}/"),
        "react-dom": REACT_DOM,
        "react-dom/": format!("{REACT_DOM}/"),
    }});
    let mut page = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{title}</title><script src=\"{TAILWIND}\"></script>\
         <script src=\"{BABEL}\"></script>\
         <script type=\"importmap\">{}</script></head><body><div id=\"root\"></div>\
         <script type=\"module\">const source = ",
        script_safe(&imports.to_string()),
    );
    let _ = write!(
        page,
        "{};{REACT_RUNNER}</script></body></html>",
        script_safe(&serde_json::Value::from(source).to_string())
    );
    page
}

/// JSON that cannot end the script element it is written in.
fn script_safe(json: &str) -> String {
    json.replace("</", "<\\/").replace("<!--", "<\\!--")
}

/// `text` with the characters HTML gives meaning escaped.
pub(super) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
