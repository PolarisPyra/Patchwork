//! Tokens each model request used, summed per message.
use crate::threads::TokenUsage;

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
        if r.top_level {
            total.context = r.usage.context;
            total.first_token_ms = r.usage.first_token_ms;
            total.output_per_second = r.usage.output_per_second;
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
    if let Some(request) = requests.iter_mut().find(|r| r.id == request_id) {
        request.usage = usage;
        return request.message_id.clone();
    }
    requests.push(Request {
        id: request_id,
        message_id: current.into(),
        top_level,
        usage,
    });
    current.into()
}
