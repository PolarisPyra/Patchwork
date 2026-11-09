//! The Output view (`OutputView.tsx`, `logs.ts`): one of this launch's logs, followed as it
//! grows, with the time dimmed and the level colored.

use std::time::Duration;

use gpui::{
    AnyElement, ClipboardItem, Context, FocusHandle, HighlightStyle, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, Role, SharedString, StyledText, Task,
    UniformListScrollHandle, Window, div, font, prelude::*, px, uniform_list,
};
use patchwork_server::settings::{LogLevel, LoggingSettings};

use super::Panel;
use super::cells::ordered;
use crate::theme::{FONT_MONO, FS_SM, Theme, theme};
use crate::ui::agent_label;

/// How often a shown log is read for new lines.
const FOLLOW: Duration = Duration::from_millis(500);
/// How many reads go by between looks for new logs.
const LIST_EVERY: u32 = 4;
/// Lines kept on screen; older ones are dropped as new ones come in.
const MAX_LINES: usize = 20_000;
/// A line's height, `leading-[1.5]`.
const LINE_H: f32 = FS_SM * 1.5;
/// The list's padding, `px-3 py-1`.
const PAD: (f32, f32) = (12., 4.);

/// One read: the logs, when it looked for them, and the shown log's new part from its offset.
type Read = (
    Option<Vec<String>>,
    Option<(String, u64, Option<patchwork_server::logging::LogChunk>)>,
);

/// A log the Output view can show: its path in the launch's log folder, its name, and the setting
/// that sets its level.
#[derive(Clone)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub setting: fn(&mut LoggingSettings) -> &mut LogLevel,
}

fn channel(id: String) -> Channel {
    let (name, setting): (String, fn(&mut LoggingSettings) -> &mut LogLevel) = match id.as_str() {
        "main.log" => ("Main".into(), |l| &mut l.app),
        "window.log" => ("Window".into(), |l| &mut l.window),
        "imports.log" => ("Imports".into(), |l| &mut l.imports),
        _ => {
            let agent = id
                .strip_prefix("agent-")
                .and_then(|s| s.strip_suffix(".log"))
                .and_then(|name| {
                    serde_json::from_value(serde_json::Value::String(name.into())).ok()
                })
                .map(agent_label);
            (agent.map_or_else(|| id.clone(), str::to_string), |l| {
                &mut l.agents
            })
        }
    };
    Channel { id, name, setting }
}

/// Main and Window first, then the agents, then the rest.
fn sort_channels(ids: Vec<String>) -> Vec<Channel> {
    let rank = |c: &Channel| match c.id.as_str() {
        "main.log" => 0,
        "window.log" => 1,
        id if id.starts_with("agent-") => 2,
        _ => 3,
    };
    let mut channels: Vec<Channel> = ids.into_iter().map(channel).collect();
    channels.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name)));
    channels
}

#[derive(Default)]
pub struct OutputState {
    pub channels: Vec<Channel>,
    /// The log picked; None is the first.
    pub picked: Option<String>,
    lines: Vec<SharedString>,
    /// The log the lines are from, and where its next read starts.
    reading: Option<(String, u64)>,
    scroll: UniformListScrollHandle,
    follow: Option<Task<()>>,
    /// A drag over the lines: where it started and where it is, as (line, character).
    selection: Option<((usize, usize), (usize, usize))>,
    selecting: bool,
    focus: Option<FocusHandle>,
    /// The width of a character in the log's monospace font.
    char_w: f32,
}

impl OutputState {
    pub fn channel(&self) -> Option<&Channel> {
        self.channels
            .iter()
            .find(|c| Some(&c.id) == self.picked.as_ref())
            .or(self.channels.first())
    }
}

impl Panel {
    /// Follows the logs while the Output view shows, and stops when it goes.
    pub(super) fn follow_output(&mut self, on: bool, cx: &mut Context<Self>) {
        if !on {
            self.output.follow = None;
            return;
        }
        if self.output.follow.is_some() {
            return;
        }
        self.output.follow = Some(cx.spawn(async move |this, cx| {
            let mut tick = 0;
            loop {
                let list = tick % LIST_EVERY == 0;
                tick += 1;
                let Ok(reading) = this.update(cx, |this, _| {
                    let id = this.output.channel().map(|c| c.id.clone());
                    let offset = match (&this.output.reading, &id) {
                        (Some((was, offset)), Some(id)) if was == id => *offset,
                        _ => 0,
                    };
                    id.map(|id| (id, offset))
                }) else {
                    return;
                };
                let read = cx
                    .background_spawn(async move {
                        let ids = list.then(patchwork_server::logging::channels);
                        let chunk = reading.map(|(id, offset)| {
                            let chunk = patchwork_server::logging::read(&id, offset).ok();
                            (id, offset, chunk)
                        });
                        (ids, chunk)
                    })
                    .await;
                let Ok(()) = this.update(cx, |this, cx| this.take_output(read, cx)) else {
                    return;
                };
                cx.background_executor().timer(FOLLOW).await;
            }
        }));
    }

    fn take_output(&mut self, (ids, chunk): Read, cx: &mut Context<Self>) {
        if let Some(ids) = ids {
            self.output.channels = sort_channels(ids);
        }
        let Some((id, offset, chunk)) = chunk else {
            return cx.notify();
        };
        let fresh = self
            .output
            .reading
            .as_ref()
            .is_none_or(|(was, _)| *was != id);
        if fresh {
            self.output.lines.clear();
        }
        // The log is not written yet; it is read again shortly.
        let Some(chunk) = chunk else {
            self.output.reading = Some((id, offset));
            return cx.notify();
        };
        if chunk.reset {
            self.output.lines.clear();
        }
        let at_bottom = self.output.scroll.is_scrolled_to_end().unwrap_or(true);
        let before = self.output.lines.len();
        self.output.lines.extend(
            chunk
                .text
                .split('\n')
                .filter(|l| !l.is_empty())
                .map(|l| SharedString::from(l.to_string())),
        );
        let over = self.output.lines.len().saturating_sub(MAX_LINES);
        self.output.lines.drain(..over);
        self.output.reading = Some((id, chunk.offset));
        // Dropped lines take the selection's place with them.
        if over > 0 || chunk.reset || fresh {
            self.output.selection = None;
        }
        if self.output.lines.len() != before || fresh {
            if at_bottom || fresh {
                self.output.scroll.scroll_to_bottom();
            }
            cx.notify();
        }
    }

    pub(super) fn pick_channel(&mut self, id: String, cx: &mut Context<Self>) {
        self.output.picked = Some(id);
        self.output.reading = None;
        self.output.lines.clear();
        self.output.selection = None;
        cx.notify();
    }

    /// The line and character under `position`, clamped to the lines there are.
    fn output_cell(&self, position: Point<Pixels>) -> (usize, usize) {
        let handle = &self.output.scroll.0.borrow().base_handle;
        let (bounds, offset) = (handle.bounds(), handle.offset());
        let x = f32::from(position.x - bounds.origin.x - offset.x) - PAD.0;
        let y = f32::from(position.y - bounds.origin.y - offset.y) - PAD.1;
        let last = self.output.lines.len().saturating_sub(1);
        let line = ((y / LINE_H).max(0.) as usize).min(last);
        let chars = self.output.lines.get(line).map_or(0, |l| l.chars().count());
        let col = ((x / self.output.char_w).round().max(0.) as usize).min(chars);
        (line, col)
    }

    fn output_selected(&self) -> Option<String> {
        let (a, b) = ordered(self.output.selection?);
        if a == b {
            return None;
        }
        let text = (a.0..=b.0)
            .filter_map(|i| {
                let line = self.output.lines.get(i)?;
                let from = if i == a.0 { a.1 } else { 0 };
                let to = if i == b.0 { b.1 } else { usize::MAX };
                Some(line.chars().skip(from).take(to - from).collect::<String>())
            })
            .collect::<Vec<_>>()
            .join("\n");
        Some(text)
    }

    fn on_output_key(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let ks = &e.keystroke;
        let copy = ks.key == "c" && (ks.modifiers.control || ks.modifiers.platform);
        if copy && let Some(text) = self.output_selected() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            cx.stop_propagation();
        }
    }

    pub(super) fn output_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = *theme(cx);
        let focus = self
            .output
            .focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        let text = window.text_system();
        self.output.char_w = text
            .advance(text.resolve_font(&font(FONT_MONO)), px(FS_SM), 'm')
            .map_or(FS_SM * 0.6, |a| f32::from(a.width));
        let char_w = self.output.char_w;
        let lines = self.output.lines.clone();
        let selection = self.output.selection.map(ordered).filter(|(a, b)| a != b);
        let list = uniform_list("output-lines", lines.len(), move |range, _, _| {
            range
                .map(|i| {
                    let picked = selection.and_then(|(a, b)| {
                        (a.0..=b.0).contains(&i).then(|| {
                            let from = if i == a.0 { a.1 } else { 0 };
                            let to = if i == b.0 {
                                b.1
                            } else {
                                lines[i].chars().count()
                            };
                            from..to
                        })
                    });
                    line(&lines[i], picked, char_w, &t).into_any_element()
                })
                .collect()
        })
        .track_scroll(&self.output.scroll)
        .size_full()
        .px(px(PAD.0))
        .py(px(PAD.1));
        div()
            .id("output")
            .role(Role::Log)
            .aria_label("Output")
            .track_focus(&focus)
            .size_full()
            .min_w_0()
            .cursor_text()
            .on_key_down(cx.listener(Self::on_output_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                    window.focus(&focus, cx);
                    let cell = this.output_cell(e.position);
                    this.output.selection = Some((cell, cell));
                    this.output.selecting = true;
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if !this.output.selecting || !e.dragging() {
                    return;
                }
                let cell = this.output_cell(e.position);
                if let Some((_, end)) = &mut this.output.selection
                    && *end != cell
                {
                    *end = cell;
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.output.selecting = false;
                    if this.output.selection.is_some_and(|(a, b)| a == b) {
                        this.output.selection = None;
                        cx.notify();
                    }
                }),
            )
            .child(list)
            .font_family(FONT_MONO)
            .text_size(px(FS_SM))
            .line_height(px(FS_SM * 1.5))
            .into_any_element()
    }
}

/// `2026-10-26 12:17:29.562 [info] …`, with the time dimmed and the level colored.
fn line(
    text: &SharedString,
    picked: Option<std::ops::Range<usize>>,
    char_w: f32,
    t: &Theme,
) -> impl IntoElement {
    let mut runs = Vec::new();
    let bytes = text.as_bytes();
    let stamped = bytes.len() > 25
        && bytes[4] == b'-'
        && bytes[10] == b' '
        && bytes[13] == b':'
        && bytes[19] == b'.'
        && bytes[23] == b' '
        && bytes[24] == b'[';
    if stamped && let Some(close) = text[25..].find(']').map(|i| i + 25) {
        let level = match &text[25..close] {
            "trace" | "debug" => Some(t.text_subtle),
            "info" => Some(t.accent_text),
            "warning" => Some(t.warning),
            "error" => Some(t.danger),
            _ => None,
        };
        runs.push((
            0..23,
            HighlightStyle {
                color: Some(t.text_subtle),
                ..Default::default()
            },
        ));
        if let Some(color) = level {
            runs.push((
                24..close + 1,
                HighlightStyle {
                    color: Some(color),
                    ..Default::default()
                },
            ));
        }
    }
    div()
        .relative()
        .whitespace_nowrap()
        .children(picked.map(|cols| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(cols.start as f32 * char_w))
                .w(px(cols.len() as f32 * char_w))
                .bg(t.bg_selected)
        }))
        .child(StyledText::new(text.clone()).with_highlights(runs))
}
