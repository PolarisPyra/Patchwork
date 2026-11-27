//! The pages of the typed quilts: a doc on paper, a slide deck, a sheet's grid, a design's
//! frame on a canvas, and a whiteboard that pans and zooms. A data quilt is a page drawn as
//! it is.

// Rust guideline compliant 2026-10-09

use std::fmt::Write as _;

use pulldown_cmark::{Options, Parser, html};

use super::render::escape;
use super::sheet::{cells, column_name};

/// A doc: Markdown set on a page of paper.
const DOC_STYLE: &str = "\
html{background:#eceef1}body{margin:0;padding:40px 16px;font:16px/1.65 Georgia,'Iowan Old Style',serif;color:#1f2328}\
article{max-width:816px;margin:0 auto;background:#fff;padding:72px 88px;box-sizing:border-box;\
box-shadow:0 1px 3px #0000001f,0 8px 24px #00000014;min-height:1056px}\
h1,h2,h3,h4{font-family:system-ui,sans-serif;line-height:1.25;margin:1.6em 0 .6em}h1{font-size:2em;margin-top:0}\
pre{background:#f6f8fa;padding:12px 16px;border-radius:6px;overflow:auto}\
code{font:13px ui-monospace,SFMono-Regular,Menlo,monospace}\
table{border-collapse:collapse;margin:1em 0}td,th{border:1px solid #d0d7de;padding:6px 10px}\
blockquote{margin:1em 0;padding-left:1em;border-left:3px solid #d0d7de;color:#57606a}\
img,svg{max-width:100%;height:auto}\
@media (max-width:700px){article{padding:32px 24px}}\
@media print{html{background:none}body{padding:0}article{box-shadow:none;padding:0;max-width:none}}";

/// A deck: each top-level `section` is a 1280 by 720 slide, scaled to fit the window.
const SLIDES_STYLE: &str = "\
html,body{margin:0;height:100%;overflow:hidden;background:#18181b}\
body>*:not(#pw-deck):not(#pw-bar):not(script):not(style){display:none}\
#pw-deck{position:fixed;inset:0;font:28px/1.4 system-ui,sans-serif;color:#1f2328}\
.pw-slide{position:absolute!important;left:50%;top:50%;width:1280px;height:720px;margin:0!important;\
box-sizing:border-box;overflow:hidden;background:#fff;transform-origin:center}\
.pw-slide:not(.pw-on){visibility:hidden}\
#pw-bar{position:fixed;right:12px;bottom:10px;display:flex;gap:4px;align-items:center;\
font:13px system-ui,sans-serif;color:#d4d4d8;opacity:.35;transition:opacity .2s}\
#pw-bar:hover,#pw-bar:focus-within{opacity:1}\
#pw-bar button{all:unset;cursor:pointer;padding:4px 9px;border-radius:6px;background:#3f3f46}\
#pw-bar button:focus-visible{outline:2px solid #60a5fa}\
:fullscreen #pw-bar{display:none}";

/// Moves the deck's slides into place and steps through them with keys, clicks and the bar.
const SLIDES_SCRIPT: &str = r"
const slides = [...document.querySelectorAll('section')].filter((s) => !s.parentElement.closest('section'));
const deck = document.createElement('div');
deck.id = 'pw-deck';
slides.forEach((s) => { s.classList.add('pw-slide'); deck.appendChild(s); });
document.body.prepend(deck);
const bar = document.getElementById('pw-bar');
const count = document.getElementById('pw-count');
let at = Math.min(Math.max(parseInt(location.hash.slice(1), 10) - 1 || 0, 0), Math.max(slides.length - 1, 0));
const fit = () => {
  const s = Math.min(innerWidth / 1280, innerHeight / 720);
  slides.forEach((el) => { el.style.transform = `translate(-50%,-50%) scale(${s})`; });
};
const go = (n) => {
  at = Math.min(Math.max(n, 0), slides.length - 1);
  slides.forEach((el, i) => el.classList.toggle('pw-on', i === at));
  count.textContent = slides.length ? `${at + 1} / ${slides.length}` : 'No slides';
  history.replaceState(null, '', `#${at + 1}`);
};
const present = () => document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen();
addEventListener('resize', fit);
addEventListener('keydown', (e) => {
  if (e.target.closest && e.target.closest('input,textarea,select,[contenteditable]')) return;
  const k = e.key;
  if (['ArrowRight', 'ArrowDown', 'PageDown', ' ', 'Enter'].includes(k)) go(at + 1);
  else if (['ArrowLeft', 'ArrowUp', 'PageUp', 'Backspace'].includes(k)) go(at - 1);
  else if (k === 'Home') go(0);
  else if (k === 'End') go(slides.length - 1);
  else if (k === 'f' || k === 'F') present();
  else return;
  e.preventDefault();
});
deck.addEventListener('click', (e) => {
  if (e.target.closest('a,button,input,textarea,select,label,[contenteditable]')) return;
  go(e.clientX < innerWidth / 3 ? at - 1 : at + 1);
});
document.getElementById('pw-prev').onclick = () => go(at - 1);
document.getElementById('pw-next').onclick = () => go(at + 1);
document.getElementById('pw-present').onclick = present;
fit();
go(at);
";

/// The deck's bar: back, the slide shown, forward, and present.
const SLIDES_BAR: &str = "<div id=\"pw-bar\" role=\"toolbar\" aria-label=\"Slides\">\
<button id=\"pw-prev\" aria-label=\"Previous slide\">&#8592;</button>\
<span id=\"pw-count\" role=\"status\"></span>\
<button id=\"pw-next\" aria-label=\"Next slide\">&#8594;</button>\
<button id=\"pw-present\" aria-label=\"Present\">Present</button></div>";

/// A sheet: the grid with its column letters and row numbers fixed while it scrolls.
const SHEET_STYLE: &str = "\
html,body{margin:0;height:100%;background:#fff}\
.pw-sheet{height:100%;overflow:auto;font:13px/1.4 system-ui,sans-serif;color:#1f2328}\
table{border-collapse:separate;border-spacing:0}\
th,td{border-right:1px solid #e1e4e8;border-bottom:1px solid #e1e4e8;padding:4px 8px;white-space:nowrap;\
min-width:72px;max-width:360px;overflow:hidden;text-overflow:ellipsis;height:20px}\
thead th{position:sticky;top:0;z-index:2;background:#f6f8fa;color:#57606a;font-weight:500;text-align:center}\
tbody th{position:sticky;left:0;z-index:1;background:#f6f8fa;color:#57606a;font-weight:500;min-width:36px;text-align:right}\
thead th:first-child{left:0;z-index:3}\
td.n{text-align:right;font-variant-numeric:tabular-nums}td.f{color:#0b57d0}\
td.e{color:#b42318}tbody tr:first-child td{font-weight:600;background:#fafbfc}";

/// A design: its frame on a neutral canvas, fit to the window, zoomed with ctrl and the wheel.
const DESIGN_STYLE: &str = "\
html,body{margin:0;height:100%;overflow:hidden;background:#e5e5e5;font:12px system-ui,sans-serif}\
#pw-canvas{position:fixed;inset:0;overflow:hidden}\
#pw-frame{position:absolute;left:50%;top:50%;transform-origin:center;background:#fff;\
box-shadow:0 1px 3px #0000002e,0 12px 32px #0000001f}\
#pw-frame iframe{border:0;width:100%;height:100%;display:block}\
#pw-label{position:absolute;left:0;bottom:100%;padding-bottom:6px;color:#52525b;white-space:nowrap;\
transform-origin:bottom left}\
#pw-zoom{position:fixed;right:12px;bottom:10px;color:#52525b}";

/// Fits the frame, and zooms it with ctrl and the wheel; 0 fits it again.
const DESIGN_SCRIPT: &str = r"
const frame = document.getElementById('pw-frame');
const label = document.getElementById('pw-label');
const shown = document.getElementById('pw-zoom');
const [w, h] = [frame.offsetWidth, frame.offsetHeight];
let zoom = 1, fitted = true;
const fit = () => Math.min((innerWidth - 64) / w, (innerHeight - 72) / h, 1);
const draw = () => {
  if (fitted) zoom = fit();
  frame.style.transform = `translate(-50%,-50%) scale(${zoom})`;
  label.style.transform = `scale(${1 / zoom})`;
  shown.textContent = `${Math.round(zoom * 100)}%`;
};
addEventListener('resize', draw);
addEventListener('wheel', (e) => {
  if (!e.ctrlKey && !e.metaKey) return;
  e.preventDefault();
  fitted = false;
  zoom = Math.min(Math.max(zoom * Math.exp(-e.deltaY / 300), 0.05), 8);
  draw();
}, { passive: false });
addEventListener('keydown', (e) => {
  if (e.key === '0') { fitted = true; draw(); }
  else if (e.key === '=' || e.key === '+') { fitted = false; zoom = Math.min(zoom * 1.25, 8); draw(); }
  else if (e.key === '-') { fitted = false; zoom = Math.max(zoom / 1.25, 0.05); draw(); }
});
draw();
";

/// A whiteboard: the drawing on a dotted board.
const BOARD_STYLE: &str = "\
html,body{margin:0;height:100%;overflow:hidden;background:#fafaf9;cursor:grab}\
body.pw-drag{cursor:grabbing}\
#pw-board{position:fixed;inset:0;background-image:radial-gradient(#d6d3d1 1px,transparent 1px)}\
#pw-ink{position:absolute;left:0;top:0;transform-origin:0 0}\
#pw-ink>svg{display:block;overflow:visible}\
#pw-zoom{position:fixed;right:12px;bottom:10px;font:12px system-ui,sans-serif;color:#78716c}";

/// Fits the drawing, pans it by dragging, and zooms it at the pointer with the wheel.
const BOARD_SCRIPT: &str = r"
const board = document.getElementById('pw-board');
const ink = document.getElementById('pw-ink');
const svg = ink.querySelector('svg');
const shown = document.getElementById('pw-zoom');
let x = 0, y = 0, zoom = 1;
const draw = () => {
  ink.style.transform = `translate(${x}px,${y}px) scale(${zoom})`;
  const step = 24 * zoom;
  board.style.backgroundSize = `${step}px ${step}px`;
  board.style.backgroundPosition = `${x}px ${y}px`;
  shown.textContent = `${Math.round(zoom * 100)}%`;
};
const fit = () => {
  let box = { x: 0, y: 0, width: ink.offsetWidth, height: ink.offsetHeight };
  try { const b = svg && svg.getBBox(); if (b && b.width && b.height) box = b; } catch (_) {}
  const r = svg ? svg.getBoundingClientRect() : { left: 0, top: 0 };
  const vb = svg && svg.viewBox && svg.viewBox.baseVal;
  const sx = vb && vb.width ? r.width / vb.width : 1;
  const left = (box.x - (vb ? vb.x : 0)) * sx, top = (box.y - (vb ? vb.y : 0)) * sx;
  const width = box.width * sx, height = box.height * sx;
  zoom = Math.min((innerWidth - 80) / width, (innerHeight - 80) / height, 2);
  x = (innerWidth - width * zoom) / 2 - left * zoom;
  y = (innerHeight - height * zoom) / 2 - top * zoom;
  draw();
};
let drag = null;
addEventListener('pointerdown', (e) => {
  if (e.button !== 0 && e.button !== 1) return;
  drag = { px: e.clientX, py: e.clientY, x, y };
  document.body.classList.add('pw-drag');
});
addEventListener('pointermove', (e) => {
  if (!drag) return;
  x = drag.x + e.clientX - drag.px;
  y = drag.y + e.clientY - drag.py;
  draw();
});
addEventListener('pointerup', () => { drag = null; document.body.classList.remove('pw-drag'); });
addEventListener('wheel', (e) => {
  e.preventDefault();
  if (!e.ctrlKey && !e.metaKey && Math.abs(e.deltaX) > 0) { x -= e.deltaX; y -= e.deltaY; draw(); return; }
  const next = Math.min(Math.max(zoom * Math.exp(-e.deltaY / 300), 0.05), 16);
  x = e.clientX - (e.clientX - x) * (next / zoom);
  y = e.clientY - (e.clientY - y) * (next / zoom);
  zoom = next;
  draw();
}, { passive: false });
addEventListener('keydown', (e) => {
  const pan = 60;
  if (e.key === '0') fit();
  else if (e.key === 'ArrowLeft') { x += pan; draw(); }
  else if (e.key === 'ArrowRight') { x -= pan; draw(); }
  else if (e.key === 'ArrowUp') { y += pan; draw(); }
  else if (e.key === 'ArrowDown') { y -= pan; draw(); }
});
fit();
";

/// A design frame's size when its content names none.
const DEFAULT_FRAME: (u32, u32) = (1440, 900);

/// A page around `body`, with `style` in its head and `scripts` after the body.
fn shell(title: &str, style: &str, body: &str, script: &str) -> String {
    let mut page = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>{title}</title><style>{style}</style></head><body>{body}"
    );
    if !script.is_empty() {
        let _ = write!(page, "<script>{script}</script>");
    }
    page.push_str("</body></html>");
    page
}

/// A doc's Markdown set on paper.
pub(super) fn doc(title: &str, markdown: &str) -> String {
    let mut body = String::from("<article role=\"document\">");
    html::push_html(&mut body, Parser::new_ext(markdown, Options::all()));
    body.push_str("</article>");
    shell(title, DOC_STYLE, &body, "")
}

/// A deck from HTML whose top-level `section`s are its slides. A whole document keeps its head,
/// so its own styles and scripts still apply.
pub(super) fn slides(title: &str, content: &str) -> String {
    let added = format!("<style>{SLIDES_STYLE}</style>");
    let tail = format!("{SLIDES_BAR}<script>{SLIDES_SCRIPT}</script>");
    match (content.find("</head>"), content.rfind("</body>")) {
        (Some(head), Some(body)) if head < body => format!(
            "{}{added}{}{tail}{}",
            &content[..head],
            &content[head..body],
            &content[body..]
        ),
        _ => shell(
            title,
            SLIDES_STYLE,
            &format!("{content}{SLIDES_BAR}"),
            SLIDES_SCRIPT,
        ),
    }
}

/// A sheet's CSV as a grid, with each formula's result and the formula on hover.
pub(super) fn sheet(title: &str, csv: &str) -> String {
    let rows = cells(csv);
    let width = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let mut body = String::from(
        "<div class=\"pw-sheet\"><table role=\"grid\"><thead><tr><th scope=\"col\"></th>",
    );
    for c in 0..width {
        let _ = write!(body, "<th scope=\"col\">{}</th>", column_name(c));
    }
    body.push_str("</tr></thead><tbody>");
    for (r, row) in rows.iter().enumerate() {
        let _ = write!(body, "<tr><th scope=\"row\">{}</th>", r + 1);
        for c in 0..width {
            let Some(cell) = row.get(c) else {
                body.push_str("<td></td>");
                continue;
            };
            let mut class = Vec::new();
            if cell.number {
                class.push("n");
            }
            if let Some(formula) = &cell.formula {
                class.push(if cell.number { "f" } else { "e" });
                let _ = write!(
                    body,
                    "<td class=\"{}\" title=\"{}\">{}</td>",
                    class.join(" "),
                    escape(formula),
                    escape(&cell.text)
                );
            } else {
                let _ = write!(
                    body,
                    "<td class=\"{}\">{}</td>",
                    class.join(" "),
                    escape(&cell.text)
                );
            }
        }
        body.push_str("</tr>");
    }
    body.push_str("</tbody></table></div>");
    shell(title, SHEET_STYLE, &body, "")
}

/// A design's frame, drawn at its size on a canvas. The size is the content's
/// `<meta name="frame" content="390x844">`, an SVG's own size, or 1440 by 900.
pub(super) fn design(title: &str, content: &str) -> String {
    let svg = content.trim_start().starts_with("<svg");
    let (w, h) = frame_size(content).unwrap_or(DEFAULT_FRAME);
    let page = if svg {
        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><style>html,body{{margin:0;\
             height:100%;overflow:hidden}}body>svg{{width:100%;height:100%;display:block}}\
             </style></head><body>{content}</body></html>"
        )
    } else {
        content.to_owned()
    };
    let body = format!(
        "<div id=\"pw-canvas\"><div id=\"pw-frame\" style=\"width:{w}px;height:{h}px;\
         transform:translate(-50%,-50%)\">\
         <div id=\"pw-label\">{title} · {w} × {h}</div>\
         <iframe title=\"{title}\" srcdoc=\"{}\"></iframe></div></div>\
         <div id=\"pw-zoom\" role=\"status\"></div>",
        escape(&page),
    );
    shell(title, DESIGN_STYLE, &body, DESIGN_SCRIPT)
}

/// A whiteboard's SVG on a board that pans and zooms.
pub(super) fn whiteboard(title: &str, svg: &str) -> String {
    let body = format!(
        "<div id=\"pw-board\" role=\"img\" aria-label=\"{title}\"><div id=\"pw-ink\">{svg}</div></div>\
         <div id=\"pw-zoom\" role=\"status\"></div>"
    );
    shell(title, BOARD_STYLE, &body, BOARD_SCRIPT)
}

/// The frame size a design names: its frame meta, or the root SVG's width and height, or its
/// view box.
fn frame_size(content: &str) -> Option<(u32, u32)> {
    if let Some(at) = content.find("name=\"frame\"") {
        let rest = &content[at..];
        let value = attribute(rest, "content")?;
        return size(&value);
    }
    let start = content.find("<svg")?;
    let tag = &content[start..start + content[start..].find('>')?];
    if let (Some(w), Some(h)) = (attribute(tag, "width"), attribute(tag, "height")) {
        return size(&format!("{w}x{h}"));
    }
    let view = attribute(tag, "viewBox")?;
    let parts: Vec<&str> = view.split_whitespace().collect();
    size(&format!("{}x{}", parts.get(2)?, parts.get(3)?))
}

/// A size written `WxH` in pixels, between 1 and 20000 each way.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "checked to be between 1 and 20000 first"
)]
fn size(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.split_once('x')?;
    let n = |s: &str| s.trim().trim_end_matches("px").parse::<f64>().ok();
    let (w, h) = (n(w)?, n(h)?);
    let fits = |v: f64| (1.0..=20000.0).contains(&v);
    (fits(w) && fits(h)).then(|| (w.round() as u32, h.round() as u32))
}

/// The value of the attribute `name` in the start of `tag`.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let end = tag[at..].find('"')?;
    Some(tag[at..at + end].to_owned())
}
