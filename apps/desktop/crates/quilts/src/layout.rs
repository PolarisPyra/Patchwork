//! The Quilts page drawn: the gallery, or the quilt opened, drawn live or as its source,
//! with its versions.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Div, KeyDownEvent, Role, SharedString, Stateful, StyledText, Window, div,
    prelude::*, px, uniform_list,
};
use patchwork_backend::quilts::{Quilt, QuiltKind, ChangeRow};

use super::catalog::{QuiltsPage, Show};
use patchwork_ui_theme::{Theme, font_mono, fs_md, fs_sm, radius_sm, theme};
use patchwork_ui_components::menu::Menu;
use patchwork_ui_components::{button, focus_ring, icon_button, input_box, scrollbar, tooltip};
use patchwork_ui_markdown::format_relative;

/// The height of a line of the changes.
const CHANGE_LINE: f32 = 20.;

/// The language the source is coloured as.
fn language(quilt: &Quilt) -> &str {
    match quilt.kind {
        QuiltKind::Html | QuiltKind::Slides | QuiltKind::Design | QuiltKind::Data => "html",
        QuiltKind::Svg | QuiltKind::Whiteboard => "xml",
        QuiltKind::Markdown | QuiltKind::Doc => "markdown",
        QuiltKind::Sheet | QuiltKind::Mermaid => "",
        QuiltKind::React => "jsx",
        QuiltKind::Code => quilt.language.as_deref().unwrap_or_default(),
    }
}

/// The icon each type is listed with.
#[must_use]
pub fn kind_icon(kind: QuiltKind) -> &'static str {
    match kind {
        QuiltKind::Html | QuiltKind::React => "globe",
        QuiltKind::Svg | QuiltKind::Mermaid => "sparkle",
        QuiltKind::Markdown => "eye",
        QuiltKind::Code => "code",
        QuiltKind::Doc => "file-text",
        QuiltKind::Slides => "play",
        QuiltKind::Sheet => "list-numbers",
        QuiltKind::Design => "palette",
        QuiltKind::Data => "chart-bar",
        QuiltKind::Whiteboard => "note-pencil",
    }
}

impl QuiltsPage {
    /// Preview, when the kind is drawn, Code and Changes, as tabs.
    fn tabs(&self, drawn: bool, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let (fg, muted, selected) = (t.text, t.text_muted, t.bg_selected_muted);
        let shows: &'static [(Show, &str)] = if drawn {
            &[
                (Show::Preview, "Preview"),
                (Show::Code, "Code"),
                (Show::Changes, "Changes"),
            ]
        } else {
            &[(Show::Code, "Code"), (Show::Changes, "Changes")]
        };
        let tab = |show: Show, label: &'static str, cx: &mut Context<Self>| {
            let on = self.show == show;
            div()
                .id(label)
                .role(Role::Tab)
                .aria_label(label)
                .aria_selected(on)
                .tab_index(0)
                .focus_visible(focus_ring)
                .cursor_pointer()
                .h(px(24.))
                .px(px(12.))
                .flex()
                .items_center()
                .rounded_full()
                .text_size(px(fs_sm()))
                .text_color(if on { fg } else { muted })
                .when(on, |d| d.bg(selected))
                .hover(move |s| s.text_color(fg))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_show(show, cx)))
                .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
                    let by = match e.keystroke.key.as_str() {
                        "left" => shows.len() - 1,
                        "right" => 1,
                        _ => return,
                    };
                    let at = shows.iter().position(|(s, _)| *s == this.show).unwrap_or(0);
                    this.set_show(shows[(at + by) % shows.len()].0, cx);
                    cx.stop_propagation();
                }))
        };
        div()
            .id("quilt-tabs")
            .role(Role::TabList)
            .aria_label("Show")
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(2.))
            .rounded_full()
            .bg(t.bg_input)
            .border_1()
            .border_color(t.border)
            .children(shows.iter().map(|&(show, label)| tab(show, label, cx)))
    }

    /// The version shown, with arrows to the one before and after, and Restore on an old one.
    fn versions(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let numbers: Vec<u32> = self.versions.iter().map(|v| v.n).collect();
        let at = self
            .shown
            .and_then(|n| numbers.iter().position(|&x| x == n))
            .unwrap_or_default();
        let (before, after) = (
            at.checked_sub(1).and_then(|i| numbers.get(i).copied()),
            numbers.get(at + 1).copied(),
        );
        let step = |id: &'static str,
                    label: &'static str,
                    glyph: &'static str,
                    to: Option<u32>,
                    cx: &mut Context<Self>| {
            icon_button(id, label, glyph, 14., cx)
                .tooltip(tooltip(label))
                .when(to.is_none(), |b| b.opacity(0.4).cursor_default())
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(n) = to {
                        this.show(n, cx);
                    }
                }))
        };
        let when = self
            .version()
            .map(|v| format_relative(v.created_at))
            .unwrap_or_default();
        div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(step(
                "quilt-older",
                "Older version",
                "arrow-left",
                before,
                cx,
            ))
            .child(
                // The arrow keys step through the versions, as a slider's do through its values.
                div()
                    .id("quilt-version")
                    .role(Role::Slider)
                    .aria_label("Version")
                    .aria_numeric_value((at + 1) as f64)
                    .aria_min_numeric_value(1.)
                    .aria_max_numeric_value(numbers.len() as f64)
                    .tab_index(0)
                    .focus_visible(focus_ring)
                    .px(px(4.))
                    .rounded(px(radius_sm()))
                    .text_size(px(fs_sm()))
                    .text_color(t.text_muted)
                    .child(format!("Version {} of {} · {when}", at + 1, numbers.len()))
                    .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                        let by = match e.keystroke.key.as_str() {
                            "left" | "down" => -1,
                            "right" | "up" => 1,
                            "home" => isize::MIN,
                            "end" => isize::MAX,
                            _ => return,
                        };
                        this.step(by, cx);
                        cx.stop_propagation();
                    })),
            )
            .child(step(
                "quilt-newer",
                "Newer version",
                "arrow-right",
                after,
                cx,
            ))
            .when(after.is_some(), |d| {
                d.child(
                    patchwork_ui_components::btn(
                        "quilt-restore",
                        "Restore",
                        patchwork_ui_components::BtnKind::Plain,
                        false,
                        t,
                    )
                    .ml(px(6.))
                    .on_click(cx.listener(|this, _, _, cx| this.restore(cx))),
                )
            })
    }

    fn header(&self, quilt: &Quilt, t: &Theme, cx: &mut Context<Self>) -> Div {
        let chat_shown = self.chat_shown.as_ref() == Some(&quilt.chat_id);
        let (id, pinned, docked) = (quilt.id.clone(), quilt.pinned, self.docked);
        let title = match &self.renaming {
            Some(field) => div()
                .flex_1()
                .min_w_0()
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    if e.keystroke.key == "escape" {
                        this.cancel_rename(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(input_box(true, t).child(div().flex_1().min_w_0().child(field.clone()))),
            None => div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(fs_md()))
                .child(quilt.title.clone()),
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .py(px(8.))
            .border_b_1()
            .border_color(t.border)
            .when(!docked, |d| {
                d.child(
                    icon_button("quilt-back", "Back to quilts", "arrow-left", 16., cx)
                        .tooltip(tooltip("Back to quilts"))
                        .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                )
            })
            .child(title)
            .when(!docked && self.renaming.is_none(), |d| {
                let id = id.clone();
                d.child(
                    icon_button("quilt-rename", "Rename", "pencil-simple", 16., cx)
                        .tooltip(tooltip("Rename"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.start_rename(&id, window, cx);
                        })),
                )
            })
            .when(!docked, |d| {
                let id = id.clone();
                d.child(
                    icon_button("quilt-pin", "Pin", "push-pin", 16., cx)
                        .tooltip(tooltip(if pinned { "Unpin" } else { "Pin" }))
                        .aria_toggled(pinned.into())
                        .when(pinned, |b| b.bg(t.bg_selected_muted))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.pin(id.clone(), !pinned, cx)),
                        ),
                )
            })
            .child(self.versions(t, cx))
            .child(self.tabs(quilt.kind.drawn(), t, cx))
            .when(!docked, |d| {
                d.child(
                    icon_button("quilt-copy", "Copy", "copy", 16., cx)
                        .tooltip(tooltip("Copy"))
                        .on_click(cx.listener(|this, _, _, cx| this.copy(cx))),
                )
                .child(
                    icon_button("quilt-save", "Save as", "export", 16., cx)
                        .tooltip(tooltip("Save as"))
                        .on_click(cx.listener(|this, _, _, cx| this.save_as(cx))),
                )
                .child(
                    icon_button("quilt-chat", "Chat", "chat-circle", 16., cx)
                        .tooltip(tooltip("Chat"))
                        .aria_expanded(chat_shown)
                        .when(chat_shown, |b| b.bg(t.bg_selected_muted))
                        .on_click(cx.listener(|this, _, _, cx| this.open_chat(cx))),
                )
                .child(
                    icon_button("quilt-delete", "Delete", "trash", 16., cx)
                        .tooltip(tooltip("Delete"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.delete(id.clone(), window, cx);
                        })),
                )
            })
            .when(quilt.kind == QuiltKind::Slides, |d| {
                d.child(
                    icon_button("quilt-present", "Present", "play", 16., cx)
                        .tooltip(tooltip("Present"))
                        .on_click(cx.listener(|this, _, _, cx| this.open_outside(cx))),
                )
            })
            .when(quilt.kind.drawn() && self.show == Show::Preview, |d| {
                let picking = self.picking(cx);
                d.child(
                    icon_button("quilt-pick", "Pick an element", "target", 16., cx)
                        .tooltip(tooltip(if picking {
                            "Stop picking (Esc)"
                        } else {
                            "Pick an element to ask about"
                        }))
                        .aria_toggled(picking.into())
                        .when(picking, |b| b.bg(t.bg_selected_muted))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_picking(cx))),
                )
            })
            .when(quilt.kind.drawn(), |d| {
                d.child(
                    icon_button("quilt-open", "Open in browser", "arrow-square-out", 16., cx)
                        .tooltip(tooltip("Open in browser"))
                        .on_click(cx.listener(|this, _, _, cx| this.open_outside(cx))),
                )
            })
            .when(docked, |d| {
                d.child(
                    icon_button("quilt-full", "Open on Quilts page", "code-block", 16., cx)
                        .tooltip(tooltip("Open on Quilts page"))
                        .on_click(cx.listener(|this, _, _, cx| this.open_full(cx))),
                )
                .child(
                    icon_button("quilt-close", "Close quilt", "x", 16., cx)
                        .tooltip(tooltip("Close"))
                        .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                )
            })
    }

    /// The errors the page met, with a button that asks the chat that made it to fix them.
    fn page_alert(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let errors = self.page_errors(cx);
        let first = errors.first()?.clone();
        let more = errors.len() - 1;
        let (hover, fg) = (t.bg_hover, t.text);
        Some(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .py(px(6.))
                .bg(t.danger_soft)
                .text_size(px(fs_sm()))
                .child(
                    div()
                        .id("quilt-page-errors")
                        .role(Role::Alert)
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(if more == 0 {
                            first
                        } else {
                            format!("{first} (and {more} more)")
                        }),
                )
                .child(
                    button("quilt-ask-fix", "Ask to fix")
                        .flex_none()
                        .px(px(8.))
                        .py(px(2.))
                        .rounded(px(radius_sm()))
                        .border_1()
                        .border_color(t.border)
                        .hover(move |s| s.bg(hover).text_color(fg))
                        .child("Ask to fix")
                        .on_click(cx.listener(|this, _, _, cx| this.ask_fix(cx))),
                ),
        )
    }

    /// The version's source, coloured.
    fn source(&self, quilt: &Quilt, t: &Theme) -> AnyElement {
        let code: SharedString = self
            .version()
            .map(|v| v.content.clone())
            .unwrap_or_default()
            .into();
        let runs = patchwork_ui_markdown::highlight::highlight_code(&code, language(quilt));
        let text = StyledText::new(code).with_highlights(
            runs.iter()
                .map(|(range, token)| (range.clone(), token.style(t))),
        );
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .bg(t.bg_editor)
            .child(
                div()
                    .id("quilt-source")
                    .role(Role::Document)
                    .aria_label(format!("{} source", quilt.title))
                    .tab_index(0)
                    .focus_visible(focus_ring)
                    .size_full()
                    .overflow_scroll()
                    .track_scroll(&self.code_scroll)
                    .p(px(12.))
                    .font_family(font_mono())
                    .text_size(px(fs_sm()))
                    .child(text),
            )
            .children(scrollbar(&self.code_scroll, 10., 2., t.border_strong))
            .into_any_element()
    }

    /// What changed in the version since the one before it: lines taken out, then put in, with
    /// the unchanged lines far from a change folded.
    fn changes_view(&self, quilt: &Quilt, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let t = *t;
        let list = uniform_list(
            "quilt-changes-lines",
            self.changes.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, _| {
                range
                    .filter_map(|i| this.changes.get(i))
                    .map(|row| {
                        let (mark, text, bg, fg) = match row {
                            ChangeRow::Same(s) => (" ", s.clone(), None, t.text),
                            ChangeRow::Added(s) => ("+", s.clone(), Some(t.diff_add_bg), t.text),
                            ChangeRow::Removed(s) => ("-", s.clone(), Some(t.diff_del_bg), t.text),
                            ChangeRow::Folded(n) => (
                                " ",
                                format!("{n} unchanged lines"),
                                Some(t.bg_hover),
                                t.text_muted,
                            ),
                        };
                        div()
                            .h(px(CHANGE_LINE))
                            .flex()
                            .items_center()
                            .whitespace_nowrap()
                            .text_color(fg)
                            .when_some(bg, gpui::Styled::bg)
                            .child(div().flex_none().w(px(20.)).pl(px(6.)).child(mark))
                            .child(SharedString::from(text))
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.changes_scroll)
        .size_full();
        div()
            .id("quilt-changes")
            .role(Role::Document)
            .aria_label(format!("{} changes", quilt.title))
            .tab_index(0)
            .focus_visible(focus_ring)
            .flex_1()
            .min_h_0()
            .py(px(8.))
            .bg(t.bg_editor)
            .font_family(font_mono())
            .text_size(px(fs_sm()))
            .child(list)
            .into_any_element()
    }

    fn viewer(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(quilt) = self.current().cloned() else {
            return div().into_any_element();
        };
        let body = match self.show {
            Show::Changes => self.changes_view(&quilt, t, cx),
            Show::Code => self.source(&quilt, t),
            Show::Preview if !quilt.kind.drawn() => self.source(&quilt, t),
            Show::Preview => match &self.preview {
                Some(view) => div()
                    .flex_1()
                    .min_h_0()
                    .child(view.clone())
                    .into_any_element(),
                None => div().flex_1().bg(t.bg_editor).into_any_element(),
            },
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(self.header(&quilt, t, cx))
            .children(self.page_alert(t, cx))
            .children(self.error.clone().map(|e| {
                div()
                    .id("quilt-error")
                    .role(Role::Alert)
                    .px(px(12.))
                    .py(px(6.))
                    .bg(t.danger_soft)
                    .text_size(px(fs_sm()))
                    .child(e)
            }))
            .child(body)
            .into_any_element()
    }
}

impl Render for QuiltsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let open = self.current().is_some();
        div()
            .id("quilts-page")
            .role(Role::Region)
            .aria_label("Quilts")
            .track_focus(&self.focus)
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            // The full page shows the chat's background under it; beside a chat it is a panel.
            .when(self.docked, |d| d.bg(t.bg_app))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" && this.current().is_some() {
                    this.close(cx);
                    cx.stop_propagation();
                }
            }))
            .map(|d| {
                if open {
                    d.child(self.viewer(&t, cx))
                } else if self.docked {
                    // Beside a chat, the quilt shows once it is read; there is no gallery.
                    d
                } else {
                    d.child(self.gallery(&t, window, cx))
                }
            })
            .children(self.menu.as_ref().map(|(menu, at)| Menu::view(menu, *at)))
    }
}
