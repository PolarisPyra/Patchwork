//! The chat drawn: its transcript or its empty state, and what sits around the composer.

// Rust guideline compliant 2026-10-09

use gpui::{Div, FontWeight, Role, SharedString, Window, div, prelude::*, px};
use patchwork_server::threads::{MessageStatus, Thread};
use crate::ui::theme::{FS_SM, FS_XL, FS_XS, RADIUS, RADIUS_SM, Theme, theme};
use crate::ui::{agent_logo, button, display_title, format_tokens, icon, tooltip};
use crate::workspace::Workspace;
use super::transcript::{sync_transcript, transcript};
use crate::ui::backdrop;
use super::background;
use super::find;
use super::folder;
use super::goal;
use super::lineage;
use super::plan;
use super::prompts;
use super::queue;
use super::resume;

/// How long a chat just chosen waits for the files under its last replies before it shows anyway.
pub const OPEN_HOLD: std::time::Duration = std::time::Duration::from_millis(250);

/// The last messages whose files a chat just chosen waits for: about a screenful.
pub(super) const HELD_MESSAGES: usize = 10;

pub fn render(
    w: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement + use<> {
    crate::ui::selection::begin_frame(cx);
    w.load_reply_files(cx);
    w.sync_secret_fields(cx);
    w.load_checkpoints(cx);
    w.load_image_sizes(cx);
    w.sync_chat_draft(cx);
    let t = *theme(cx);
    // A reply seen streaming keeps its steps open once it settles.
    if let Some(thread) = &w.thread {
        for m in &thread.messages {
            if m.status == MessageStatus::Streaming {
                w.streamed.insert(m.id.clone());
            }
        }
    }
    // The files under a reply load after it first draws and add rows beneath it, so a chat just
    // chosen waits for its last replies' files rather than jumping up as they arrive.
    let held = w.opened_at.elapsed() < OPEN_HOLD
        && w.thread.as_ref().is_some_and(|thread| {
            w.transcript_rows.0.as_deref() != Some(thread.id.as_str())
                && thread
                    .messages
                    .iter()
                    .rev()
                    .take(HELD_MESSAGES)
                    .any(|m| w.reply_files.get(&m.id).is_some_and(|f| f.loading))
        });
    if !held {
        sync_transcript(w);
    }
    let body = match &w.thread {
        Some(thread) if held && !thread.messages.is_empty() => div().flex_1().into_any_element(),
        Some(thread) if !thread.messages.is_empty() => {
            transcript(w, thread, &t, window, cx).into_any_element()
        }
        _ => empty_chat(w, &t).into_any_element(),
    };
    let is_new = w.thread.as_ref().is_none_or(|t| t.messages.is_empty());
    let background = background::background_tasks(w, &t, cx);
    let steps = w.plan_steps(cx);
    let plan = (!steps.is_empty()).then(|| plan::plan_panel(w, &steps, &t, cx));
    let ready = w.ready_plan().map(|p| plan::ready_bar(&p, &t, cx));
    let banner = w.approval_banner(cx);
    w.shown_choice = None;
    let prompt = w.prompt().map(|p| prompts::choice_panel(w, p, &t, cx));
    let queued = queue::queued_strip(w, &t, cx);
    let goal = goal::goal_banner(w, &t, cx);
    let missing = folder::missing_folder(w, &t, cx);
    let resume = resume::resume_prompt(w, &t, cx);
    let backdrop = backdrop::chat_backdrop(w, &t, cx);
    let parent = subagent_footer(w, &t, cx);
    w.reveal_find(window, cx);
    div()
        .id("chat")
        .role(Role::Main)
        .aria_label("Chat")
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .bg(t.bg_editor)
        .children(backdrop)
        .when(is_new, gpui::Styled::justify_center)
        .child(body)
        .when_some(parent, gpui::ParentElement::child)
        .when(w.thread.as_ref().is_none_or(|t| t.parent.is_none()), |c| {
            c.child(
                div()
                    .w_full()
                    .flex_none()
                    .px(px(24.))
                    .pb(px(16.))
                    .children(background)
                    .children(plan)
                    .children(ready)
                    .children(banner)
                    .children(missing)
                    .children(resume)
                    .children(prompt)
                    .children(queued)
                    .children(goal)
                    .child(w.composer.clone()),
            )
        })
        .when(!is_new, |c| c.child(floating(w, &t, window, cx)))
}

/// A subagent's thread is its transcript; it takes no messages.
pub(super) fn subagent_footer(
    w: &Workspace,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Option<Div> {
    let parent = w.thread.as_ref()?.parent.as_ref()?;
    let found = w.threads.iter().find(|p| p.id == parent.thread_id);
    let title = found.map_or_else(
        || "a deleted thread".to_string(),
        |p| display_title(&p.title),
    );
    let hover = t.bg_hover;
    let link = button("subagent-parent", title.clone())
        .min_w_0()
        .truncate()
        .rounded(px(RADIUS_SM))
        .px(px(4.))
        .text_color(t.text)
        .child(title);
    let link = match found.map(|p| p.id.clone()) {
        Some(id) => link
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(move |w, _, _, cx| w.open_thread(&id, cx))),
        None => link.cursor_default(),
    };
    Some(
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
            .text_size(px(FS_SM))
            .text_color(t.text_muted)
            .child(div().flex_none().child("Subagent of"))
            .child(link)
            .child(
                div()
                    .ml_auto()
                    .flex_none()
                    .text_color(t.text_subtle)
                    .child("Read-only"),
            ),
    )
}

/// Search, Android devices and "Open in editor", over the transcript's top right.
pub(super) fn floating(
    w: &mut Workspace,
    t: &Theme,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Div {
    let lineage = lineage::lineage_card(w, t, window, cx);
    let hover = t.bg_hover;
    let open = w.project().map(|p| p.path.clone()).map(|root| {
        button("open-in-editor", "Open in editor")
            .h(px(28.))
            .gap(px(6.))
            .px(px(12.))
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .text_size(px(FS_SM))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.12),
                offset: gpui::point(px(0.), px(1.)),
                blur_radius: px(3.),
                spread_radius: px(0.),
                inset: false,
            }])
            .hover(move |s| s.bg(hover))
            .child(icon("code", 14.))
            .child("Open in editor")
            .on_click(cx.listener(move |_, _, _, cx| {
                let root = root.clone();
                cx.background_spawn(async move {
                    if let Err(e) = patchwork_server::workspace::open_in_editor(&root, "", None) {
                        log::warn!(error:% = e; "could not open the editor: {{error}}");
                    }
                })
                .detach();
            }))
    });
    let android = w.mobile.is_some().then(|| {
        let shown = w.android_open(cx);
        let fg = t.text;
        button("android-devices", "Android devices")
            .aria_expanded(shown)
            .flex_none()
            .size(px(28.))
            .justify_center()
            .rounded(px(RADIUS))
            .border_1()
            .border_color(t.border)
            .bg(t.bg_panel)
            .text_color(t.text_muted)
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.12),
                offset: gpui::point(px(0.), px(1.)),
                blur_radius: px(3.),
                spread_radius: px(0.),
                inset: false,
            }])
            .hover(move |s| s.bg(hover).text_color(fg))
            .tooltip(tooltip("Android devices"))
            .child(icon("device-mobile", 14.))
            .on_click(cx.listener(|w, _, window, cx| w.toggle_android(window, cx)))
    });
    let bar = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(find::find_field(w, t, cx))
        .children(android)
        .children(open);
    div()
        .absolute()
        .top(px(12.))
        .right(px(16.))
        .flex()
        .flex_col()
        .items_end()
        .gap(px(8.))
        .child(bar)
        .children(lineage)
}

/// What a chat shows before its first message.
pub(super) fn empty_chat(w: &Workspace, t: &Theme) -> Div {
    let agent = w
        .thread
        .as_ref()
        .map(|t| t.agent)
        .or(w.settings.agents.first().copied())
        .unwrap_or(patchwork_server::provider::AgentKind::Claude);
    let project = w.project();
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
            w.thread.as_ref().and_then(|t| t.model.as_deref()),
            40.,
        ))
        .child(
            div()
                .text_size(px(FS_XL))
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
                    button(SharedString::from(*s), *s)
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

/// The chat's token use: totals over its replies, context from the last.
pub(crate) fn usage_line(thread: &Thread, warn_above: u64, t: &Theme) -> Div {
    let line = div()
        .mb(px(6.))
        .flex()
        .flex_wrap()
        .justify_end()
        .gap_x(px(12.))
        .pr(px(12.))
        .text_size(px(FS_XS))
        .line_height(px(16.));
    let Some(last) = thread.messages.iter().rev().find_map(|m| m.usage.as_ref()) else {
        return line.invisible().child("Context");
    };
    let (mut input, mut read, mut write, mut output) = (0, 0, 0, 0);
    for usage in thread.messages.iter().filter_map(|m| m.usage.as_ref()) {
        input += usage.input;
        read += usage.cache_read;
        write += usage.cache_write;
        output += usage.output;
    }
    let large = last.context > warn_above;
    line.text_color(if large { t.warning } else { t.text_muted })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .when(large, |d| d.child(icon("warning", 13.)))
                .child(if large {
                    format!("Context over {}", format_tokens(warn_above))
                } else {
                    format!("Context {}", format_tokens(last.context))
                }),
        )
        .child(format!("Cache read {} tokens", format_tokens(read)))
        .child(format!("Cache write {} tokens", format_tokens(write)))
        .child(format!("Input {} tokens", format_tokens(input)))
        .child(format!("Output {} tokens", format_tokens(output)))
        .children(last.first_token_ms.map(|ms| {
            if ms < 1000 {
                format!("First token {ms} ms")
            } else {
                format!("First token {:.1} s", ms as f64 / 1000.)
            }
        }))
        .children(last.output_per_second.map(|n| format!("{n} tokens/s")))
}
