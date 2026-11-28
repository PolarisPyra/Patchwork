//! The chat drawn: its transcript or its empty state, and what sits around the composer.

// Rust guideline compliant 2026-11-27

use gpui::component::button::Button;
use gpui::component::button::ButtonVariants as _;
use gpui::component::Sizable as _;
use crate::prelude::*;
use patchwork_client::GitModel;
use patchwork_client::AppState;
use patchwork_views_logos::agent_logo;
use gpui::{
    AnyElement, Div, FontWeight, Role, SharedString, Window, div, prelude::*, px, Entity,
    FocusHandle, Subscription, WeakEntity,
};
use patchwork_design_icons::IconName;
use patchwork_design_theme::{InScheme, Theme, fs_sm, fs_xl, radius_sm, theme};
use patchwork_design_components::button;
use patchwork_design_text::display_title;

use crate::panes::PaneId;
use super::transcript::message_list::{follow_width, sync_transcript, transcript};
use super::{backdrop, tabs};
use super::transcript::reply::background_tasks;
use super::top_right::buttons::header;
use super::above_composer::{
    missing_folder, goal_bar, in_terminal, plan_steps, agent_questions, queued_messages,
};
use patchwork_design_theme::in_scheme;
use crate::top_right::find_bar::FindState;

/// How long a chat just chosen waits for the files under its last replies before it shows anyway.
pub const OPEN_HOLD: std::time::Duration = std::time::Duration::from_millis(250);

/// The last messages whose files a chat just chosen waits for: about a screenful.
pub(super) const HELD_MESSAGES: usize = 10;

/// The focused pane `pane`: the open chat whole, with its tabs over it.
pub fn focused_chat<W: ChatWindow>(
    w: &mut W,
    v: &mut ChatView<W>,
    pane: PaneId,
    window: &mut Window,
    cx: &mut Context<W>,
) -> AnyElement {
    patchwork_design_text::selection::begin_frame(cx);
    w.load_reply_files(cx);
    w.sync_secret_fields(cx);
    w.load_checkpoints(cx);
    w.load_image_sizes(cx);
    w.sync_chat_draft(cx);
    // Over a background drawn for a scheme, the chat is drawn as the Orchestrator page is.
    let scheme = backdrop::chat_scheme(w, cx);
    let _scope = scheme.map(InScheme::enter);
    let t = *theme(cx);
    // A reply seen streaming keeps its steps open once it settles.
    let ui: &mut crate::ChatUi = w;
    if let Some(chat) = &ui.chat {
        for m in &chat.messages {
            if m.is_streaming() {
                ui.transcript.streamed.insert(m.id.clone());
            }
        }
    }
    // The files under a reply load after it first draws and add rows beneath it, so a chat just
    // chosen waits for its last replies' files rather than jumping up as they arrive.
    let held = w.opened_at.elapsed() < OPEN_HOLD
        && w.chat.as_ref().is_some_and(|chat| {
            w.transcript.rows.0.as_deref() != Some(chat.id.as_str())
                && chat.messages.iter().rev().take(HELD_MESSAGES).any(|m| {
                    GitModel::read(cx)
                        .reply_files
                        .get(&m.id)
                        .is_some_and(|f| f.loading)
                })
        });
    if !held {
        sync_transcript(w, cx);
    }
    follow_width(w, window, cx);
    let body = match &w.chat {
        Some(chat) if held && !chat.messages.is_empty() => div().flex_1().into_any_element(),
        Some(chat) if !chat.messages.is_empty() => {
            let marks = v.find_marks(chat, cx);
            transcript(w, marks, None, chat, &t, window, cx).into_any_element()
        }
        _ => empty_chat(w, &t, cx).into_any_element(),
    };
    let is_new = w.chat.as_ref().is_none_or(|t| t.messages.is_empty());
    let background = background_tasks::background_tasks(w, &t, cx);
    let steps = w.plan_steps(cx);
    let plan = (!steps.is_empty()).then(|| plan_steps::plan_panel(w, &steps, &t, cx));
    let banner = w.approval_banner(cx);
    w.shown_choice = None;
    let prompt = w
        .prompt(cx)
        .map(|p| agent_questions::choice_panel(w, p, &t, cx));
    let queued = queued_messages::queued_strip(w, &t, cx);
    let goal = goal_bar::goal_banner(w, &t, cx);
    // The composer shows the worktree's status on the line above it.
    w.load_worktree(cx);
    w.load_worktree_told(cx);
    let missing = missing_folder::missing_folder(w, &t, cx);
    let away = in_terminal::in_terminal(w, &t, cx);
    let parent = subagent_footer(w, &t, cx);
    let header = (!is_new).then(|| header(w, v, &t, window, cx));
    w.reveal_find(v, window, cx);
    let chat = div()
        .id("chat")
        .role(Role::Main)
        .aria_label("Chat")
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .text_color(t.text)
        .when(is_new, gpui::Styled::justify_center)
        .child(body)
        .children(header)
        .when_some(parent, gpui::ParentElement::child)
        .when(w.composer_shown(), |c| {
            c.child(
                div()
                    .w_full()
                    .flex_none()
                    .px(px(24.))
                    .pb(px(16.))
                    .children(background)
                    .children(plan)
                    .children(banner)
                    .children(missing)
                    .children(away)
                    .children(prompt)
                    .children(queued)
                    .children(goal)
                    .child(w.composer.clone()),
            )
        });
    let chat = match tabs::tab_strip(w, pane, &t, cx) {
        Some(strip) => div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(strip)
            .child(chat)
            .into_any_element(),
        None => chat.into_any_element(),
    };
    match scheme {
        Some(scheme) => in_scheme(scheme, chat).into_any_element(),
        None => chat.into_any_element(),
    }
}

/// A subagent's chat is its transcript; it takes no messages. Nor does a chat looked in on from
/// the orchestrator page, which takes its replies through the pill there.
pub(super) fn subagent_footer<W: ChatWindow>(w: &W, t: &Theme, cx: &mut Context<W>) -> Option<Div> {
    let hover = t.bg_hover;
    let chat = w.chat.as_ref()?;
    if w.peeking && chat.parent.is_none() {
        let back = Button::new("peek-back")
            .ghost()
            .small()
            .icon(IconName::ArrowLeft)
            .accessibility_label("Back to the orchestrator")
            .flex_none()
            .on_click(cx.listener(|w, _, _, cx| w.back_to_orchestrator(cx)));
        let link = button("peek-orchestrator", "Orchestrator")
            .flex_none()
            .rounded(px(radius_sm()))
            .px(px(4.))
            .text_color(t.text)
            .hover(move |s| s.bg(hover))
            .child("Orchestrator")
            .on_click(cx.listener(|w, _, _, cx| w.back_to_orchestrator(cx)));
        return Some(read_only_bar(t, Some(back), "Opened from", link));
    }
    let parent = chat.parent.as_ref()?;
    let found = AppState::get(cx)
        .chats
        .iter()
        .find(|p| p.id == parent.chat_id);
    let title = found.map_or_else(|| "a deleted chat".to_string(), |p| display_title(&p.title));
    let link = button("subagent-parent", title.clone())
        .min_w_0()
        .truncate()
        .rounded(px(radius_sm()))
        .px(px(4.))
        .text_color(t.text)
        .child(title);
    let link = match found.map(|p| p.id.clone()) {
        Some(id) => link
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(move |w, _, _, cx| w.open_chat(&id, cx))),
        None => link.cursor_default(),
    };
    // Back to the chat shown before, or to the parent when the subagent was opened first.
    let parent_id = found.map(|p| p.id.clone());
    let back = (w.can_go_back() || parent_id.is_some()).then(|| {
        Button::new("subagent-back")
            .ghost()
            .small()
            .icon(IconName::ArrowLeft)
            .accessibility_label("Go back")
            .flex_none()
            .on_click(cx.listener(move |w, _, _, cx| {
                if w.can_go_back() {
                    w.go_back(cx);
                } else if let Some(id) = &parent_id {
                    w.open_chat(id, cx);
                }
            }))
    });
    Some(read_only_bar(t, back, "Subagent of", link))
}

/// The bar in place of the composer: a back button, where the chat was opened from, and that it
/// takes no messages here.
fn read_only_bar(
    t: &Theme,
    back: Option<impl IntoElement>,
    from: &'static str,
    link: impl IntoElement,
) -> Div {
    div()
        .flex()
        .w_full()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .border_t_1()
        .border_color(t.border)
        .px(px(24.))
        .py(px(10.))
        .text_size(px(fs_sm()))
        .text_color(t.text_muted)
        .children(back)
        .child(div().flex_none().child(from))
        .child(link)
        .child(
            div()
                .ml_auto()
                .flex_none()
                .text_color(t.text_subtle)
                .child("Read-only"),
        )
}

/// What a chat shows before its first message.
pub(super) fn empty_chat<W: ChatWindow>(w: &W, t: &Theme, cx: &gpui::App) -> Div {
    let agent = w
        .chat
        .as_ref()
        .map_or_else(|| AppState::get(cx).settings.default_agent(), |t| t.agent);
    let project = w.project(cx);
    let suggestions: &[&str] = if project.is_some() {
        &[
            "Explain this codebase",
            "Find and fix a bug",
            "Write tests for…",
        ]
    } else {
        &["Explain a concept", "Draft a script", "Review a snippet"]
    };
    let hover = t.bg_hover;
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(16.))
        .p(px(24.))
        .child(agent_logo(
            agent,
            w.chat.as_ref().and_then(|t| t.model.as_deref()),
            40.,
        ))
        .child(
            div()
                .text_size(px(fs_xl()))
                .font_weight(FontWeight::SEMIBOLD)
                .child(match project {
                    Some(p) => format!("What should we build in {}?", p.name),
                    None => "What's on your mind?".into(),
                }),
        )
        .when(project.is_none(), |c| {
            c.child(
                div().mt(px(-6.)).text_color(t.text_muted).child(
                    "This chat has no project; the agent works in a private scratch folder.",
                ),
            )
        })
        .child(
            div()
                .flex()
                .flex_wrap()
                .justify_center()
                .max_w(px(560.))
                .gap(px(8.))
                .children(suggestions.iter().map(|s| {
                    let composer = w.composer.clone();
                    // Written into the composer to finish or send, with what is attached kept.
                    let text = s.trim_end_matches('…');
                    button(SharedString::from(*s), *s)
                        .on_click(move |_, window, cx| {
                            composer.update(cx, |c, cx| {
                                let attached = c.draft(cx).1;
                                c.set_draft(text, attached, cx);
                                c.focus(window, cx);
                            });
                        })
                        .h(px(28.))
                        .rounded(px(14.))
                        .border_1()
                        .border_color(t.border_strong)
                        .bg(t.bg_panel)
                        .px(px(12.))
                        .hover(move |st| st.bg(hover))
                        .child(*s)
                })),
        )
}

/// One chat on screen and the state of its view.
pub struct ChatView<W: ChatWindow> {
    pub workspace: WeakEntity<W>,
    /// Where the keys go back to when nothing in the chat had them.
    pub home_focus: FocusHandle,
    pub find: FindState,
    _changes: Subscription,
}

impl<W: ChatWindow> ChatView<W> {
    /// A view drawing from `workspace`, redrawn whenever it changes.
    pub fn new(workspace: &Entity<W>, home_focus: FocusHandle, cx: &mut Context<Self>) -> Self {
        Self {
            workspace: workspace.downgrade(),
            home_focus,
            find: FindState::default(),
            _changes: cx.observe(workspace, |_, _, cx| cx.notify()),
        }
    }
}

impl<W: ChatWindow> Render for ChatView<W> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.clone();
        let body = workspace
            .update(cx, |w, cx| crate::panes::panes(w, self, window, cx))
            .ok();
        div().size_full().flex().children(body)
    }
}
