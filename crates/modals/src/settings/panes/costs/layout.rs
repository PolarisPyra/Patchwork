//! The Costs pane drawn.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Role, SharedString, div, prelude::*, px};
use patchwork_core::costs::{CostFilter, CostRange, Tally};

use super::chart::chart;
use super::hero::hero;
use super::lines::lines;
use super::rows::{Cell, chat_row, dollars, grouped, kinds, pick_row, provider_row, sources, strip};
use super::report::CostMetric;
use crate::settings::SettingsDialog;
use crate::settings::controls::{empty, list_box, section_heading, segmented};
use patchwork_ui::theme::{fs_sm, radius_lg, radius_sm, theme};
use patchwork_ui::{agent_label, button, format_tokens, icon, spinner};

pub fn render(d: &mut SettingsDialog, cx: &mut Context<SettingsDialog>) -> AnyElement {
    d.load_costs(cx);
    let t = *theme(cx);
    let Some(report) = d.costs.report.clone() else {
        return div()
            .id("costs-loading")
            .role(Role::Status)
            .aria_label("Adding up costs")
            .flex()
            .items_center()
            .gap(px(8.))
            .py(px(10.))
            .text_color(t.text_muted)
            .child(spinner(14., t.text_muted))
            .child("Adding up costs")
            .into_any_element();
    };
    let ranges: Vec<(CostRange, &str)> = CostRange::ALL.iter().map(|r| (*r, r.label())).collect();
    let picker = segmented(
        "costs-range",
        "Range",
        d.costs.range,
        &ranges,
        &t,
        SettingsDialog::set_cost_range,
        cx,
    );
    let metric = segmented(
        "costs-metric",
        "Measure",
        d.costs.metric,
        &[(CostMetric::Cost, "Cost"), (CostMetric::Tokens, "Tokens")],
        &t,
        SettingsDialog::set_cost_metric,
        cx,
    );
    let hover = t.bg_hover;
    let fg = t.text;
    let export = button("costs-export", "Export CSV")
        .flex_none()
        .h(px(26.))
        .gap(px(6.))
        .px(px(10.))
        .rounded(px(radius_sm()))
        .border_1()
        .border_color(t.border_strong)
        .text_size(px(fs_sm()))
        .text_color(t.text_muted)
        .hover(move |s| s.bg(hover).text_color(fg))
        .on_click(cx.listener(|d, _, _, cx| d.export_costs(cx)))
        .child(icon("export", 14.))
        .child("Export CSV");
    let clear = d.costs.filter.as_ref().map(|f| {
        let name = match f {
            CostFilter::Provider(agent) => agent_label(*agent).to_string(),
            CostFilter::Model(agent, model) => format!(
                "{} · {}",
                model.as_deref().unwrap_or("Default model"),
                agent_label(*agent)
            ),
            CostFilter::Project(name) => name.clone().unwrap_or_else(|| "No project".into()),
        };
        button("costs-filter", format!("Clear filter: {name}"))
            .flex_none()
            .min_w_0()
            .h(px(26.))
            .gap(px(6.))
            .px(px(10.))
            .rounded(px(radius_sm()))
            .bg(t.bg_selected)
            .text_size(px(fs_sm()))
            .hover(move |s| s.bg(hover))
            .on_click(cx.listener(|d, _, _, cx| d.toggle_cost_filter(None, cx)))
            .child(div().min_w_0().truncate().child(name))
            .child(icon("x", 11.))
    });
    let header = div()
        .flex()
        .items_center()
        .gap(px(8.))
        // The pane starts here, without a row's own space above it.
        .pt(px(12.))
        .pb(px(4.))
        .child(metric)
        .child(picker)
        .children(clear)
        .child(div().flex_1())
        .child(export);
    if report.all.replies == 0 {
        return empty("No replies with token counts yet", &t).into_any_element();
    }
    let range = &report.in_range;
    let by_cost = d.costs.metric == CostMetric::Cost;
    let overview = div()
        .id("costs-overview")
        .role(Role::Group)
        .aria_label("Overview")
        .flex()
        .gap(px(24.))
        .p(px(16.))
        .rounded(px(radius_lg()))
        .border_1()
        .border_color(t.border)
        .child(hero(&report, by_cost, &t))
        .child(chart(&report, d.costs.bar, by_cost, &t, cx));
    // A period's figure in the metric picked, with the other one under it.
    let cell = |label: &'static str, tally: &Tally| {
        let tokens = format!("{} tokens", format_tokens(tally.tokens()));
        let (value, detail) = if by_cost {
            (dollars(tally.cost()), tokens)
        } else {
            (tokens, dollars(tally.cost()))
        };
        Cell {
            label,
            value,
            detail,
        }
    };
    let per_reply = if by_cost {
        format!(
            "{} per reply",
            dollars(range.cost() / range.replies.max(1) as f64)
        )
    } else {
        format!(
            "{} tokens per reply",
            format_tokens(range.tokens() / range.replies.max(1))
        )
    };
    let summary = strip(
        "costs-summary",
        "Summary",
        vec![
            cell("Today", &report.today),
            cell("This month", &report.month),
            cell("All time", &report.all),
            Cell {
                label: "Saved by cache",
                value: dollars(range.cache_saved),
                detail: format!("{:.0}% of input cached", range.cache_share() * 100.),
            },
            Cell {
                label: "Replies",
                value: grouped(range.replies),
                detail: per_reply,
            },
        ],
        &t,
    );
    let chats: Vec<AnyElement> = report
        .chats
        .iter()
        .enumerate()
        .map(|(ix, c)| chat_row(ix, c, range, &t, cx))
        .collect();
    let on = d.costs.filter.as_ref();
    let providers: Vec<AnyElement> = report
        .providers
        .iter()
        .enumerate()
        .map(|(ix, p)| provider_row(ix, p, range, on, &t, cx))
        .collect();
    let models: Vec<AnyElement> = report
        .models
        .iter()
        .enumerate()
        .map(|(ix, m)| {
            let model = m.model.as_deref().unwrap_or("Default model");
            pick_row(
                ix,
                format!("{model} · {}", agent_label(m.agent)),
                &m.tally,
                range,
                (CostFilter::Model(m.agent, m.model.clone()), on),
                &t,
                cx,
            )
        })
        .collect();
    let projects: Vec<AnyElement> = report
        .projects
        .iter()
        .enumerate()
        .map(|(ix, p)| {
            let name = p.name.clone().unwrap_or_else(|| "No project".into());
            pick_row(
                ix,
                name,
                &p.tally,
                range,
                (CostFilter::Project(p.name.clone()), on),
                &t,
                cx,
            )
        })
        .collect();
    let note = if range.unpriced > 0 {
        format!(
            "Estimated at API rates. {} tokens from models without a known price add no cost.",
            format_tokens(range.unpriced)
        )
    } else {
        "Estimated at API rates.".into()
    };
    let list = |rows: Vec<AnyElement>, id: &'static str, label: &'static str| {
        list_box(rows, &t)
            .id(id)
            .role(Role::List)
            .aria_label(SharedString::from(label))
    };
    // 16 above a heading and 8 below it, as in the other panes, with the column's 8 gap.
    let heading = |text: &'static str| section_heading(text, false, &t).mt(px(8.)).mb(px(0.));
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(header)
        .child(overview)
        .child(summary)
        .children(lines(&report, by_cost, &t))
        .child(heading("Tokens"))
        .child(kinds(range, &t))
        .when(range.subagent_tokens > 0, |d| {
            d.child(heading("By source")).child(sources(range, &t))
        })
        .when(!chats.is_empty(), |d| {
            d.child(heading("Most expensive chats")).child(list(
                chats,
                "costs-chats",
                "Most expensive chats",
            ))
        })
        .child(heading("By provider"))
        .child(list(providers, "costs-providers", "Cost by provider"))
        .child(heading("By model"))
        .child(list(models, "costs-models", "Cost by model"))
        .child(heading("By project"))
        .child(list(projects, "costs-projects", "Cost by project"))
        .child(
            div()
                .text_size(px(fs_sm()))
                .text_color(t.text_subtle)
                .child(note),
        )
        .into_any_element()
}
