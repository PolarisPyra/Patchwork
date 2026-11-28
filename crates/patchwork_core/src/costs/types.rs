//! The cost report and its parts.

// Rust guideline compliant 2026-10-09

use chrono::NaiveDate;

use crate::provider::AgentKind;
use crate::chats::TokenUsage;

/// Tokens of each kind and what they cost, in US dollars.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Tally {
    /// Input that was neither read from nor written to the prompt cache.
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
    pub input_cost: f64,
    pub cache_read_cost: f64,
    pub cache_write_cost: f64,
    pub output_cost: f64,
    /// What the cache reads would have cost as plain input, less what they cost.
    pub cache_saved: f64,
    /// Tokens of replies without a known cost, which add none.
    pub unpriced: u64,
    /// Replies counted.
    pub replies: u64,
    /// The part of the tokens that subagents and background tasks used. Replies saved before
    /// it was recorded count none.
    pub subagent_tokens: u64,
    /// The part of the cost those tokens make up.
    pub subagent_cost: f64,
}

impl Tally {
    /// Every token, of every kind.
    #[must_use]
    pub fn tokens(&self) -> u64 {
        self.input + self.cache_read + self.cache_write + self.output
    }

    /// The cost of every kind, in US dollars.
    #[must_use]
    pub fn cost(&self) -> f64 {
        self.input_cost + self.cache_read_cost + self.cache_write_cost + self.output_cost
    }

    /// The share of the prompt read from the cache, 0 to 1.
    #[must_use]
    pub fn cache_share(&self) -> f64 {
        let prompt = self.input + self.cache_read + self.cache_write;
        if prompt == 0 {
            0.
        } else {
            self.cache_read as f64 / prompt as f64
        }
    }

    pub(super) fn merge(&mut self, other: &Self) {
        self.input += other.input;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.output += other.output;
        self.input_cost += other.input_cost;
        self.cache_read_cost += other.cache_read_cost;
        self.cache_write_cost += other.cache_write_cost;
        self.output_cost += other.output_cost;
        self.cache_saved += other.cache_saved;
        self.unpriced += other.unpriced;
        self.replies += other.replies;
        self.subagent_tokens += other.subagent_tokens;
        self.subagent_cost += other.subagent_cost;
    }
}

/// One model's share.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCost {
    pub agent: AgentKind,
    /// None when neither the chat nor the agent's settings name one.
    pub model: Option<String>,
    pub tally: Tally,
}

/// One provider's spend over the same periods as the report's totals.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderCost {
    pub agent: AgentKind,
    pub today: Tally,
    pub month: Tally,
    pub all: Tally,
    pub in_range: Tally,
}

/// One project's share; `None` is the chats without a project.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectCost {
    pub name: Option<String>,
    pub tally: Tally,
}

/// One chat's share.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatCost {
    pub chat_id: String,
    pub title: String,
    pub project: Option<String>,
    /// False once the chat is deleted, so there is nothing to open.
    pub open: bool,
    pub tally: Tally,
}

/// A bar of the chart: a local calendar day, or a month in the all-time chart.
#[derive(Debug, Clone, PartialEq)]
pub struct Bucket {
    /// The day, or the month's first day.
    pub start: NaiveDate,
    pub tally: Tally,
    /// Each provider's part, in the order of the report's `providers`, without those that
    /// had no reply in it.
    pub by_agent: Vec<(AgentKind, Tally)>,
}

/// The replies a report is narrowed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostFilter {
    Provider(AgentKind),
    /// A provider's model; None is the one its settings pick.
    Model(AgentKind, Option<String>),
    /// A project, by name; None is the chats without one.
    Project(Option<String>),
}

impl CostFilter {
    pub(super) fn keeps(&self, r: &ReplyCost) -> bool {
        match self {
            Self::Provider(agent) => r.agent == *agent,
            Self::Model(agent, model) => r.agent == *agent && r.model == *model,
            Self::Project(name) => r.project == *name,
        }
    }
}

/// The span the chart and the breakdowns cover, ending today.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CostRange {
    Week,
    #[default]
    Month,
    Quarter,
    All,
}

impl CostRange {
    /// Every range, shortest first.
    pub const ALL: [Self; 4] = [Self::Week, Self::Month, Self::Quarter, Self::All];

    /// Its name, as the picker shows it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Week => "7 days",
            Self::Month => "30 days",
            Self::Quarter => "90 days",
            Self::All => "All time",
        }
    }

    /// Days it covers, today included; None for all time.
    #[must_use]
    pub fn days(self) -> Option<u64> {
        match self {
            Self::Week => Some(7),
            Self::Month => Some(30),
            Self::Quarter => Some(90),
            Self::All => None,
        }
    }
}

/// Everything Settings → Costs shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostReport {
    pub range: CostRange,
    /// The replies counted; None counts every one.
    pub filter: Option<CostFilter>,
    pub today: Tally,
    /// This calendar month so far.
    pub month: Tally,
    pub all: Tally,
    /// The replies in `range`.
    pub in_range: Tally,
    /// `in_range`'s cost spread over its days; all time spreads from the first reply.
    pub daily_cost: f64,
    /// `in_range`'s tokens spread the same way.
    pub daily_tokens: u64,
    /// The first day with a counted reply.
    pub since: Option<NaiveDate>,
    /// True when each bar is a month, as in the all-time chart.
    pub monthly: bool,
    /// Every day (or month) of the range, oldest first, each present even without replies.
    pub buckets: Vec<Bucket>,
    /// Every provider with a counted reply, most expensive in `range` first, then all time.
    pub providers: Vec<ProviderCost>,
    /// In `range`, most expensive first, then most tokens.
    pub models: Vec<ModelCost>,
    /// In `range`, most expensive first, then most tokens.
    pub projects: Vec<ProjectCost>,
    /// The ten chats that cost most in `range`.
    pub chats: Vec<ChatCost>,
}

impl CostReport {
    /// The days (or months) of the range with at least one counted reply.
    #[must_use]
    pub fn active_periods(&self) -> usize {
        self.buckets.iter().filter(|b| b.tally.replies > 0).count()
    }

    /// The day (or month) that cost most, or used the most tokens when `by_cost` is false; None
    /// when no period has any.
    #[must_use]
    pub fn peak(&self, by_cost: bool) -> Option<&Bucket> {
        let value = |b: &Bucket| {
            if by_cost {
                b.tally.cost()
            } else {
                b.tally.tokens() as f64
            }
        };
        self.buckets
            .iter()
            .filter(|b| value(b) > 0.)
            .max_by(|a, b| value(a).total_cmp(&value(b)))
    }
}

/// A finished reply's tokens, with what it is counted under. The ledger keeps one for each reply
/// that is deleted.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReplyCost {
    pub(crate) message_id: String,
    pub(crate) chat_id: String,
    pub(crate) title: String,
    /// The project's name.
    pub(crate) project: Option<String>,
    pub(crate) agent: AgentKind,
    pub(crate) model: Option<String>,
    /// Unix epoch milliseconds.
    pub(crate) at: u64,
    pub(crate) usage: TokenUsage,
}
