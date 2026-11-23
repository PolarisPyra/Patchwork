//! Answers to Patchwork's own JSON-RPC requests: sessions, settings, and finished prompts.

// Rust guideline compliant 2026-11-23

use serde_json::Value;

use crate::chats::TokenUsage;
use crate::provider::{Parsed, TurnSpec};

use super::errors::{QUOTA_MESSAGE, logged_error};
use super::input::{
    EFFORT_REQUEST, INIT_REQUEST, LOAD_REQUEST, MODE_REQUEST, MODEL_REQUEST, PROMPT_REQUEST,
    SESSION_REQUEST, begin, load_request,
};
use super::probe::auth_required;

pub(super) fn handle_response(id: &Value, v: &Value, spec: &TurnSpec, out: &mut Vec<Parsed>) {
    let error = v.get("error").filter(|e| !e.is_null());
    let error_msg = error.and_then(|e| e["message"].as_str());
    let error_code = error.and_then(|e| e["code"].as_i64());

    if id == INIT_REQUEST {
        if let Some(msg) = error_msg {
            out.extend([Parsed::Error(msg.into()), Parsed::Finished]);
        }
        return;
    }

    let loaded = id.as_str() == Some(LOAD_REQUEST);
    if id == SESSION_REQUEST || loaded {
        if loaded {
            out.push(Parsed::Replaying(false));
        }
        if let Some(msg) = error_msg {
            if auth_required(error_code, msg) {
                out.extend([
                    Parsed::Error(
                        "Antigravity authentication required. Sign in under Settings → Providers."
                            .into(),
                    ),
                    Parsed::Finished,
                ]);
            } else if let Some(session_id) = spec.session_id.filter(|_| session_missing(msg)) {
                if loaded {
                    // A session agy can neither resume nor load is started afresh, given the
                    // conversation so far.
                    out.extend([Parsed::SessionLost, Parsed::Finished]);
                } else {
                    // agy loads a session it cannot resume, replaying its conversation first.
                    let load = load_request(spec, session_id).to_string();
                    out.extend([Parsed::Replaying(true), Parsed::Send(load)]);
                }
            } else {
                // Other failures are shown, so a passing one does not replace the session.
                out.extend([Parsed::Error(msg.into()), Parsed::Finished]);
            }
            return;
        }
        if let Some(session_id) = v["result"]["sessionId"]
            .as_str()
            .filter(|id| !id.is_empty())
        {
            out.push(Parsed::SessionId(session_id.into()));
            out.extend(begin(spec, session_id, &v["result"]));
        } else {
            out.extend([
                Parsed::Error("Antigravity ACP did not return a session id".into()),
                Parsed::Finished,
            ]);
        }
        return;
    }

    if id == MODE_REQUEST || id == MODEL_REQUEST || id == EFFORT_REQUEST {
        // A model agy refuses ends the turn rather than reply on another; a mode or effort it
        // refuses leaves its own.
        if id == MODEL_REQUEST
            && let Some(msg) = error_msg
        {
            out.extend([Parsed::Error(msg.into()), Parsed::Finished]);
            return;
        }
        let config = &v["result"]["configOptions"];
        if config.is_array() {
            out.push(Parsed::AgentConfig(config.clone()));
        }
        out.push(Parsed::SendNext);
        return;
    }

    if let Some(prompt_id) = id.as_u64().filter(|&n| n >= PROMPT_REQUEST) {
        if let Some(msg) = error_msg {
            let msg = if auth_required(error_code, msg) {
                "Antigravity authentication required. Sign in under Settings → Providers."
            } else {
                msg
            };
            out.extend([Parsed::Error(msg.into()), Parsed::Finished]);
            return;
        }
        let result = &v["result"];
        let usage = &result["usage"];
        if usage.is_object() {
            let count = |key: &str| usage[key].as_u64().unwrap_or(0);
            let input_tokens = count("inputTokens");
            let cached_read = count("cachedReadTokens");
            let cached_write = count("cachedWriteTokens");
            let output_tokens = count("outputTokens") + count("thoughtTokens");
            let total_tokens = count("totalTokens");
            if input_tokens > 0 || output_tokens > 0 || total_tokens > 0 {
                out.push(Parsed::RequestUsage {
                    request_id: format!("acp-prompt-{prompt_id}"),
                    usage: TokenUsage {
                        input: input_tokens,
                        cache_read: cached_read,
                        cache_write: cached_write,
                        output: output_tokens,
                        // These counts are summed over the turn's model calls, so they say
                        // nothing of the context; `usage_update` reports that.
                        ..Default::default()
                    },
                    top_level: true,
                    model: None,
                });
            }
        }
        let reason = result["stopReason"].as_str();
        // A failed model call ends the reply as `end_turn`, and a used-up quota as `refusal`;
        // only agy's log says what failed and when a limit resets, so it goes first.
        if matches!(reason, Some("end_turn" | "refusal"))
            && let Some(session_id) = spec.session_id
        {
            out.extend(logged_error(session_id));
        }
        out.extend(stop_reason(reason));
        out.push(Parsed::Finished);
    }
}

/// Whether a `session/resume` error says agy has no such session. agy 1.5 answers an unknown
/// id with `Failed to read session <id>; retry, or load it with session/load`.
fn session_missing(message: &str) -> bool {
    let lower = message.to_lowercase();
    [
        "failed to read session",
        "not found",
        "no such session",
        "does not exist",
    ]
    .iter()
    .any(|term| lower.contains(term))
}

/// What a reply's ACP `stopReason` adds before it ends: nothing for a normal end or a stop.
///
/// agy ends a reply with `refusal` only when the model's quota ran out; the quota notice it
/// streamed before, when any, is kept over this one.
fn stop_reason(reason: Option<&str>) -> Option<Parsed> {
    let error = match reason? {
        "refusal" => QUOTA_MESSAGE,
        "max_tokens" => "Antigravity's reply reached the model's output limit and was cut off.",
        "max_turn_requests" => {
            "Antigravity reached its limit of model requests for one reply. Send a message to \
             continue."
        }
        _ => return None,
    };
    Some(Parsed::Error(error.into()))
}
