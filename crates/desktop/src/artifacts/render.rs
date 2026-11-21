//! The Artifacts page drawn: the gallery, or the artifact opened, drawn live or as its source,
//! with its versions.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Div, KeyDownEvent, Role, SharedString, Stateful, StyledText, Window, div,
    prelude::*, px, uniform_list,
};
use patchwork_core::artifacts::{Artifact, ArtifactKind, ChangeRow};

use super::state::{ArtifactsPage, Show};
use crate::ui::theme::{Theme, font_mono, fs_md, fs_sm, radius_sm, theme};
use crate::ui::menu::Menu;
use crate::ui::{button, focus_ring, format_relative, icon_button, input_box, scrollbar, tooltip};

/// The height of a line of the changes.
const CHANGE_LINE: f32 = 20.;

/// The language the source is coloured as.
fn language(artifact: &Artifact) -> &str {
    match artifact.kind {
        ArtifactKind::Html | ArtifactKind::Slides | ArtifactKind::Design | ArtifactKind::Data => {
            "html"
        }
        ArtifactKind::Svg | ArtifactKind::Whiteboard => "xml",
        ArtifactKind::Markdown | ArtifactKind::Doc => "markdown",
        ArtifactKind::Sheet | ArtifactKind::Mermaid => "",
        ArtifactKind::React => "jsx",
        ArtifactKind::Code => artifact.language.as_deref().unwrap_or_default(),
    }
}

/// The icon each type is listed with.
pub(crate) fn kind_icon(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Html | ArtifactKind::React => "globe",
        ArtifactKind::Svg | ArtifactKind::Mermaid => "sparkle",
        ArtifactKind::Markdown => "eye",
        ArtifactKind::Code => "code",
        ArtifactKind::Doc => "file-text",
        ArtifactKind::Slides => "play",
        ArtifactKind::Sheet => "list-numbers",
        ArtifactKind::Design => "palette",
        ArtifactKind::Data => "chart-bar",
        ArtifactKind::Whiteboard => "note-pencil",
    }
}

impl ArtifactsPage {
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
            .id("artifact-tabs")
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
                "artifact-older",
                "Older version",
                "arrow-left",
                before,
                cx,
            ))
            .child(
                // The arrow keys step through the versions, as a slider's do through its values.
                div()
                    .id("artifact-version")
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
                "artifact-newer",
                "Newer version",
                "arrow-right",
                after,
                cx,
            ))
            .when(after.is_some(), |d| {
                d.child(
                    crate::ui::btn(
                        "artifact-restore",
                        "Restore",
                        crate::ui::BtnKind::Plain,
                        false,
                        t,
                    )
                    .ml(px(6.))
                    .on_click(cx.listener(|this, _, _, cx| this.restore(cx))),
                )
            })
    }

    fn header(&self, artifact: &Artifact, t: &Theme, cx: &mut Context<Self>) -> Div {
        let chat_shown = self.chat_shown.as_ref() == Some(&artifact.thread_id);
        let (id, pinned, docked) = (artifact.id.clone(), artifact.pinned, self.docked);
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
                .child(artifact.title.clone()),
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
                    icon_button("artifact-back", "Back to artifacts", "arrow-left", 16., cx)
                        .tooltip(tooltip("Back to artifacts"))
                        .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                )
            })
            .child(title)
            .when(!docked && self.renaming.is_none(), |d| {
                let id = id.clone();
                d.child(
                    icon_button("artifact-rename", "Rename", "pencil-simple", 16., cx)
                        .tooltip(tooltip("Rename"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.start_rename(&id, window, cx);
                        })),
                )
            })
            .when(!docked, |d| {
                let id = id.clone();
                d.child(
                    icon_button("artifact-pin", "Pin", "push-pin", 16., cx)
                        .tooltip(tooltip(if pinned { "Unpin" } else { "Pin" }))
                        .aria_toggled(pinned.into())
                        .when(pinned, |b| b.bg(t.bg_selected_muted))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.pin(id.clone(), !pinned, cx)),
                        ),
                )
            })
            .child(self.versions(t, cx))
            .child(self.tabs(artifact.kind.drawn(), t, cx))
            .when(!docked, |d| {
                d.child(
                    icon_button("artifact-copy", "Copy", "copy", 16., cx)
                        .tooltip(tooltip("Copy"))
                        .on_click(cx.listener(|this, _, _, cx| this.copy(cx))),
                )
                .child(
                    icon_button("artifact-save", "Save as", "export", 16., cx)
                        .tooltip(tooltip("Save as"))
                        .on_click(cx.listener(|this, _, _, cx| this.save_as(cx))),
                )
                .child(
                    icon_button("artifact-chat", "Chat", "chat-circle", 16., cx)
                        .tooltip(tooltip("Chat"))
                        .aria_expanded(chat_shown)
                        .when(chat_shown, |b| b.bg(t.bg_selected_muted))
                        .on_click(cx.listener(|this, _, _, cx| this.open_chat(cx))),
                )
                .child(
                    icon_button("artifact-delete", "Delete", "trash", 16., cx)
                        .tooltip(tooltip("Delete"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.delete(id.clone(), window, cx);
                        })),
                )
            })
            .when(artifact.kind == ArtifactKind::Slides, |d| {
                d.child(
                    icon_button("artifact-present", "Present", "play", 16., cx)
                        .tooltip(tooltip("Present"))
                        .on_click(cx.listener(|this, _, _, cx| this.open_outside(cx))),
                )
            })
            .when(artifact.kind.drawn(), |d| {
                d.child(
                    icon_button(
                        "artifact-open",
                        "Open in browser",
                        "arrow-square-out",
                        16.,
                        cx,
                    )
                    .tooltip(tooltip("Open in browser"))
                    .on_click(cx.listener(|this, _, _, cx| this.open_outside(cx))),
                )
            })
            .when(docked, |d| {
                d.child(
                    icon_button(
                        "artifact-full",
                        "Open on Artifacts page",
                        "code-block",
                        16.,
                        cx,
                    )
                    .tooltip(tooltip("Open on Artifacts page"))
                    .on_click(cx.listener(|this, _, _, cx| this.open_full(cx))),
                )
                .child(
                    icon_button("artifact-close", "Close artifact", "x", 16., cx)
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
                        .id("artifact-page-errors")
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
                    button("artifact-ask-fix", "Ask to fix")
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
    fn source(&self, artifact: &Artifact, t: &Theme) -> AnyElement {
        let code: SharedString = self
            .version()
            .map(|v| v.content.clone())
            .unwrap_or_default()
            .into();
        let runs = crate::ui::highlight::highlight_code(&code, language(artifact));
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
                    .id("artifact-source")
                    .role(Role::Document)
                    .aria_label(format!("{} source", artifact.title))
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
    fn changes_view(&self, artifact: &Artifact, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let t = *t;
        let list = uniform_list(
            "artifact-changes-lines",
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
            .id("artifact-changes")
            .role(Role::Document)
            .aria_label(format!("{} changes", artifact.title))
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
        let Some(artifact) = self.current().cloned() else {
            return div().into_any_element();
        };
        let body = match self.show {
            Show::Changes => self.changes_view(&artifact, t, cx),
            Show::Code => self.source(&artifact, t),
            Show::Preview if !artifact.kind.drawn() => self.source(&artifact, t),
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
            .child(self.header(&artifact, t, cx))
            .children(self.page_alert(t, cx))
            .children(self.error.clone().map(|e| {
                div()
                    .id("artifact-error")
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

impl Render for ArtifactsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = *theme(cx);
        let open = self.current().is_some();
        div()
            .id("artifacts-page")
            .role(Role::Region)
            .aria_label("Artifacts")
            .track_focus(&self.focus)
            .flex()
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(t.bg_app)
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
                    // Beside a chat, the artifact shows once it is read; there is no gallery.
                    d
                } else {
                    d.child(self.gallery(&t, window, cx))
                }
            })
            .children(self.menu.as_ref().map(|(menu, at)| Menu::view(menu, *at)))
    }
}
