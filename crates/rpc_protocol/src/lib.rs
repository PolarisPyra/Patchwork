//! The messages Patchwork's server and its phone and web clients exchange.
//!
//! Every request, answer and push that crosses the wire has a type here, in the shapes the web
//! client's `packages/contracts` has. The server decodes a request's params into the method's
//! [`Method::Params`], calls one operation, and encodes its [`Method::Output`]. Field order is
//! the order on the wire.

// Rust guideline compliant 2026-10-09

pub mod app;
pub mod chats;
pub mod devices;
pub mod frames;
pub mod messages;
pub mod methods;
pub mod pushes;
pub mod turns;

use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;

/// One method a client calls: its name, what it is sent and what it answers.
pub trait Method {
    /// The name a request gives, such as `chats.list`.
    const NAME: &'static str;
    /// The request's params.
    type Params: DeserializeOwned;
    /// The answer's `result`.
    type Output: Serialize;
}

/// Reads a request's params for `M`.
///
/// Params that are not an object, such as null when a request has none, read as an empty one.
///
/// # Errors
///
/// Returns the message a client shows, naming a missing param as `Bad params for {method}:
/// {name} is required`.
pub fn decode<M: Method>(params: &serde_json::Value) -> Result<M::Params, String> {
    let empty = serde_json::Value::Object(serde_json::Map::new());
    let params = if params.is_object() { params } else { &empty };
    M::Params::deserialize(params).map_err(|e| bad_params(M::NAME, &e))
}

/// Writes an answer for `M` as JSON.
///
/// # Errors
///
/// Returns the serializer's message; the types here only fail on a path that is not UTF-8.
pub fn encode<M: Method>(output: &M::Output) -> Result<serde_json::Value, String> {
    serde_json::to_value(output).map_err(|e| e.to_string())
}

/// The message for params of `method` that do not decode.
fn bad_params(method: &str, error: &serde_json::Error) -> String {
    let error = error.to_string();
    let missing = error
        .strip_prefix("missing field `")
        .and_then(|rest| rest.split_once('`'))
        .map(|(name, _)| name);
    match missing {
        Some(name) => format!("Bad params for {method}: {name} is required"),
        None => format!("Bad params for {method}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{decode, methods};

    #[test]
    fn a_missing_param_is_named() {
        assert_eq!(
            decode::<methods::SendTurn>(&json!({ "chatId": "c-1" })).map(|_| ()),
            Err("Bad params for turn.send: text is required".to_string())
        );
        assert_eq!(
            decode::<methods::ListMessages>(&json!({})).map(|_| ()),
            Err("Bad params for messages.list: chatId is required".to_string())
        );
    }

    #[test]
    fn a_param_of_the_wrong_type_is_described() {
        let error = decode::<methods::StopTurn>(&json!({ "chatId": 3 })).map(|_| ());
        assert_eq!(
            error,
            Err("Bad params for turn.stop: invalid type: integer `3`, expected a string".into())
        );
    }

    #[test]
    fn methods_without_params_take_anything() {
        decode::<methods::ListChats>(&json!({ "extra": 1 })).expect("extra params are ignored");
        decode::<methods::ListAgents>(&json!(null)).expect("no params are taken");
    }
}
