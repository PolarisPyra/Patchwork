//! A bot's details beside its conversation: its look and name, then the Details, Library and
//! Computer tabs.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Div, Focusable, FontWeight, KeyDownEvent, Role, SharedString, Stateful, Toggled,
    Window, div, prelude::*, px,
};
use crate::ui::avatar::{Mood, avatar};
use crate::ui::theme::{Theme, fs_md, fs_sm, fs_xl, radius_sm, theme};
use crate::ui::{
    BtnKind, btn, button, checkbox, display_title, focus_ring, format_relative, format_tokens,
    icon, icon_button, input_box, spinner,
};
use super::state::{BotPage, Tab};

/// The width of the details.
const WIDTH: f32 = 320.;

/// A section's title over its controls.
pub(super) fn heading(text: &'static str, t: &Theme) -> Stateful<Div> {
    div()
        .id(text)
        .role(Role::Heading)
        .aria_level(2)
        .aria_label(text)
        .pt(px(20.))
        .pb(px(6.))
        .text_size(px(fs_md()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.text)
        .child(text)
}

/// One choice of a group where one is picked.
pub(super) fn radio(
    id: SharedString,
    label: SharedString,
    checked: bool,
    t: &Theme,
) -> Stateful<Div> {
    let (hover, fg, muted) = (t.bg_hover, t.text, t.text_muted);
    div()
        .id(id)
        .role(Role::RadioButton)
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
        .h(px(26.))
        .px(px(12.))
        .rounded(px(radius_sm()))
        .border_1()
        .whitespace_nowrap()
        .text_size(px(fs_sm()))
        .map(|d| {
            if checked {
                d.border_color(t.accent).bg(t.bg_accent_soft).text_color(fg)
            } else {
                d.border_color(t.border_strong)
                    .text_color(muted)
                    .hover(move |s| s.bg(hover).text_color(fg))
            }
        })
        .child(label)
}

/// A row of radio choices under one name.
pub(super) fn radio_group(id: &str, label: &str) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("{id}-group")))
        .role(Role::RadioGroup)
        .aria_label(label.to_string())
        .flex()
        .flex_wrap()
        .gap(px(6.))
}

impl BotPage {
    /// The details: a close button, the bot's avatar and name, then the tabs and the open one.
    pub(super) fn details(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = *theme(cx);
        let body = match (self.drill, self.tab) {
            (Some(to), _) => self.drill_page(to, &t, cx),
            (None, Tab::Details) => self.details_tab(cx),
            (None, Tab::Library) => self.knowledge_tab(cx),
            (None, Tab::Computer) => self.access(cx),
        };
        let label = Tab::ALL
            .iter()
            .find(|(tab, _)| *tab == self.tab)
            .map_or("", |(_, label)| *label);
        div()
            .id("bot-details")
            .role(Role::Complementary)
            .aria_label("Bot details")
            .flex_none()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .min_h_0()
            .border_l_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .justify_end()
                    .px(px(8.))
                    .pt(px(8.))
                    .child(
                        icon_button("bot-background", "Background", "image", 16., cx).on_click(
                            cx.listener(|this, _, window, cx| {
                                let (workspace, id) = (this.workspace.clone(), this.bot.id.clone());
                                let _ = workspace.update(cx, |w, cx| {
                                    w.open_bot_background_dialog(&id, window, cx);
                                });
                            }),
                        ),
                    )
                    .child(
                        icon_button("bot-details-close", "Close details", "x", 16., cx)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_details(cx))),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .px(px(16.))
                    .child(avatar(&self.bot.id, &self.bot.name, Mood::Idle, 72.))
                    .child(self.name_view(window, &t, cx))
                    .child(self.tabs(&t, cx)),
            )
            .child(
                div()
                    .id("bot-tab-body")
                    .role(Role::TabPanel)
                    .aria_label(label)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(16.))
                    .pt(px(8.))
                    .pb(px(24.))
                    .child(body),
            )
    }

    /// The bot's name in bold, which turns into its field to rename it.
    fn name_view(&self, window: &mut Window, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        // The name is a field while it has the cursor.
        if self.name.read(cx).focus_handle(cx).is_focused(window) {
            return input_box(true, t)
                .id("bot-name-field")
                .w_full()
                .text_size(px(fs_md()))
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                    if e.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.cancel_name(window, cx);
                    }
                }))
                .child(div().flex_1().min_w_0().child(self.name.clone()))
                .into_any_element();
        }
        let hover = t.bg_hover;
        button("bot-name", format!("Rename {}", self.bot.name))
            .max_w_full()
            .px(px(8.))
            .py(px(2.))
            .rounded(px(radius_sm()))
            .hover(move |s| s.bg(hover))
            .text_size(px(fs_xl()))
            .font_weight(FontWeight::BOLD)
            .child(div().truncate().child(self.bot.name.clone()))
            .on_click(cx.listener(|this, _, window, cx| this.edit_name(window, cx)))
            .into_any_element()
    }

    /// The tabs as one pill, the open one raised.
    fn tabs(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let (fg, muted, selected) = (t.text, t.text_muted, t.bg_selected_muted);
        div()
            .id("bot-tabs")
            .role(Role::TabList)
            .aria_label("Bot settings")
            .flex()
            .items_center()
            .gap(px(2.))
            .p(px(3.))
            .rounded_full()
            .bg(t.bg_input)
            .border_1()
            .border_color(t.border)
            .children(Tab::ALL.into_iter().map(|(tab, label)| {
                let on = self.tab == tab;
                div()
                    .id(SharedString::from(format!("bot-tab-{label}")))
                    .role(Role::Tab)
                    .aria_label(label)
                    .aria_selected(on)
                    .tab_index(0)
                    .focus_visible(focus_ring)
                    .cursor_pointer()
                    .h(px(26.))
                    .px(px(14.))
                    .flex()
                    .items_center()
                    .rounded_full()
                    .text_size(px(fs_md()))
                    .text_color(if on { fg } else { muted })
                    .when(on, |d| d.bg(selected))
                    .hover(move |s| s.text_color(fg))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| this.open_tab(tab, cx)))
                    .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
                        let n = Tab::ALL.len();
                        let i = Tab::ALL
                            .iter()
                            .position(|(x, _)| *x == this.tab)
                            .unwrap_or(0);
                        let i = match e.keystroke.key.as_str() {
                            "left" => (i + n - 1) % n,
                            "right" => (i + 1) % n,
                            _ => return,
                        };
                        this.open_tab(Tab::ALL[i].0, cx);
                    }))
            }))
    }

    /// The bot's conversations, the newest one's tools, and its routines.
    fn details_tab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *theme(cx);
        div()
            .flex()
            .flex_col()
            .child(heading("Conversations", &t))
            .child(self.conversations(&t, cx))
            .children(self.this_conversation(&t, cx))
            .child(self.routines(&t, cx))
            .into_any_element()
    }

    /// New conversation, then each of the bot's runs, newest first.
    fn conversations(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let hover = t.bg_hover;
        let row = |id: SharedString, label: String| {
            button(id, label)
                .gap(px(8.))
                .h(px(30.))
                .px(px(8.))
                .rounded(px(radius_sm()))
                .hover(move |s| s.bg(hover))
                .text_size(px(fs_md()))
        };
        let new = row("bot-new-conversation".into(), "New conversation".into())
            .text_color(t.text_muted)
            .child(icon("note-pencil", 15.))
            .child("New conversation")
            .on_click(cx.listener(|this, _, window, cx| {
                if let Some(w) = this.workspace.upgrade() {
                    let editor = w.read(cx).composer.read(cx).editor.clone();
                    editor.update(cx, |e, cx| e.clear(window, cx));
                }
                this.focus_composer(window, cx);
            }));
        let Some(usage) = &self.usage else {
            return div()
                .flex()
                .flex_col()
                .child(new)
                .child(
                    div()
                        .id("bot-runs-loading")
                        .role(Role::Status)
                        .aria_label("Loading conversations")
                        .p(px(8.))
                        .child(spinner(16., t.text_muted)),
                )
                .into_any_element();
        };
        let mut list = div()
            .id("bot-runs")
            .role(Role::List)
            .aria_label("Conversations")
            .flex()
            .flex_col();
        for run in &usage.runs {
            let id = run.thread_id.clone();
            let tokens =
                run.usage.input + run.usage.cache_read + run.usage.cache_write + run.usage.output;
            let title = display_title(&run.title);
            list = list.child(
                row(
                    SharedString::from(format!("bot-run-{}", run.thread_id)),
                    format!("Open {title}"),
                )
                .role(Role::ListItem)
                .child(div().flex_1().min_w_0().truncate().child(title))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(fs_sm()))
                        .text_color(t.text_muted)
                        .child(format!(
                            "{} · {}",
                            format_tokens(tokens),
                            format_relative(run.started_at),
                        )),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.open_run(&id, cx))),
            );
        }
        div()
            .flex()
            .flex_col()
            .child(new)
            .child(list)
            .into_any_element()
    }

    /// The files the bot knows, with Add and Remove.
    fn knowledge_tab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *theme(cx);
        let mut list = div()
            .id("bot-files")
            .role(Role::List)
            .aria_label("Files")
            .flex()
            .flex_col()
            .gap(px(2.));
        if self.knowledge.is_empty() {
            list = list.child(
                div()
                    .py(px(6.))
                    .text_size(px(fs_md()))
                    .text_color(t.text_muted)
                    .child("No files yet"),
            );
        }
        for file in &self.knowledge {
            let name = file.name.clone();
            list = list.child(
                div()
                    .id(SharedString::from(format!("bot-file-{}", file.name)))
                    .role(Role::ListItem)
                    .aria_label(file.name.clone())
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(32.))
                    .px(px(8.))
                    .rounded(px(radius_sm()))
                    .hover(|s| s.bg(t.bg_hover))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(fs_md()))
                            .child(file.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(fs_sm()))
                            .text_color(t.text_muted)
                            .child(format_size(file.size)),
                    )
                    .child(
                        icon_button(
                            SharedString::from(format!("bot-file-remove-{}", file.name)),
                            format!("Remove {}", file.name),
                            "trash",
                            15.,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_file(&name, cx))),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .child(
                div().pt(px(16.)).pb(px(8.)).child(
                    btn("bot-add-files", "Add files", BtnKind::Plain, false, &t)
                        .child("Add files")
                        .on_click(cx.listener(|this, _, _, cx| this.add_files(cx))),
                ),
            )
            .child(list)
            .into_any_element()
    }

    /// The projects the bot may work in.
    fn access(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let t = *theme(cx);
        let local: Vec<_> = self
            .core
            .settings()
            .projects
            .into_iter()
            .filter(|p| p.host().is_none())
            .collect();
        let mut projects = div()
            .id("bot-projects")
            .role(Role::Group)
            .aria_label("Projects")
            .flex()
            .flex_col()
            .gap(px(8.));
        if local.is_empty() {
            projects = projects.child(
                div()
                    .text_size(px(fs_md()))
                    .text_color(t.text_muted)
                    .child("No projects yet"),
            );
        }
        for project in local {
            let on = self.bot.projects.contains(&project.id);
            let click = project.id.clone();
            projects = projects.child(
                checkbox(
                    SharedString::from(format!("bot-project-{}", project.id)),
                    project.name.clone(),
                    on,
                    &t,
                )
                .text_size(px(fs_md()))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_project(&click, cx))),
            );
        }
        div()
            .flex()
            .flex_col()
            .child(heading("Projects", &t))
            .child(projects)
            .into_any_element()
    }
}

/// `bytes` as a person reads a file's size.
fn format_size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{} KB", bytes / 1024),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.),
    }
}
