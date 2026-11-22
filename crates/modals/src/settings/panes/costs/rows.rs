//! The Costs pane's tiles, and the rows of its breakdowns.

// Rust guideline compliant 2026-10-09

use gpui::{
    AnyElement, Div, ElementId, FontWeight, Hsla, Role, SharedString, Stateful, div, prelude::*,
    px, rgb,
};
use patchwork_core::provider::AgentKind;
use patchwork_core::costs::{ChatCost, CostFilter, ProviderCost, Tally};

use crate::settings::SettingsDialog;
use crate::settings::controls::list_box;
use crate::settings::dialog::SettingsEvent;
use patchwork_ui::theme::{Theme, fs_lg, fs_sm, fs_xs, radius};
use patchwork_ui::{agent_label, agent_logo, button, format_tokens};

/// `$12.34`, `$1,234`, or `<$0.01` for a cost too small to show.
pub(super) fn dollars(cost: f64) -> String {
    if cost <= 0. {
        "$0.00".into()
    } else if cost < 0.01 {
        "<$0.01".into()
    } else if cost < 1000. {
        format!("${cost:.2}")
    } else {
        format!("${}", grouped(cost.round() as u64))
    }
}

/// `33,271,711,671`.
pub(super) fn grouped(n: u64) -> String {
    let whole = n.to_string();
    let mut out = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// What a tally measures on the charts: its cost, or its tokens.
pub(super) fn value(tally: &Tally, by_cost: bool) -> f64 {
    if by_cost {
        tally.cost()
    } else {
        tally.tokens() as f64
    }
}

/// A charted value written out: `$12.34`, or `45.2k` tokens.
pub(super) fn amount(value: f64, by_cost: bool) -> String {
    if by_cost {
        dollars(value)
    } else {
        format_tokens(value as u64)
    }
}

/// One cell of a strip: a name, a figure, and a smaller second figure.
pub(super) struct Cell {
    pub(super) label: &'static str,
    pub(super) value: String,
    pub(super) detail: String,
}

/// A row of figures in one box, side by side.
pub(super) fn strip(
    id: &'static str,
    label: &'static str,
    cells: Vec<Cell>,
    t: &Theme,
) -> AnyElement {
    let border = t.border;
    div()
        .id(id)
        .role(Role::Group)
        .aria_label(label)
        .grid()
        .grid_cols(cells.len() as u16)
        .overflow_hidden()
        .rounded(px(radius()))
        .border_1()
        .border_color(border)
        .children(cells.into_iter().enumerate().map(|(i, c)| {
            div()
                .id(ElementId::Integer(i as u64))
                .role(Role::Group)
                .aria_label(SharedString::from(format!(
                    "{}: {}, {}",
                    c.label, c.value, c.detail
                )))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(2.))
                .min_w_0()
                .px(px(8.))
                .py(px(12.))
                .when(i > 0, |d| d.border_l_1().border_color(border))
                .child(
                    div()
                        .text_size(px(fs_lg()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(c.value),
                )
                .child(
                    div()
                        .text_size(px(fs_sm()))
                        .text_color(t.text_muted)
                        .child(c.label),
                )
                .child(
                    div()
                        .max_w_full()
                        .text_size(px(fs_xs()))
                        .text_color(t.text_subtle)
                        .truncate()
                        .child(c.detail),
                )
        }))
        .into_any_element()
}

/// A thin bar filled to `share` (0–1) of its width.
fn share_bar(share: f64, color: Hsla, t: &Theme) -> Div {
    div()
        .h(px(4.))
        .w_full()
        .rounded(px(2.))
        .bg(t.bg_hover)
        .child(
            div()
                .h_full()
                .rounded(px(2.))
                .bg(color)
                .w(gpui::relative(share.clamp(0., 1.) as f32)),
        )
}

/// `tally`'s share of `whole`: of the cost, or of the tokens when nothing has a cost.
fn share(tally: &Tally, whole: &Tally) -> f64 {
    if whole.cost() > 0. {
        tally.cost() / whole.cost()
    } else if whole.tokens() > 0 {
        tally.tokens() as f64 / whole.tokens() as f64
    } else {
        0.
    }
}

/// A name, its tokens, and its cost or "No price" when none of it has one.
fn line(name: String, tally: &Tally, t: &Theme) -> Div {
    let cost = if tally.unpriced < tally.tokens() {
        dollars(tally.cost())
    } else {
        "No price".into()
    };
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(div().flex_1().min_w_0().truncate().child(name))
        .child(
            div()
                .flex_none()
                .text_size(px(fs_sm()))
                .text_color(t.text_muted)
                .child(format!("{} tokens", format_tokens(tally.tokens()))),
        )
        .child(
            div()
                .flex_none()
                .w(px(80.))
                .text_right()
                .font_weight(FontWeight::MEDIUM)
                .when(tally.unpriced >= tally.tokens(), |d| {
                    d.text_color(t.text_subtle)
                })
                .child(cost),
        )
}

/// What a screen reader reads for a row.
fn spoken(name: &str, tally: &Tally) -> SharedString {
    let cost = if tally.unpriced < tally.tokens() {
        dollars(tally.cost())
    } else {
        "No price".into()
    };
    format!(
        "{name}: {cost}, {} tokens, {} replies",
        format_tokens(tally.tokens()),
        tally.replies
    )
    .into()
}

/// One row of a breakdown: a name, its tokens and cost, and a bar for its share of `whole`.
pub(super) fn row(id: usize, name: String, tally: &Tally, whole: &Tally, t: &Theme) -> AnyElement {
    div()
        .id(ElementId::Integer(id as u64))
        .role(Role::ListItem)
        .aria_label(spoken(&name, tally))
        .flex()
        .flex_col()
        .gap(px(6.))
        .px(px(12.))
        .py(px(8.))
        .child(line(name, tally, t))
        .child(share_bar(share(tally, whole), t.accent, t))
        .into_any_element()
}

/// A provider's colour in the charts, their legends and its row: Claude orange, Codex the text
/// colour (white on a dark theme), the API providers blue.
pub(super) fn series(agent: AgentKind, t: &Theme) -> Hsla {
    match agent {
        AgentKind::Claude => rgb(0x00d9_7757).into(),
        AgentKind::Codex => t.text,
        AgentKind::Api => rgb(0x004c_8df6).into(),
        AgentKind::Antigravity => t.syn_string,
        AgentKind::OpenCode => t.syn_keyword,
    }
}

/// `$0.12/reply · 45.2k tokens/reply`.
fn per_reply(tally: &Tally) -> String {
    let n = tally.replies.max(1);
    format!(
        "{}/reply · {} tokens/reply",
        dollars(tally.cost() / n as f64),
        format_tokens(tally.tokens() / n)
    )
}

/// `In:out 42:1 · 87% cached`: the prompt tokens to each output token, and the share of the
/// prompt read from the cache.
fn mix(tally: &Tally) -> String {
    let prompt = tally.input + tally.cache_read + tally.cache_write;
    let ratio = if tally.output == 0 {
        "In:out —".to_string()
    } else {
        let r = prompt as f64 / tally.output as f64;
        if r >= 10. {
            format!("In:out {r:.0}:1")
        } else {
            format!("In:out {r:.1}:1")
        }
    };
    format!("{ratio} · {:.0}% cached", tally.cache_share() * 100.)
}

/// A row's small second line.
fn detail(text: String, t: &Theme) -> Div {
    div()
        .text_size(px(fs_sm()))
        .text_color(t.text_subtle)
        .truncate()
        .child(text)
}

/// A breakdown row that narrows the page to `filter`, or back to every reply while it is the
/// filter set.
fn filter_row(
    ix: usize,
    label: String,
    filter: CostFilter,
    on: Option<&CostFilter>,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> Stateful<Div> {
    let on = on == Some(&filter);
    let hover = t.bg_hover;
    button(ElementId::Integer(ix as u64), label)
        .aria_toggled(on.into())
        .items_stretch()
        .flex_col()
        .gap(px(6.))
        .px(px(12.))
        .py(px(8.))
        .when(on, |d| d.bg(t.bg_selected))
        .hover(move |s| s.bg(hover))
        .on_click(cx.listener(move |d, _, _, cx| {
            d.toggle_cost_filter(Some(filter.clone()), cx);
        }))
}

/// A model's or project's row, which narrows the page to it. A model's shows what a reply costs
/// and how its tokens split. `pick` is the filter the row sets, then the one set now.
pub(super) fn pick_row(
    ix: usize,
    name: String,
    tally: &Tally,
    whole: &Tally,
    (filter, on): (CostFilter, Option<&CostFilter>),
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let more = matches!(filter, CostFilter::Model(..))
        .then(|| format!("{} · {}", per_reply(tally), mix(tally)));
    let label = match &more {
        Some(more) => format!("{}. {more}", spoken(&name, tally)),
        None => spoken(&name, tally).to_string(),
    };
    filter_row(ix, label, filter, on, t, cx)
        .child(line(name, tally, t))
        .child(share_bar(share(tally, whole), t.accent, t))
        .children(more.map(|m| detail(m, t)))
        .into_any_element()
}

/// A provider's row: its share of the range in its chart colour, what a reply costs, then its
/// today, this month and all time. It narrows the page to the provider.
pub(super) fn provider_row(
    ix: usize,
    p: &ProviderCost,
    whole: &Tally,
    on: Option<&CostFilter>,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let name = agent_label(p.agent).to_string();
    let periods = format!(
        "Today {} · This month {} · All time {}",
        dollars(p.today.cost()),
        dollars(p.month.cost()),
        dollars(p.all.cost())
    );
    let each = per_reply(&p.in_range);
    let label = format!("{}. {each}. {periods}", spoken(&name, &p.in_range));
    filter_row(ix, label, CostFilter::Provider(p.agent), on, t, cx)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(agent_logo(p.agent, None, 16.))
                .child(line(name, &p.in_range, t).flex_1().min_w_0()),
        )
        .child(share_bar(share(&p.in_range, whole), series(p.agent, t), t))
        .child(detail(each, t))
        .child(detail(periods, t))
        .into_any_element()
}

/// The range's cost split between the agents' own replies and their subagents and background
/// tasks.
pub(super) fn sources(all: &Tally, t: &Theme) -> AnyElement {
    let part = |tokens: u64, cost: f64| Tally {
        input: tokens,
        input_cost: cost,
        replies: all.replies,
        ..Tally::default()
    };
    let main = part(
        all.tokens().saturating_sub(all.subagent_tokens),
        (all.cost() - all.subagent_cost).max(0.),
    );
    let sub = part(all.subagent_tokens, all.subagent_cost);
    list_box(
        vec![
            row(0, "Main agents".into(), &main, all, t),
            row(1, "Subagents and background tasks".into(), &sub, all, t),
        ],
        t,
    )
    .id("costs-sources")
    .role(Role::List)
    .aria_label("Cost by source")
    .into_any_element()
}

/// The four kinds of token, each with its count and cost.
pub(super) fn kinds(all: &Tally, t: &Theme) -> AnyElement {
    let kind = |ix: usize, name: &str, tokens: u64, cost: f64| {
        let tally = Tally {
            input: tokens,
            input_cost: cost,
            replies: all.replies,
            ..Tally::default()
        };
        row(ix, name.to_string(), &tally, all, t)
    };
    list_box(
        vec![
            kind(0, "Input", all.input, all.input_cost),
            kind(1, "Cache reads", all.cache_read, all.cache_read_cost),
            kind(2, "Cache writes", all.cache_write, all.cache_write_cost),
            kind(3, "Output", all.output, all.output_cost),
        ],
        t,
    )
    .id("costs-kinds")
    .role(Role::List)
    .aria_label("Tokens by kind")
    .into_any_element()
}

/// A chat's row, which opens the chat while it is still there.
pub(super) fn chat_row(
    ix: usize,
    chat: &ChatCost,
    whole: &Tally,
    t: &Theme,
    cx: &mut Context<SettingsDialog>,
) -> AnyElement {
    let name = match &chat.project {
        Some(project) => format!("{} · {project}", chat.title),
        None => chat.title.clone(),
    };
    let name = if chat.open {
        name
    } else {
        format!("{name} (deleted)")
    };
    let subagents = (chat.tally.subagent_tokens > 0).then(|| {
        format!(
            "Subagents {} · {} tokens",
            dollars(chat.tally.subagent_cost),
            format_tokens(chat.tally.subagent_tokens)
        )
    });
    let spoken_as = |name: &str| match &subagents {
        Some(s) => SharedString::from(format!("{}. {s}", spoken(name, &chat.tally))),
        None => spoken(name, &chat.tally),
    };
    let body = |d: Stateful<Div>| {
        d.flex()
            .flex_col()
            .gap(px(6.))
            .px(px(12.))
            .py(px(8.))
            .child(line(name.clone(), &chat.tally, t))
            .child(share_bar(share(&chat.tally, whole), t.accent, t))
            .children(subagents.clone().map(|s| detail(s, t)))
    };
    if !chat.open {
        return body(
            div()
                .id(ElementId::Integer(ix as u64))
                .role(Role::ListItem)
                .aria_label(spoken_as(&name)),
        )
        .into_any_element();
    }
    let id = chat.chat_id.clone();
    let hover = t.bg_hover;
    body(button(
        ElementId::Integer(ix as u64),
        spoken_as(&format!("Open {name}")),
    ))
    .items_stretch()
    .hover(move |s| s.bg(hover))
    .on_click(cx.listener(move |_, _, _, cx| cx.emit(SettingsEvent::OpenChat(id.clone()))))
    .into_any_element()
}
