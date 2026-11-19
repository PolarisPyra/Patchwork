//! The Costs pane drawn.

// Rust guideline compliant 2026-10-09

use gpui::{AnyElement, Role, SharedString, div, prelude::*, px};
use patchwork_core::costs::{CostFilter, CostRange};

use super::chart::chart;
use super::rows::{chat_row, dollars, kinds, pick_row, provider_row, sources, stat, tally_stat};
use crate::settings::SettingsDialog;
use crate::settings::controls::{list_box, section_heading, segmented};
use crate::ui::theme::{fs_sm, radius_sm, theme};
use crate::ui::{agent_label, button, format_tokens, icon, spinner};

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
        .pb(px(8.))
        .child(picker)
        .children(clear)
        .child(div().flex_1())
        .child(export);
    if report.all.replies == 0 {
        return div()
            .py(px(10.))
            .text_color(t.text_muted)
            .child("No replies with token counts yet")
            .into_any_element();
    }
    let range = &report.in_range;
    let span = match report.range {
        CostRange::All => stat(
            "Replies",
            range.replies.to_string(),
            report
                .since
                .map(|s| format!("Since {}", s.format("%b %-d, %Y")))
                .unwrap_or_default(),
            &t,
        ),
        r => tally_stat(&format!("Last {}", r.label()), range, &t),
    };
    let month = tally_stat("This month", &report.month, &t);
    let tiles = div()
        .id("costs-summary")
        .role(Role::Group)
        .aria_label("Summary")
        .grid()
        .grid_cols(3)
        .gap(px(8.))
        .child(tally_stat("Today", &report.today, &t))
        .child(stat(
            "Daily average",
            format!("{} tokens", format_tokens(report.daily_tokens)),
            dollars(report.daily_cost),
            &t,
        ))
        .child(span)
        .child(month)
        .child(tally_stat("All time", &report.all, &t))
        .child(stat(
            "Saved by cache",
            dollars(range.cache_saved),
            format!("{:.0}% of input from cache", range.cache_share() * 100.),
            &t,
        ));
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
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(header)
        .child(tiles)
        .child(section_heading(
            if report.monthly { "By month" } else { "By day" },
            false,
            &t,
        ))
        .child(chart(&report, d.costs.bar, &t, cx))
        .child(section_heading("Tokens", false, &t))
        .child(kinds(range, &t))
        .when(range.subagent_tokens > 0, |d| {
            d.child(section_heading("By source", false, &t))
                .child(sources(range, &t))
        })
        .when(!chats.is_empty(), |d| {
            d.child(section_heading("Most expensive chats", false, &t))
                .child(list(chats, "costs-chats", "Most expensive chats"))
        })
        .child(section_heading("By provider", false, &t))
        .child(list(providers, "costs-providers", "Cost by provider"))
        .child(section_heading("By model", false, &t))
        .child(list(models, "costs-models", "Cost by model"))
        .child(section_heading("By project", false, &t))
        .child(list(projects, "costs-projects", "Cost by project"))
        .child(
            div()
                .text_size(px(fs_sm()))
                .text_color(t.text_subtle)
                .child(note),
        )
        .into_any_element()
}
