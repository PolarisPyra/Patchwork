//! The values the open vault holds, handed to the log's scrubber so that none of them is
//! written down: not in a log, and not in a reply Patchwork saves.

// Rust guideline compliant 2026-10-09

use serde_json::Value;

use super::crypt::unseal;
use super::state::Vault;
use crate::logging;

/// The fewest characters a key or a token has to be replaced. A shorter one would take
/// ordinary words out of what is written.
const MIN_WHOLE: usize = 8;

/// The fewest characters one value among many has: a header's, a variable's.
const MIN_PART: usize = 16;

/// Reads every value in `vault` again, after it opened or a secret changed. Nothing is logged
/// here: a line logged while the values are swapped would wait on itself.
pub(super) fn refresh(vault: &Vault) {
    let rows: Vec<(String, Vec<u8>, Vec<u8>)> = vault
        .conn
        .prepare("SELECT name, nonce, value FROM secrets")
        .and_then(|mut all| {
            all.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect()
        })
        .unwrap_or_default();
    let mut values = Vec::new();
    for (name, nonce, sealed) in rows {
        let Ok(bytes) = unseal(&vault.cipher, &name, &nonce, &sealed) else {
            continue;
        };
        if let Ok(text) = std::str::from_utf8(&bytes) {
            collect(&name, text, &mut values);
        }
    }
    logging::set_known(values);
}

/// Forgets the values, as the vault is locked.
pub(super) fn clear() {
    logging::set_known(Vec::new());
}

/// Adds what the secret `name` holds to `values`. A key or a token is one value. Headers, an
/// MCP server's values and the terminal variables are several, and most are not secret: only
/// the ones that look like a key are taken.
fn collect(name: &str, text: &str, values: &mut Vec<String>) {
    let many = name.ends_with(":headers") || name.starts_with("mcp/") || name == "terminal:env";
    if !many {
        if text.len() >= MIN_WHOLE {
            values.push(text.to_string());
        }
        return;
    }
    if let Ok(json) = serde_json::from_str::<Value>(text) {
        leaves(&json, values);
    }
}

/// The secret-looking words of every string in `json`. `Bearer <token>` gives its token.
fn leaves(json: &Value, values: &mut Vec<String>) {
    match json {
        Value::String(text) => values.extend(
            text.split_whitespace()
                .filter(|word| looks_secret(word))
                .map(String::from),
        ),
        Value::Array(items) => items.iter().for_each(|item| leaves(item, values)),
        Value::Object(fields) => fields.values().for_each(|field| leaves(field, values)),
        _ => {}
    }
}

/// Whether `word` reads as a key or a token, not as a path, an address or a plain setting.
fn looks_secret(word: &str) -> bool {
    word.len() >= MIN_PART
        && word.chars().any(|c| c.is_ascii_alphabetic())
        && word.chars().any(|c| c.is_ascii_digit())
        && !word.contains("://")
        && !word.starts_with(['/', '~', '.'])
}
