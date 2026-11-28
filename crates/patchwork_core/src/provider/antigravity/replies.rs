//! Answers to Patchwork's own JSON-RPC requests: sessions, settings, and finished prompts.

// Rust guideline compliant 2026-11-23

use serde_json::Value;

use crate::chats::TokenUsage;
use crate::provider::{AgentKind, Parsed, TurnSpec, acp};

use super::errors::{QUOTA_MESSAGE, logged_error};
use super::input::{
    EFFORT_REQUEST, INIT_REQUEST, LOAD_REQUEST, MODE_REQUEST, MODEL_REQUEST, PROMPT_REQUEST,
    SESSION_REQUEST, begin, load_request, session_request,
};
use super::probe::auth_required;

pub(super) fn handle_response(id: &Value, v: &Value, spec: &TurnSpec, out: &mut Vec<Parsed>) {
    let error = v.get("error").filter(|e| !e.is_null());
    let error_msg = error.and_then(|e| e["message"].as_str());
    let error_code = error.and_then(|e| e["code"].as_i64());

    if id == INIT_REQUEST {
        if let Some(msg) = error_msg {
            out.extend([Parsed::Error(msg.into()), Parsed::Finished]);
        } else if spec.agent.is_listed_acp() {
            out.extend(open_session(spec, &v["result"]));
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
                out.extend([Parsed::Error(sign_in_message(spec.agent)), Parsed::Finished]);
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
        // A load may answer without the id, which is the one it was asked to load.
        let session_id = v["result"]["sessionId"]
            .as_str()
            .or(spec
                .session_id
                .filter(|_| loaded && spec.agent.is_listed_acp()))
            .filter(|id| !id.is_empty());
        if let Some(session_id) = session_id {
            out.push(Parsed::SessionId(session_id.into()));
            out.extend(begin(spec, session_id, &v["result"]));
        } else {
            out.extend([
                Parsed::Error(format!(
                    "{} did not return a session id",
                    acp::label(spec.agent)
                )),
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
                sign_in_message(spec.agent)
            } else {
                msg.into()
            };
            out.extend([Parsed::Error(msg), Parsed::Finished]);
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
        if spec.agent == AgentKind::Antigravity
            && matches!(reason, Some("end_turn" | "refusal"))
            && let Some(session_id) = spec.session_id
        {
            out.extend(logged_error(session_id));
        }
        out.extend(stop_reason(spec.agent, reason));
        out.push(Parsed::Finished);
    }
}

/// What a listed agent is sent once it answers `initialize`: `session/load` for the chat's
/// session when it can load one, else a new session. A session it cannot load is started
/// afresh, given the conversation so far.
fn open_session(spec: &TurnSpec, init: &Value) -> Vec<Parsed> {
    let Some(session_id) = spec.session_id else {
        return vec![Parsed::Send(session_request(spec, None).to_string())];
    };
    if init["agentCapabilities"]["loadSession"] == true {
        let load = load_request(spec, session_id).to_string();
        vec![Parsed::Replaying(true), Parsed::Send(load)]
    } else {
        vec![Parsed::SessionLost, Parsed::Finished]
    }
}

/// What to do when the agent says it is not signed in.
fn sign_in_message(agent: AgentKind) -> String {
    match acp::spec(agent) {
        Some(spec) => format!(
            "{} is not signed in. Run `{}` in a terminal, then try again.",
            spec.label, spec.login
        ),
        None => "Antigravity authentication required. Sign in under Settings → Providers.".into(),
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
fn stop_reason(agent: AgentKind, reason: Option<&str>) -> Option<Parsed> {
    let label = acp::label(agent);
    let error = match reason? {
        "refusal" if agent == AgentKind::Antigravity => QUOTA_MESSAGE.into(),
        "refusal" => format!("{label}'s model refused to answer."),
        "max_tokens" => {
            format!("{label}'s reply reached the model's output limit and was cut off.")
        }
        "max_turn_requests" => format!(
            "{label} reached its limit of model requests for one reply. Send a message to continue."
        ),
        _ => return None,
    };
    Some(Parsed::Error(error))
}
