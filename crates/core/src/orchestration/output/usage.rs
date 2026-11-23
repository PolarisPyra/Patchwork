//! Tokens each model request used, summed per message.

// Rust guideline compliant 2026-10-09

use crate::chats::TokenUsage;

/// How the id of the main agent's output that its requests left uncounted starts. It is kept out
/// of the context like a subagent's request, but it is the main agent's.
pub(in crate::orchestration) const TURN_OUTPUT: &str = "turn-output-";

/// A model request the agent made during a turn, and the tokens it used.
pub(in crate::orchestration) struct Request {
    pub(in crate::orchestration) id: String,
    /// The message that was current when the request was reported.
    pub(in crate::orchestration) message_id: String,
    /// False for a subagent's request, which does not count toward the context.
    pub(in crate::orchestration) top_level: bool,
    pub(in crate::orchestration) usage: TokenUsage,
}

/// The tokens a message used, summed over its requests. Its context and speed are those of the
/// last request the main agent made for it.
pub(in crate::orchestration) fn message_usage(
    requests: &[Request],
    message_id: &str,
) -> TokenUsage {
    let mut total = TokenUsage::default();
    for r in requests.iter().filter(|r| r.message_id == message_id) {
        total.input += r.usage.input;
        total.cache_read += r.usage.cache_read;
        total.cache_write += r.usage.cache_write;
        total.output += r.usage.output;
        if let Some(cost) = r.usage.cost_micros {
            total.cost_micros = Some(total.cost_micros.unwrap_or(0) + cost);
        }
        if !r.top_level && !r.id.starts_with(TURN_OUTPUT) {
            let u = &r.usage;
            total.subagent_tokens += u.input + u.cache_read + u.cache_write + u.output;
            if let Some(cost) = u.cost_micros {
                total.subagent_cost_micros = Some(total.subagent_cost_micros.unwrap_or(0) + cost);
            }
        }
        if r.top_level {
            total.context = r.usage.context;
            if r.usage.first_token_ms.is_some() {
                total.first_token_ms = r.usage.first_token_ms;
            }
            if r.usage.output_per_second.is_some() {
                total.output_per_second = r.usage.output_per_second;
            }
        }
    }
    total
}

/// Records the tokens a request used, made while `current` was the message being written, and
/// returns the message they count toward. Claude reports a request once per content block, so a
/// repeat updates it.
pub(in crate::orchestration) fn record_request(
    requests: &mut Vec<Request>,
    request_id: String,
    usage: TokenUsage,
    top_level: bool,
    current: &str,
) -> String {
    if let Some(pos) = requests.iter().position(|r| r.id == request_id) {
        let mut request = requests.remove(pos);
        request.usage = usage;
        let message_id = request.message_id.clone();
        requests.push(request);
        return message_id;
    }
    requests.push(Request {
        id: request_id,
        message_id: current.into(),
        top_level,
        usage,
    });
    current.into()
}
