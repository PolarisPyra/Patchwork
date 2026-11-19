//! API rates by model, and pricing a model request or a finished reply with them.

// Rust guideline compliant 2026-10-09

use crate::provider::AgentKind;
use crate::threads::TokenUsage;

use super::types::Tally;

/// US dollars per million tokens of each kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Price {
    input: f64,
    cache_write: f64,
    cache_read: f64,
    output: f64,
}

/// Anthropic's rates, where a 5-minute cache write costs 1.25 times input.
const fn anthropic(input: f64, cache_read: f64, output: f64) -> Price {
    Price {
        input,
        cache_write: input * 1.25,
        cache_read,
        output,
    }
}

/// `OpenAI`'s rates, where writing the cache costs what input does.
const fn openai(input: f64, cache_read: f64, output: f64) -> Price {
    Price {
        input,
        cache_write: input,
        cache_read,
        output,
    }
}

/// First-party API rates as of 2026-11, by a part of the model id. More specific parts come
/// first, so `opus-5-5` matches before `opus-5`. A model left out, such as GPT-6.1 and 6.2 whose
/// published rates disagree, counts its tokens but adds no cost.
const PRICES: &[(&str, Price)] = &[
    ("fable-5-2", anthropic(8., 0.20, 40.)),
    ("mythos-5-2", anthropic(8., 0.20, 40.)),
    ("fable-5-1", anthropic(10., 0.25, 50.)),
    ("mythos-5-1", anthropic(10., 0.25, 50.)),
    ("fable-5", anthropic(10., 1., 50.)),
    ("mythos-5", anthropic(10., 1., 50.)),
    ("opus-5-5", anthropic(4., 0.20, 20.)),
    ("opus-5", anthropic(5., 0.50, 25.)),
    ("opus-4-8", anthropic(5., 0.50, 25.)),
    ("opus-4-7", anthropic(5., 0.50, 25.)),
    ("opus-4-6", anthropic(5., 0.50, 25.)),
    ("sonnet-5-5", anthropic(2., 0.10, 10.)),
    ("sonnet-5", anthropic(2., 0.20, 10.)),
    ("sonnet-4-6", anthropic(3., 0.30, 15.)),
    ("haiku-5-5", anthropic(0.10, 0.01, 0.50)),
    ("haiku-4-5", anthropic(1., 0.10, 5.)),
    ("gpt-6-astra", openai(10., 1., 50.)),
    ("gpt-6-sol", openai(2., 0.20, 10.)),
    ("gpt-6-luna", openai(0.10, 0.01, 0.50)),
];

/// The aliases an agent accepts for the newest model of a line.
const ALIASES: &[(&str, &str)] = &[
    ("fable", "fable-5-2"),
    ("opus", "opus-5-5"),
    ("sonnet", "sonnet-5-5"),
    ("haiku", "haiku-5-5"),
];

/// The price of `model`, read from its id: `claude-opus-5-5[1m]`, `anthropic/claude-opus-5.5`
/// and `opus` all name Opus 5.5.
pub(super) fn price_of(model: &str) -> Option<Price> {
    let id = model
        .split('[')
        .next()
        .unwrap_or(model)
        .trim()
        .to_ascii_lowercase()
        .replace('.', "-");
    let id = ALIASES
        .iter()
        .find(|(alias, _)| id == *alias)
        .map_or(id.as_str(), |(_, full)| full);
    PRICES
        .iter()
        .find(|(part, _)| contains_part(id, part))
        .map(|(_, p)| *p)
}

/// Whether `part` is in `id` and not followed by more of a version, so `opus-5` is not found in
/// `opus-5-5`. A date suffix such as `-20260101` is not part of the version.
fn contains_part(id: &str, part: &str) -> bool {
    let digits = |s: &str| s.chars().take_while(char::is_ascii_digit).count();
    id.match_indices(part).any(|(at, _)| {
        let rest = &id[at + part.len()..];
        digits(rest) == 0
            && !rest
                .strip_prefix('-')
                .is_some_and(|r| (1..=2).contains(&digits(r)))
    })
}

fn per_million(tokens: u64, dollars: f64) -> f64 {
    tokens as f64 * dollars / 1_000_000.
}

/// A cost an agent reports in US dollars, as millionths of a dollar; None when it reports none.
pub(crate) fn reported(cost: &serde_json::Value) -> Option<u64> {
    cost.as_f64()
        .filter(|c| c.is_finite() && *c > 0.)
        .map(|c| (c * 1_000_000.).round() as u64)
}

/// The model a turn runs on: the one it names, else the agent's own configured default.
pub(crate) fn turn_model(agent: AgentKind, model: Option<&str>) -> Option<String> {
    model.map(String::from).or_else(|| match agent {
        AgentKind::Claude => crate::provider::claude::configured_model(),
        AgentKind::Codex => crate::provider::codex::configured_model(),
        _ => None,
    })
}

/// What a request's tokens cost on `model` at API rates, in millionths of a US dollar.
pub(crate) fn priced(model: Option<&str>, usage: &TokenUsage) -> Option<u64> {
    let p = price_of(model?)?;
    let dollars = per_million(usage.input, p.input)
        + per_million(usage.cache_read, p.cache_read)
        + per_million(usage.cache_write, p.cache_write)
        + per_million(usage.output, p.output);
    Some((dollars * 1_000_000.).round() as u64)
}

/// One reply's tally. Its saved cost is split over the kinds as `model`'s rates split them, or
/// by tokens when the model has no known rate; without a saved cost, `model`'s rates price it.
pub(super) fn reply_tally(usage: &TokenUsage, model: Option<&str>) -> Tally {
    let mut t = Tally {
        input: usage.input,
        cache_read: usage.cache_read,
        cache_write: usage.cache_write,
        output: usage.output,
        replies: 1,
        ..Tally::default()
    };
    let reported = usage.cost_micros.map(|c| c as f64 / 1_000_000.);
    if let Some(p) = model.and_then(price_of) {
        t.input_cost = per_million(t.input, p.input);
        t.cache_read_cost = per_million(t.cache_read, p.cache_read);
        t.cache_write_cost = per_million(t.cache_write, p.cache_write);
        t.output_cost = per_million(t.output, p.output);
        t.cache_saved = per_million(t.cache_read, p.input - p.cache_read);
        let rated = t.cost();
        if let Some(want) = reported
            && rated > 0.
        {
            let scale = want / rated;
            t.input_cost *= scale;
            t.cache_read_cost *= scale;
            t.cache_write_cost *= scale;
            t.output_cost *= scale;
            t.cache_saved *= scale;
        }
    } else if let Some(want) = reported {
        let tokens = t.tokens().max(1) as f64;
        t.input_cost = want * t.input as f64 / tokens;
        t.cache_read_cost = want * t.cache_read as f64 / tokens;
        t.cache_write_cost = want * t.cache_write as f64 / tokens;
        t.output_cost = want * t.output as f64 / tokens;
    } else {
        t.unpriced = t.tokens();
    }
    t.subagent_tokens = usage.subagent_tokens.min(t.tokens());
    // The subagents' share of the cost: as reported, else by their share of the tokens.
    let share = match (usage.subagent_cost_micros, usage.cost_micros) {
        (Some(part), Some(whole)) if whole > 0 => part as f64 / whole as f64,
        _ => t.subagent_tokens as f64 / t.tokens().max(1) as f64,
    };
    t.subagent_cost = t.cost() * share.clamp(0., 1.);
    t
}
