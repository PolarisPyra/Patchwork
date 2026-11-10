//! What the agent waits on above the composer: its questions, and the options a reply offers in a
//! ```choices``` fence.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;

use gpui::{
    Div, Focusable, FontWeight, Keystroke, Role, SharedString, Stateful, Window, div, prelude::*,
    px,
};
use patchwork_server::threads::{ApprovalStatus, MessageStatus, Role as MessageRole};

use crate::ui::theme::{FS_MD, FS_SM, FS_XS, RADIUS, RADIUS_SM, Theme};
use crate::ui::button;
use crate::workspace::{Workspace, is_working};

/// Most options a ```choices``` fence offers.
const MAX_CHOICES: usize = 9;

/// The answers given so far to a request's questions, and whether the shown one is being written in
/// the composer instead of picked.
pub struct Answering {
    id: String,
    answers: HashMap<String, Vec<String>>,
    in_composer: bool,
}

/// What the panel shows: a question the agent asked, or the options of the last reply.
pub struct Prompt {
    /// The request or reply it belongs to; the highlighted row starts over when it changes.
    key: String,
    question: Option<String>,
    options: Vec<String>,
    descriptions: Vec<String>,
    /// A click or number key only highlights an option, and a second one sends it.
    confirm: bool,
}

/// The options in a reply's last ```choices``` fence: a JSON list of strings.
fn parse_choices(text: &str) -> Option<Vec<String>> {
    let start = text.rfind("```choices")?;
    let body = &text[start + "```choices".len()..];
    let body = body.trim_start_matches([' ', '\t']).strip_prefix('\n')?;
    let end = body.find("\n```")?;
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&body[..end]).ok()?;
    let options: Vec<String> = parsed
        .into_iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .filter(|o| !o.trim().is_empty())
        .take(MAX_CHOICES)
        .collect();
    (!options.is_empty()).then_some(options)
}

impl Workspace {
    /// The request whose questions the agent is waiting on. A secret is entered in its own card.
    fn asking(&self) -> Option<&patchwork_server::threads::Approval> {
        self.thread
            .as_ref()?
            .messages
            .iter()
            .flat_map(|m| &m.approvals)
            .find(|a| {
                a.status == ApprovalStatus::Pending && a.secret.is_none() && !a.questions.is_empty()
            })
    }

    fn answered(&self, id: &str) -> HashMap<String, Vec<String>> {
        self.answering
            .as_ref()
            .filter(|a| a.id == id)
            .map(|a| a.answers.clone())
            .unwrap_or_default()
    }

    /// The answer to the agent's question is being written in the composer, which sends it there.
    pub fn answering_in_composer(&self) -> bool {
        let Some(asking) = self.asking() else {
            return false;
        };
        self.answering
            .as_ref()
            .is_some_and(|a| a.id == asking.id && a.in_composer)
    }

    /// The reply whose options show: the last one, settled, while nothing runs.
    fn choices(&self) -> Option<(String, Vec<String>)> {
        let thread = self.thread.as_ref()?;
        let last = thread.messages.last()?;
        if is_working(thread)
            || last.role != MessageRole::Assistant
            || last.status != MessageStatus::Complete
            || self.dismissed_choices.contains(&last.id)
        {
            return None;
        }
        Some((last.id.clone(), parse_choices(&last.text)?))
    }

    /// The panel above the composer, if anything waits on an answer. Remembers which reply's
    /// options it shows, since leaving the chat with them open counts as dismissing them.
    pub fn prompt(&mut self) -> Option<Prompt> {
        if let Some(asking) = self.asking() {
            let answered = self.answered(&asking.id);
            let question = asking
                .questions
                .iter()
                .find(|q| !answered.contains_key(&q.id));
            if let Some(q) = question.filter(|_| !self.answering_in_composer()) {
                return Some(Prompt {
                    key: format!("{}-{}", asking.id, q.id),
                    question: Some(q.question.clone()),
                    options: q.options.iter().map(|o| o.label.clone()).collect(),
                    descriptions: q.options.iter().map(|o| o.description.clone()).collect(),
                    confirm: self.settings.confirm_answers,
                });
            }
        }
        let (id, options) = self.choices()?;
        self.shown_choice = Some(id.clone());
        Some(Prompt {
            key: id,
            question: None,
            options,
            descriptions: Vec::new(),
            confirm: false,
        })
    }

    /// Records the answer to the shown question, and sends them all once the last is answered.
    pub fn answer_question(&mut self, answer: String, cx: &mut Context<Self>) {
        let Some(asking) = self.asking().cloned() else {
            return;
        };
        let mut answers = self.answered(&asking.id);
        let Some(question) = asking
            .questions
            .iter()
            .find(|q| !answers.contains_key(&q.id))
        else {
            return;
        };
        answers.insert(question.id.clone(), vec![answer]);
        if asking.questions.iter().all(|q| answers.contains_key(&q.id)) {
            self.answering = None;
            self.respond_question(&asking.id, true, answers, cx);
        } else {
            self.answering = Some(Answering {
                id: asking.id,
                answers,
                in_composer: false,
            });
        }
        self.choice_active = None;
        cx.notify();
    }

    fn respond_question(
        &mut self,
        id: &str,
        allow: bool,
        answers: HashMap<String, Vec<String>>,
        cx: &mut Context<Self>,
    ) {
        let Some(thread_id) = self.thread.as_ref().map(|t| t.id.clone()) else {
            return;
        };
        if let Err(e) = self
            .server
            .respond_approval(&thread_id, id, allow, answers, None)
        {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }

    /// Sends an option: the answer to the agent's question, or the next message.
    fn pick(&mut self, option: String, cx: &mut Context<Self>) {
        if self.asking().is_some() {
            return self.answer_question(option, cx);
        }
        if let Some((id, _)) = self.choices() {
            self.dismissed_choices.insert(id);
        }
        self.choice_active = None;
        self.send(option, Vec::new(), None, None, cx);
    }

    /// "Submit my own response": the composer takes the answer.
    fn write_own(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(asking) = self.asking() {
            let (id, answers) = (asking.id.clone(), self.answered(&asking.id));
            self.answering = Some(Answering {
                id,
                answers,
                in_composer: true,
            });
        } else if let Some((id, _)) = self.choices() {
            self.dismissed_choices.insert(id);
        }
        self.choice_active = None;
        self.composer.update(cx, |c, cx| c.focus(window, cx));
        cx.notify();
    }

    /// Escape: the question is declined, or the options put away.
    fn dismiss_prompt(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.asking().map(|a| a.id.clone()) {
            self.answering = None;
            self.respond_question(&id, false, HashMap::new(), cx);
        } else if let Some((id, _)) = self.choices() {
            self.dismissed_choices.insert(id);
        }
        self.choice_active = None;
        cx.notify();
    }

    /// The panel takes its keys before the composer's own bindings: Escape, the arrows and Enter,
    /// and the number of a row. Another field being typed in keeps its keys.
    pub fn on_prompt_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let m = &keystroke.modifiers;
        if m.control || m.alt || m.shift || m.platform {
            return;
        }
        let composer = self.composer.read(cx).editor.focus_handle(cx);
        if window
            .focused(cx)
            .is_some_and(|f| f != composer && f != self.focus)
        {
            return;
        }
        let Some(prompt) = self.prompt() else {
            return;
        };
        let own = prompt.options.len();
        let current = self
            .choice_active
            .as_ref()
            .filter(|(key, _)| key == &prompt.key)
            .map(|(_, i)| *i);
        match keystroke.key.as_str() {
            "escape" => self.dismiss_prompt(cx),
            key @ ("down" | "up") => {
                let rows = own + 1;
                let next = match current {
                    None if key == "down" => 0,
                    None => rows - 1,
                    Some(i) if key == "down" => (i + 1) % rows,
                    Some(i) => (i + rows - 1) % rows,
                };
                self.choice_active = Some((prompt.key, next));
                cx.notify();
            }
            "enter" => match current {
                Some(i) if i == own => self.write_own(window, cx),
                Some(i) => self.pick(prompt.options[i].clone(), cx),
                None => return,
            },
            key => {
                let Some(n) = key
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=own + 1).contains(n))
                else {
                    return;
                };
                if prompt.confirm && current != Some(n - 1) {
                    self.choice_active = Some((prompt.key, n - 1));
                    cx.notify();
                } else if n - 1 == own {
                    self.write_own(window, cx);
                } else {
                    self.pick(prompt.options[n - 1].clone(), cx);
                }
            }
        }
        cx.stop_propagation();
    }
}

/// Numbered options above the composer. A click or number key sends an option, and the arrows
/// highlight one for Enter; with `confirm`, a click or number key highlights it and a second sends
/// it. The last row writes the answer instead of sending.
pub fn choice_panel(
    w: &Workspace,
    prompt: Prompt,
    t: &Theme,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let active = w
        .choice_active
        .as_ref()
        .filter(|(key, _)| key == &prompt.key)
        .map(|(_, i)| *i);
    let own = prompt.options.len();
    let (hover, selected) = (t.bg_hover, t.bg_selected);
    let row = |i: usize, label: String| {
        button(
            SharedString::from(format!("choice-{}-{i}", prompt.key)),
            label,
        )
        .role(Role::ListBoxOption)
        .aria_selected(active == Some(i))
        .w_full()
        .gap(px(12.))
        .rounded(px(RADIUS_SM))
        .px(px(10.))
        .py(px(6.))
        .text_size(px(FS_MD))
        .text_color(t.text)
        .map(|r| {
            if active == Some(i) {
                r.bg(selected)
            } else {
                r.hover(move |s| s.bg(hover))
            }
        })
        .child(kbd(i + 1, t))
    };
    let rows = prompt
        .options
        .iter()
        .enumerate()
        .map(|(i, option)| {
            let (key, option, confirm) = (prompt.key.clone(), option.clone(), prompt.confirm);
            row(i, option.clone())
                .child(div().flex_1().min_w_0().truncate().child(option.clone()))
                .children(
                    prompt
                        .descriptions
                        .get(i)
                        .filter(|d| !d.is_empty())
                        .map(|d| {
                            div()
                                .flex_grow(2.)
                                .flex_basis(px(0.))
                                .min_w_0()
                                .truncate()
                                .text_size(px(FS_SM))
                                .text_color(t.text_muted)
                                .child(d.clone())
                        }),
                )
                .on_click(cx.listener(move |w, _, _, cx| {
                    let current = w
                        .choice_active
                        .as_ref()
                        .filter(|(k, _)| k == &key)
                        .map(|(_, i)| *i);
                    if confirm && current != Some(i) {
                        w.choice_active = Some((key.clone(), i));
                        cx.notify();
                    } else {
                        w.pick(option.clone(), cx);
                    }
                }))
        })
        .collect::<Vec<_>>();
    div()
        .id("choices")
        .role(Role::ListBox)
        .aria_label("Choose a reply")
        .mb(px(6.))
        .flex()
        .flex_col()
        .gap(px(1.))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(t.border)
        .bg(t.bg_panel)
        .p(px(4.))
        .children(prompt.question.map(|q| {
            div()
                .px(px(10.))
                .pt(px(4.))
                .pb(px(6.))
                .text_size(px(FS_MD))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.text)
                .child(q)
        }))
        .children(rows)
        .child(
            row(own, "Submit my own response".into())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(t.text_muted)
                        .child("Submit my own response"),
                )
                .on_click(cx.listener(|w, _, window, cx| w.write_own(window, cx))),
        )
}

/// `.kbd`: a key's label in a small tile.
fn kbd(n: usize, t: &Theme) -> Div {
    div()
        .flex_none()
        .min_w(px(20.))
        .h(px(20.))
        .px(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .bg(t.bg_selected_muted)
        .text_color(t.text_muted)
        .text_size(px(FS_XS))
        .font_weight(FontWeight::MEDIUM)
        .child(n.to_string())
}
