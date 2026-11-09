//! The building blocks every view uses: icons, buttons, the spinner, and the web UI's formatting
//! helpers (`apps/web/src/lib/util.ts` and `lib/tokens.ts`), so text reads the same in both.

use std::f32::consts::TAU;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use std::time::Instant;

use gpui::{
    Anchor, AnyElement, App, BoxShadow, Div, ElementId, Hsla, Image, ImageFormat, IntoElement,
    ParentElement, PathBuilder, Role, SharedString, Stateful, StyleRefinement, Styled, Toggled,
    Window, anchored, canvas, deferred, div, img, point, prelude::*, px,
};
use patchwork_server::provider::AgentKind;
use patchwork_server::provider::api::config::ApiProvider;

use crate::theme::{FS_MD, RADIUS, RADIUS_SM, Theme, theme};

/// A Phosphor icon by its kebab-case name, as `@phosphor-icons/react` draws it at `size`.
pub fn icon(name: &str, size: f32) -> Icon {
    mono_svg(SharedString::from(format!("icons/{name}.svg")), size)
}

/// A one-colour SVG asset, `size` square.
fn mono_svg(path: SharedString, size: f32) -> Icon {
    Icon {
        path,
        style: StyleRefinement::default(),
    }
    .size(px(size))
    .flex_none()
}

/// A one-colour SVG drawn in the text colour around it, as `currentColor` is in the web UI, so it
/// follows its button's hover colour. A colour set on the icon itself wins.
#[derive(IntoElement)]
pub struct Icon {
    path: SharedString,
    style: StyleRefinement,
}

impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Icon {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let own = self.style.text.color;
        let path = self.path;
        // The colour around it is known while laying out; painting no longer has it.
        let mut drawn = canvas(
            move |_, window, _| own.unwrap_or_else(|| window.text_style().color),
            move |bounds, color, window: &mut Window, cx| {
                if let Err(e) =
                    window.paint_svg(bounds, path.clone(), None, Default::default(), color, cx)
                {
                    log::warn!("could not draw {path}: {e}");
                }
            },
        );
        *drawn.style() = self.style;
        drawn
    }
}

/// `:focus-visible`: the 2px outline a control shows while the keyboard, not the mouse, focused it.
/// Drawn just inside its edge, as a shadow outside would fill a control with no background.
pub fn focus_ring(s: StyleRefinement) -> StyleRefinement {
    s.shadow(vec![BoxShadow {
        color: crate::theme::border_focus(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(2.),
        inset: true,
    }])
}

/// A focusable button: Enter and Space click it, and assistive tech reads `label`.
pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .tab_index(0)
        .focus_visible(focus_ring)
        .cursor_pointer()
        .flex()
        .items_center()
}

/// `.btn-icon`: a 28px square icon button, muted until hovered.
pub fn icon_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    name: &str,
    size: f32,
    cx: &App,
) -> Stateful<Div> {
    let t = theme(cx);
    let (hover, fg) = (t.bg_hover, t.text);
    button(id, label)
        .size(px(28.))
        .justify_center()
        .rounded(px(RADIUS_SM))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .child(icon(name, size))
}

/// The steady spinner (`components/Spinner.tsx`): a faint ring with a quarter arc running around
/// it once every 0.8s. Every arc's lap counts from the app's start, so all spinners turn in step,
/// however late each one appears. Nothing rotates, so the ring keeps its pixels.
pub fn spinner(size: f32, color: Hsla) -> impl IntoElement {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    let ring = canvas(
        |_, _, _| {},
        move |bounds, _, window: &mut Window, _| {
            window.request_animation_frame();
            let scale = size / 24.;
            let center = bounds.center();
            let radius = 9. * scale;
            let width = px(3. * scale);
            let at = |turn: f32| {
                let angle = turn * TAU;
                point(
                    center.x + px(radius * angle.cos()),
                    center.y + px(radius * angle.sin()),
                )
            };
            let mut ring = PathBuilder::stroke(width);
            ring.move_to(at(0.));
            ring.arc_to(point(px(radius), px(radius)), px(0.), false, true, at(0.5));
            ring.arc_to(point(px(radius), px(radius)), px(0.), false, true, at(0.));
            if let Ok(path) = ring.build() {
                window.paint_path(path, color.opacity(0.25));
            }
            let lap = (start.elapsed().as_secs_f32() / 0.8).fract();
            let (from, to) = (lap, lap + 0.25);
            let mut arc = PathBuilder::stroke(width);
            arc.move_to(at(from));
            arc.arc_to(point(px(radius), px(radius)), px(0.), false, true, at(to));
            if let Ok(path) = arc.build() {
                window.paint_path(path, color);
            }
            // The arc's round caps.
            for end in [from, to] {
                let c = at(end);
                let r = px(1.5 * scale);
                let mut cap = PathBuilder::fill();
                cap.move_to(point(c.x + r, c.y));
                cap.arc_to(point(r, r), px(0.), false, true, point(c.x - r, c.y));
                cap.arc_to(point(r, r), px(0.), false, true, point(c.x + r, c.y));
                if let Ok(path) = cap.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .size(px(size))
    .flex_none();
    div()
        .id("spinner")
        .role(Role::ProgressIndicator)
        .aria_label("Loading")
        .flex_none()
        .child(ring)
}

/// An agent's mark (`components/ProviderLogos.tsx`). A one-colour mark takes the text colour.
/// An API chat shows its provider's preset mark, found from the `provider/model` id.
pub fn agent_logo(kind: AgentKind, model: Option<&str>, size: f32) -> AnyElement {
    let (file, mono) = match kind {
        AgentKind::Claude => ("claude-color.svg", false),
        AgentKind::Codex => ("openai.svg", true),
        AgentKind::Antigravity => ("gemini-color.svg", false),
        AgentKind::OpenCode => ("opencode.svg", true),
        AgentKind::Api => {
            let provider = model.and_then(api_provider_of);
            return provider_logo(
                provider.as_ref().map_or("", |p| p.preset.as_str()),
                provider.as_ref().and_then(|p| p.logo.as_deref()),
                size,
            );
        }
    };
    logo_file(file, mono, size)
}

fn logo_file(file: &str, mono: bool, size: f32) -> AnyElement {
    let path = SharedString::from(format!("logos/{file}"));
    if mono {
        mono_svg(path, size).into_any_element()
    } else {
        img(path).size(px(size)).flex_none().into_any_element()
    }
}

/// An API provider's mark: the picture it was given, else its preset's, else a plug.
pub fn provider_logo(preset: &str, logo: Option<&str>, size: f32) -> AnyElement {
    if let Some(image) = logo.and_then(logo_image) {
        return img(image).size(px(size)).flex_none().into_any_element();
    }
    match preset_logo(preset) {
        Some((file, mono)) => logo_file(file, mono, size),
        None => icon("plugs-connected", size).into_any_element(),
    }
}

/// A `data:image/…;base64,` logo as an image, decoded once.
fn logo_image(url: &str) -> Option<Arc<Image>> {
    static DECODED: OnceLock<Mutex<HashMap<String, Arc<Image>>>> = OnceLock::new();
    let decoded = DECODED.get_or_init(Default::default);
    let mut decoded = decoded.lock().ok()?;
    if let Some(image) = decoded.get(url) {
        return Some(Arc::clone(image));
    }
    let (head, data) = url.strip_prefix("data:")?.split_once(";base64,")?;
    let format = ImageFormat::from_mime_type(head)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    let image = Arc::new(Image::from_bytes(format, bytes));
    decoded.insert(url.to_string(), Arc::clone(&image));
    Some(image)
}

/// The API providers by id, read at the first API model drawn and again after Settings changes
/// them.
fn api_providers() -> Arc<HashMap<String, ApiProvider>> {
    let mut cached = PROVIDERS.lock().unwrap_or_else(|e| e.into_inner());
    Arc::clone(cached.get_or_insert_with(|| {
        Arc::new(
            patchwork_server::provider::api::config::providers()
                .into_iter()
                .map(|p| (p.id.clone(), p))
                .collect(),
        )
    }))
}

static PROVIDERS: Mutex<Option<Arc<HashMap<String, ApiProvider>>>> = Mutex::new(None);

/// Reads the API providers again, after one was added, changed or removed.
pub fn reload_api_providers() {
    *PROVIDERS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The provider of a `provider/model` id (`apiProviderOf`).
fn api_provider_of(model: &str) -> Option<ApiProvider> {
    api_providers().get(model.split('/').next()?).cloned()
}

/// The name of the API provider a `provider/model` id belongs to.
pub fn api_provider_name(model: &str) -> Option<String> {
    api_provider_of(model).map(|p| p.name)
}

/// Where a local provider's model runs: "this computer", or the machine its server is on.
pub fn api_model_place(model: &str) -> Option<String> {
    let provider = api_provider_of(model).filter(|p| p.local)?;
    let rest = provider.base_url.split("://").nth(1)?;
    let host = rest.split(['/', '?']).next()?;
    let host = match host.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name,
        _ => host,
    };
    Some(match host {
        "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0" => "this computer".into(),
        other => other.into(),
    })
}

/// The mark of an API provider preset (`PRESET_LOGOS`).
fn preset_logo(preset: &str) -> Option<(&'static str, bool)> {
    Some(match preset {
        "deepseek" => ("deepseek-color.svg", false),
        "openai" => ("openai.svg", true),
        "gemini" => ("gemini-color.svg", false),
        "xai" => ("grok.svg", true),
        "mistral" => ("mistral-color.svg", false),
        "zai" => ("zai.svg", true),
        "llama" => ("meta-color.svg", false),
        "moonshot" => ("kimi.svg", true),
        "dashscope" => ("qwen-color.svg", false),
        "minimax" => ("minimax-color.svg", false),
        "groq" => ("groq.svg", true),
        "together" => ("together-color.svg", false),
        "fireworks" => ("fireworks-color.svg", false),
        "cerebras" => ("cerebras.svg", true),
        "openrouter" => ("openrouter.svg", true),
        "perplexity" => ("perplexity-color.svg", false),
        "lmstudio" => ("lmstudio.svg", true),
        _ => return None,
    })
}

/// The agents in the order the pickers and the task list show them (`AGENT_KINDS`).
pub const AGENT_KINDS: [AgentKind; 5] = [
    AgentKind::Claude,
    AgentKind::Codex,
    AgentKind::Antigravity,
    AgentKind::OpenCode,
    AgentKind::Api,
];

/// The agent's name, as `AGENT_LABELS` gives it.
pub fn agent_label(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "Claude Code",
        AgentKind::Codex => "Codex",
        AgentKind::Antigravity => "Antigravity CLI",
        AgentKind::OpenCode => "OpenCode CLI",
        AgentKind::Api => "API",
    }
}

/// `title` as a file name, as `slugFileName` makes it.
pub fn slug_file_name(title: &str, fallback: &str) -> String {
    let lower = title.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit())
        .filter(|w| !w.is_empty())
        .collect();
    let slug = words.join("-");
    format!("{}.md", if slug.is_empty() { fallback } else { &slug })
}

/// The first letter capitalised, as the task list shows a title.
pub fn display_title(title: &str) -> String {
    let mut chars = title.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Compact age: "now", "5m", "3h", "2d".
pub fn format_relative(ts: u64) -> String {
    let minutes = now_ms().saturating_sub(ts) / 60_000;
    match minutes {
        0 => "now".into(),
        1..60 => format!("{minutes}m"),
        _ if minutes / 60 < 24 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 60 / 24),
    }
}

/// "9:05 PM", in local time.
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

/// "812", "66.4k", "612k", "1.2m".
pub fn format_tokens(n: u64) -> String {
    let trim = |s: String| s.strip_suffix(".0").map(str::to_string).unwrap_or(s);
    if n < 1000 {
        n.to_string()
    } else if n < 100_000 {
        trim(format!("{:.1}", n as f64 / 1000.)) + "k"
    } else if n < 1_000_000 {
        format!("{}k", (n as f64 / 1000.).round())
    } else {
        trim(format!("{:.1}", n as f64 / 1_000_000.)) + "m"
    }
}

/// The tile a project is drawn with (`ProjectTile`): its first two letters on a colour picked from
/// its name, so each project keeps its colour.
pub fn project_tile(name: &str, size: f32, cx: &App) -> Div {
    let t = theme(cx);
    let palette = [
        t.accent,
        t.syn_string,
        t.syn_property,
        t.syn_keyword,
        t.syn_type,
    ];
    let hash = name
        .chars()
        .fold(0u32, |h, c| h.wrapping_mul(31).wrapping_add(c as u32));
    let initials: String = name
        .trim()
        .chars()
        .take(2)
        .collect::<String>()
        .to_uppercase();
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .bg(palette[hash as usize % palette.len()])
        .text_color(t.text_on_accent)
        .text_size(px(10.))
        .font_weight(gpui::FontWeight::BOLD)
        .child(initials)
}

/// Whether the element `handle` tracks scrolls, as last laid out. When layout changes that, the
/// view draws once more, so a scrollbar and the room kept for it follow content that grew or shrank.
pub fn overflows<V: 'static>(
    handle: &gpui::ScrollHandle,
    window: &mut Window,
    cx: &mut gpui::Context<V>,
) -> bool {
    let seen = (handle.bounds().size.height, handle.max_offset().y);
    let handle = handle.clone();
    cx.on_next_frame(window, move |_, _, cx| {
        if (handle.bounds().size.height, handle.max_offset().y) != seen {
            cx.notify();
        }
    });
    seen.1 > px(0.)
}

/// A scrollbar thumb for the element `handle` tracks, `width` wide with `inset` of clear space
/// around it, as the web UI's `::-webkit-scrollbar-thumb` draws one. None when nothing scrolls.
pub fn scrollbar(handle: &gpui::ScrollHandle, width: f32, inset: f32, color: Hsla) -> Option<Div> {
    let view = f32::from(handle.bounds().size.height);
    let max = f32::from(handle.max_offset().y);
    thumb(
        view,
        max,
        -f32::from(handle.offset().y),
        width,
        inset,
        color,
    )
}

/// `scrollbar` for a `list`, sized from the rows it has measured so far.
pub fn list_scrollbar(state: &gpui::ListState, width: f32, inset: f32, color: Hsla) -> Option<Div> {
    let view = f32::from(state.viewport_bounds().size.height);
    let max = f32::from(state.max_offset_for_scrollbar().y);
    let scrolled = -f32::from(state.scroll_px_offset_for_scrollbar().y);
    thumb(view, max, scrolled, width, inset, color)
}

fn thumb(view: f32, max: f32, scrolled: f32, width: f32, inset: f32, color: Hsla) -> Option<Div> {
    if max <= 0. || view <= 0. {
        return None;
    }
    let scrolled = scrolled.clamp(0., max);
    let thumb = (view * view / (view + max)).max(20.);
    let top = scrolled / max * (view - thumb);
    Some(
        div()
            .absolute()
            .top(px(top))
            .right_0()
            .w(px(width))
            .h(px(thumb))
            .p(px(inset))
            .child(div().size_full().rounded(px(width / 2.)).bg(color)),
    )
}

/// `Dropdown`'s menu: a raised box with a strong border and the popup shadow.
pub fn popup(t: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w(px(200.))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(t.border_strong)
        .bg(t.bg_elevated)
        .p(px(4.))
        .text_size(px(FS_MD))
        .shadow(popup_shadow(t))
}

/// `--shadow-popup`: `0 6px 24px`.
pub fn popup_shadow(t: &Theme) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: t.shadow,
        offset: point(px(0.), px(6.)),
        blur_radius: px(24.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// Lays `content` over the window with its bottom-left corner 4px above the top-left of the
/// element this is a child of, which must be `relative`: a menu that opens up from its chip.
pub fn above(content: impl IntoElement) -> Div {
    div().absolute().top_0().left_0().child(
        deferred(
            anchored()
                .anchor(Anchor::BottomLeft)
                .offset(point(px(0.), px(-4.)))
                .snap_to_window_with_margin(px(8.))
                .child(content),
        )
        .with_priority(1),
    )
}

/// A menu row that picks one value of several (`role="menuitemradio"`): the picked one shaded, and
/// the one under the keyboard highlighted as the pointer would.
pub fn menu_item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    highlighted: bool,
    t: &Theme,
) -> Stateful<Div> {
    let hover = t.bg_hover;
    div()
        .id(id)
        .role(Role::MenuItemRadio)
        .aria_label(label)
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(6.))
        .min_w(px(160.))
        .rounded(px(RADIUS_SM))
        .px(px(8.))
        .py(px(5.))
        .whitespace_nowrap()
        .text_color(t.text)
        .when(checked, |row| row.bg(t.bg_selected_muted))
        .when(highlighted && !checked, |row| row.bg(hover))
        .hover(move |s| s.bg(hover))
}

/// A checkbox and its label (`<label><input type="checkbox">…</label>`); the row toggles it.
pub fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    t: &Theme,
) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .role(Role::CheckBox)
        .aria_label(label.clone())
        .aria_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        })
        .tab_index(0)
        .focus_visible(focus_ring)
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .size(px(13.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .border_1()
                .map(|b| {
                    if checked {
                        b.border_color(t.accent)
                            .bg(t.accent)
                            .text_color(t.text_on_accent)
                            .child(icon("check", 10.))
                    } else {
                        b.border_color(t.border_strong).bg(t.bg_editor)
                    }
                }),
        )
        .child(label)
}

/// What the web UI's `title` attribute shows: a small box with `text` beside the pointer.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView {
    let text = text.into();
    move |_, cx| {
        let text = text.clone();
        cx.new(|_| Tooltip(text)).into()
    }
}

struct Tooltip(SharedString);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let t = theme(cx);
        div()
            .max_w(px(360.))
            .rounded(px(RADIUS_SM))
            .border_1()
            .border_color(t.border_strong)
            .bg(t.bg_elevated)
            .px(px(6.))
            .py(px(2.))
            .text_size(px(crate::theme::FS_SM))
            .line_height(px(16.))
            .text_color(t.text)
            .shadow(popup_shadow(t))
            .children(self.0.split('\n').map(|line| div().child(line.to_string())))
    }
}

/// `color-mix(in srgb, a pct%, b)`.
pub fn mix(a: Hsla, b: Hsla, pct: f32) -> Hsla {
    let (a, b) = (gpui::Rgba::from(a), gpui::Rgba::from(b));
    let p = pct / 100.;
    gpui::Rgba {
        r: a.r * p + b.r * (1. - p),
        g: a.g * p + b.g * (1. - p),
        b: a.b * p + b.b * (1. - p),
        a: a.a * p + b.a * (1. - p),
    }
    .into()
}

/// A path's last part (`baseName`).
pub fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// A path's folder, or "" at the top (`dirName`).
pub fn dir_name(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

/// `.btn`'s variants: the plain button and the coloured ones.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BtnKind {
    Plain,
    Primary,
    Success,
}

/// `.btn`: a 28px button with a border, its label at `--fs-md`. A disabled one is faded and does
/// not react to the pointer; its owner leaves out the click handler.
pub fn btn(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: BtnKind,
    disabled: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (bg, border, fg, hover) = match kind {
        BtnKind::Plain => (t.bg_editor, t.border_strong, t.text, t.bg_hover),
        BtnKind::Primary => (t.accent, t.accent, t.text_on_accent, t.accent_hover),
        BtnKind::Success => (t.success, t.success, t.text_on_accent, darken(t.success)),
    };
    let base = button(id, label)
        .flex_none()
        .h(px(28.))
        .justify_center()
        .gap(px(6.))
        .px(px(14.))
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_size(px(FS_MD))
        .whitespace_nowrap();
    if disabled {
        base.opacity(0.5).cursor_default()
    } else {
        base.hover(move |s| s.bg(hover))
    }
}

/// `filter: brightness(0.92)`.
fn darken(c: Hsla) -> Hsla {
    Hsla { l: c.l * 0.92, ..c }
}

/// `.badge`: a small count or tag.
pub fn badge(text: impl Into<SharedString>, t: &Theme) -> Div {
    div()
        .flex_none()
        .h(px(20.))
        .flex()
        .items_center()
        .px(px(6.))
        .rounded(px(RADIUS_SM))
        .bg(t.bg_selected_muted)
        .text_size(px(crate::theme::FS_XS))
        .text_color(t.text_muted)
        .child(text.into())
}
