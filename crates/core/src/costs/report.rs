//! Adding the replies up over a range, and the report written out as CSV.

// Rust guideline compliant 2026-10-09

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use chrono::{DateTime, Datelike, Days, Local, Months, NaiveDate};

use crate::Core;
use crate::provider::AgentKind;

use super::prices::reply_tally;
use super::types::{
    Bucket, ChatCost, CostFilter, CostRange, CostReport, ModelCost, ProjectCost, ProviderCost,
    ReplyCost, Tally,
};

/// The chats the report lists.
const TOP_CHATS: usize = 10;

impl Core {
    /// Adds up the tokens and cost of every reply `filter` keeps: the totals, and over `range`
    /// the chart and the breakdowns.
    #[must_use]
    pub fn cost_report(&self, range: CostRange, filter: Option<CostFilter>) -> CostReport {
        let (mut replies, open) = self.counted_replies();
        if let Some(f) = &filter {
            replies.retain(|r| f.keeps(r));
        }
        CostReport {
            filter,
            ..report(&replies, &open, range, Local::now().date_naive())
        }
    }
}

/// The local calendar day of a time in Unix epoch milliseconds.
fn day_of(at: u64) -> NaiveDate {
    DateTime::from_timestamp_millis(i64::try_from(at).unwrap_or(i64::MAX)).map_or_else(
        || Local::now().date_naive(),
        |d| d.with_timezone(&Local).date_naive(),
    )
}

fn month_start(day: NaiveDate) -> NaiveDate {
    day.with_day(1).unwrap_or(day)
}

fn report(
    replies: &[ReplyCost],
    open: &HashSet<String>,
    range: CostRange,
    today: NaiveDate,
) -> CostReport {
    let first = range
        .days()
        .map(|n| today.checked_sub_days(Days::new(n - 1)).unwrap_or(today));
    let month = month_start(today);
    let monthly = range == CostRange::All;
    let mut out = CostReport {
        range,
        monthly,
        ..CostReport::default()
    };
    let mut by_bucket: HashMap<NaiveDate, (Tally, HashMap<AgentKind, Tally>)> = HashMap::new();
    let mut by_model: HashMap<(AgentKind, Option<String>), Tally> = HashMap::new();
    let mut by_project: HashMap<Option<String>, Tally> = HashMap::new();
    let mut by_chat: HashMap<&str, (&ReplyCost, Tally)> = HashMap::new();
    let mut by_provider: HashMap<AgentKind, ProviderCost> = HashMap::new();
    for r in replies {
        let one = reply_tally(&r.usage, r.model.as_deref());
        let day = day_of(r.at);
        let provider = by_provider.entry(r.agent).or_insert_with(|| ProviderCost {
            agent: r.agent,
            today: Tally::default(),
            month: Tally::default(),
            all: Tally::default(),
            in_range: Tally::default(),
        });
        out.all.merge(&one);
        provider.all.merge(&one);
        out.since = Some(out.since.map_or(day, |s| s.min(day)));
        if day == today {
            out.today.merge(&one);
            provider.today.merge(&one);
        }
        if day >= month {
            out.month.merge(&one);
            provider.month.merge(&one);
        }
        if first.is_some_and(|f| day < f) {
            continue;
        }
        out.in_range.merge(&one);
        provider.in_range.merge(&one);
        let bucket = if monthly { month_start(day) } else { day };
        let (whole, parts) = by_bucket.entry(bucket).or_default();
        whole.merge(&one);
        parts.entry(r.agent).or_default().merge(&one);
        by_model
            .entry((r.agent, r.model.clone()))
            .or_default()
            .merge(&one);
        by_project.entry(r.project.clone()).or_default().merge(&one);
        by_chat
            .entry(r.chat_id.as_str())
            .or_insert_with(|| (r, Tally::default()))
            .1
            .merge(&one);
    }
    out.providers = by_provider.into_values().collect();
    out.providers
        .sort_by(|a, b| by_spend(&a.in_range, &b.in_range).then_with(|| by_spend(&a.all, &b.all)));
    let Some(since) = out.since else {
        return out;
    };
    let start = first.map_or(since, |f| f.max(since));
    let days = (today - start).num_days().max(0) as f64 + 1.;
    out.daily_cost = out.in_range.cost() / days;
    out.daily_tokens = (out.in_range.tokens() as f64 / days).round() as u64;
    let providers = &out.providers;
    let bucket = |start: NaiveDate| {
        let Some((tally, parts)) = by_bucket.get(&start) else {
            return Bucket {
                start,
                tally: Tally::default(),
                by_agent: Vec::new(),
            };
        };
        Bucket {
            start,
            tally: *tally,
            by_agent: providers
                .iter()
                .filter_map(|p| parts.get(&p.agent).map(|t| (p.agent, *t)))
                .collect(),
        }
    };
    out.buckets = if monthly {
        std::iter::successors(Some(month_start(since)), |m| {
            m.checked_add_months(Months::new(1)).filter(|n| *n <= month)
        })
        .map(bucket)
        .collect()
    } else {
        first
            .unwrap_or(today)
            .iter_days()
            .take_while(|d| *d <= today)
            .map(bucket)
            .collect()
    };
    out.models = by_model
        .into_iter()
        .map(|((agent, model), tally)| ModelCost {
            agent,
            model,
            tally,
        })
        .collect();
    out.models.sort_by(|a, b| by_spend(&a.tally, &b.tally));
    out.projects = by_project
        .into_iter()
        .map(|(name, tally)| ProjectCost { name, tally })
        .collect();
    out.projects.sort_by(|a, b| by_spend(&a.tally, &b.tally));
    out.chats = by_chat
        .into_values()
        .map(|(r, tally)| ChatCost {
            chat_id: r.chat_id.clone(),
            title: r.title.clone(),
            project: r.project.clone(),
            open: open.contains(&r.chat_id),
            tally,
        })
        .collect();
    out.chats.sort_by(|a, b| by_spend(&a.tally, &b.tally));
    out.chats.truncate(TOP_CHATS);
    out
}

/// Most expensive first, then most tokens.
fn by_spend(a: &Tally, b: &Tally) -> std::cmp::Ordering {
    b.cost()
        .total_cmp(&a.cost())
        .then_with(|| b.tokens().cmp(&a.tokens()))
}

impl CostReport {
    /// The range's days (or months) and its models, as CSV for a spreadsheet.
    #[must_use]
    pub fn csv(&self) -> String {
        let mut out = String::from(
            "Period,Input,Cache reads,Cache writes,Output,Tokens,Cost (USD),Replies\n",
        );
        for b in &self.buckets {
            let period = if self.monthly {
                b.start.format("%Y-%m").to_string()
            } else {
                b.start.to_string()
            };
            push_row(&mut out, &[period], &b.tally);
        }
        out.push_str(
            "\nAgent,Model,Input,Cache reads,Cache writes,Output,Tokens,Cost (USD),Replies\n",
        );
        for m in &self.models {
            let agent = serde_json::to_value(m.agent)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            push_row(
                &mut out,
                &[agent, m.model.clone().unwrap_or_default()],
                &m.tally,
            );
        }
        out
    }
}

/// A CSV row: the leading fields, quoted where they need it, then the tally's figures.
fn push_row(out: &mut String, lead: &[String], t: &Tally) {
    for field in lead {
        if field.contains([',', '"', '\n']) {
            let _ = write!(out, "\"{}\",", field.replace('"', "\"\""));
        } else {
            let _ = write!(out, "{field},");
        }
    }
    let _ = writeln!(
        out,
        "{},{},{},{},{},{:.4},{}",
        t.input,
        t.cache_read,
        t.cache_write,
        t.output,
        t.tokens(),
        t.cost(),
        t.replies
    );
}
