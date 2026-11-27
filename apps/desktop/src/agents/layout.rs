//! An agent's home drawn: the agent's name over a conversation that starts with its greeting, the
//! starting points while it has no instructions, the message field, and its details beside.

// Rust guideline compliant 2026-10-09

use gpui::{Div, FontWeight, KeyDownEvent, Role, SharedString, Stateful, Window, div, prelude::*, px};
use patchwork_backend::agents::STARTING_POINTS;
use crate::chat::backdrop::{backdrop_for, chat_backdrop, chat_scheme, scheme_for};
use patchwork_ui_components::avatar::{Mood, avatar};
use patchwork_ui_theme::{InScheme, Theme, fs_lg, fs_md, fs_sm, radius, radius_lg, radius_sm, theme};
use patchwork_ui_components::{button, icon_button, spinner, tooltip};
use patchwork_ui_theme::icon;
use super::details::{radio, radio_group};
use super::home::{AgentPage, Layout};

/// The widest the conversation gets.
const COLUMN: f32 = 720.;

/// The key that picks the starting point at `index`: A, B, C and on.
fn start_key(index: usize) -> char {
    u8::try_from(index).map_or('?', |i| char::from(b'A' + i))
}

/// The soft shadow under the header's buttons, as a chat's header draws it.
fn header_shadow() -> Vec<gpui::BoxShadow> {
    vec![gpui::BoxShadow {
        color: gpui::hsla(0., 0., 0., 0.12),
        offset: gpui::point(px(0.), px(1.)),
        blur_radius: px(3.),
        spread_radius: px(0.),
        inset: false,
    }]
}

impl Render for AgentPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.layout == Layout::BesideChat {
            // Drawn over the chat's background as the agent's page draws its details over its own.
            let scheme = self
                .workspace
                .upgrade()
                .and_then(|w| chat_scheme(w.read(cx)));
            let _scope = scheme.map(InScheme::enter);
            let t = *theme(cx);
            let backdrop = self
                .workspace
                .update(cx, |w, cx| chat_backdrop(w, &t, cx))
                .ok()
                .flatten();
            return div()
                .relative()
                .flex()
                .flex_none()
                .min_h_0()
                .bg(t.bg_editor)
                .children(backdrop)
                .child(self.details(window, cx))
                .into_any_element();
        }
        // Its own background, else the one every chat shares, drawn as a chat draws it.
        let background = self.workspace.upgrade().map(|w| {
            let w = w.read(cx);
            let background = self
                .agent
                .background
                .clone()
                .unwrap_or_else(|| w.settings.chat_background.clone());
            let scheme = scheme_for(w, &background);
            (background, scheme)
        });
        let _scope = background
            .as_ref()
            .and_then(|(_, scheme)| *scheme)
            .map(InScheme::enter);
        let t = *theme(cx);
        let backdrop = background.and_then(|(background, _)| {
            self.workspace
                .update(cx, |w, cx| backdrop_for(w, &background, &t, cx))
                .ok()
                .flatten()
        });
        let details = self.details_open.then(|| self.details(window, cx));
        div()
            .id("agent-page")
            .relative()
            .track_focus(&self.focus)
            .role(Role::Main)
            .aria_label(format!("Conversation with {}", self.agent.name))
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(t.bg_editor)
            .children(backdrop)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(self.header(&t, cx))
                    .child(
                        div()
                            .id("agent-conversation")
                            .role(Role::Log)
                            .aria_label("Conversation")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(32.))
                            .py(px(16.))
                            .child(
                                div()
                                    .mx_auto()
                                    .w_full()
                                    .max_w(px(COLUMN))
                                    .flex()
                                    .flex_col()
                                    .gap(px(16.))
                                    .child(self.greeting(&t))
                                    .when(self.offers_start(), |d| {
                                        d.child(self.start_card(&t, cx))
                                    }),
                            ),
                    )
                    .child(self.composer(cx)),
            )
            .children(details)
            .into_any_element()
    }
}

impl AgentPage {
    /// The agent's name in a chip at the top, and the button that shows its details.
    fn header(&self, t: &Theme, cx: &mut Context<Self>) -> Div {
        div()
            .relative()
            .flex_none()
            .h(px(52.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("agent-title")
                    .role(Role::Heading)
                    .aria_level(1)
                    .aria_label(self.agent.name.clone())
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(34.))
                    .pl(px(6.))
                    .pr(px(14.))
                    .rounded_full()
                    .bg(t.bg_hover)
                    .child(avatar(&self.agent.id, &self.agent.name, Mood::Idle, 22.))
                    .child(
                        div()
                            .text_size(px(fs_md()))
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.agent.name.clone()),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .right(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .children(self.find_button(t, cx))
                    .child(self.open_button(t, cx))
                    .child(self.details_button(t, cx)),
            )
    }

    /// The chat's search button, which finds in the agent's newest conversation. None while it
    /// has none.
    fn find_button(&self, t: &Theme, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        self.usage.as_ref()?.runs.first()?;
        let hover = t.bg_hover;
        Some(
            button("agent-find-toggle", "Find in conversation")
                .flex_none()
                .size(px(28.))
                .justify_center()
                .rounded(px(radius()))
                .border_1()
                .border_color(t.border)
                .bg(t.bg_panel)
                .shadow(header_shadow())
                .hover(move |s| s.bg(hover))
                .tooltip(tooltip("Find in conversation"))
                .child(icon("magnifying-glass", 14.))
                .on_click(cx.listener(|this, _, window, cx| this.find_in_newest(window, cx))),
        )
    }

    /// The button that shows and hides the agent's details, drawn like the header's others.
    fn details_button(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = self.details_open;
        let hover = t.bg_hover;
        button("agent-details-toggle", "Agent details")
            .aria_expanded(open)
            .flex_none()
            .size(px(28.))
            .justify_center()
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .shadow(header_shadow())
            .hover(move |s| s.bg(hover))
            .tooltip(tooltip("Agent details"))
            .child(icon("gear-six", 14.))
            .on_click(cx.listener(|this, _, _, cx| this.toggle_details(cx)))
    }

    /// The chat's Open in editor button, which opens where the next run works.
    fn open_button(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let hover = t.bg_hover;
        button("agent-open-in-editor", "Open in editor")
            .flex_none()
            .h(px(28.))
            .gap(px(6.))
            .px(px(12.))
            .rounded(px(radius()))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .text_size(px(fs_sm()))
            .shadow(header_shadow())
            .hover(move |s| s.bg(hover))
            .child(icon("code", 14.))
            .child("Open in editor")
            .on_click(cx.listener(|this, _, _, cx| {
                let (core, id, place) = (
                    std::sync::Arc::clone(&this.core),
                    this.agent.id.clone(),
                    this.run_place(),
                );
                cx.background_spawn(async move {
                    if let Err(e) = core.open_agent_in_editor(&id, place.as_deref()) {
                        log::warn!(error:% = e; "could not open the editor: {{error}}");
                    }
                })
                .detach();
            }))
    }

    /// The agent saying hello, as the first message of the conversation.
    fn greeting(&self, t: &Theme) -> Div {
        div()
            .flex()
            .items_end()
            .gap(px(8.))
            .child(avatar(&self.agent.id, &self.agent.name, Mood::Idle, 40.))
            .child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(px(16.))
                    .bg(t.bubble)
                    .text_size(px(fs_md()))
                    .child(format!(
                        "Hi, I'm {}. What would you like me to do?",
                        self.agent.name
                    )),
            )
    }

    /// The roles the agent can start in, each picked by a click or its letter.
    fn start_card(&self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let hover = t.bg_hover;
        let mut list = div()
            .id("agent-starts")
            .role(Role::List)
            .aria_label("Starting points")
            .flex()
            .flex_col()
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let key = e.keystroke.key.to_ascii_uppercase();
                let mut chars = key.chars();
                let (Some(c), None) = (chars.next(), chars.next()) else {
                    return;
                };
                if let Some(index) = (0..STARTING_POINTS.len()).find(|&i| start_key(i) == c) {
                    cx.stop_propagation();
                    this.pick_start(index, window, cx);
                }
            }));
        for (index, point) in STARTING_POINTS.iter().enumerate() {
            let key = start_key(index);
            list = list.child(
                button(
                    SharedString::from(format!("agent-start-{index}")),
                    format!("{}, {key}", point.label),
                )
                .role(Role::ListItem)
                .justify_between()
                .h(px(36.))
                .px(px(10.))
                .rounded(px(radius_sm()))
                .hover(move |s| s.bg(hover))
                .text_size(px(fs_md()))
                .child(point.label)
                .child(
                    div()
                        .text_size(px(fs_sm()))
                        .text_color(t.text_muted)
                        .child(key.to_string()),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.pick_start(index, window, cx);
                })),
            );
        }
        div()
            .id("agent-start")
            .role(Role::Region)
            .aria_label("Pick a starting point")
            .ml(px(36.))
            .max_w(px(520.))
            .p(px(12.))
            .rounded(px(radius_lg()))
            .bg(t.bg_panel)
            .border_1()
            .border_color(t.border)
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pl(px(10.))
                    .child(
                        div()
                            .text_size(px(fs_lg()))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Pick a starting point"),
                    )
                    .child(
                        icon_button("agent-start-close", "Close starting points", "x", 16., cx)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_start(window, cx);
                            })),
                    ),
            )
            .child(list)
    }

    /// Where the run works, when the agent may work in more than one project, over the composer
    /// every chat has, which starts the agent's runs here.
    fn composer(&mut self, cx: &mut Context<Self>) -> Div {
        let t = *theme(cx);
        let linked = self.linked_projects();
        let place = self.run_place();
        let mut places = radio_group("agent-run-in", "Where it works");
        for (id, name) in &linked {
            let checked = place.as_ref() == Some(id);
            let click = id.clone();
            places = places.child(
                radio(
                    format!("agent-run-in-{id}").into(),
                    name.clone().into(),
                    checked,
                    &t,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_run_in(Some(click.clone()), cx);
                })),
            );
        }
        let composer = self
            .workspace
            .upgrade()
            .map(|w| w.read(cx).composer.clone());
        let starting = self.starting.then(|| {
            div()
                .id("agent-starting")
                .role(Role::Status)
                .aria_label("Starting the conversation")
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(spinner(14., t.text_muted))
                .child("Starting")
        });
        div()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(24.))
            .pb(px(16.))
            .when(linked.len() > 1, |d| d.child(places))
            .children(starting)
            .children(composer)
    }
}
