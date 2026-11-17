//! Keeping secrets and bulk out of the logs: keys and tokens are redacted, and long strings and texts are cut.

// Rust guideline compliant 2026-10-09

use std::fmt;
use std::sync::LazyLock;
use regex::Regex;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

/// Strings in logged JSON longer than this are cut to it.
const MAX_STRING_CHARS: usize = 300;

/// Text that is not JSON is cut to this.
pub(super) const MAX_TEXT_CHARS: usize = 4000;

pub(super) fn scrubbed(mut json: Json) -> Json {
    scrub(&mut json, false);
    json
}

/// Hides string values under secret-looking keys, and every value under `headers`; redacts
/// and cuts the other strings.
pub(super) fn scrub(json: &mut Json, secret: bool) {
    match json {
        Json::String(s) if secret => *s = "[REDACTED]".into(),
        Json::String(s) => *s = cut(&redact(s), MAX_STRING_CHARS),
        Json::Array(items) => items.iter_mut().for_each(|item| scrub(item, secret)),
        Json::Object(fields) => {
            for (key, value) in fields.iter_mut() {
                scrub(value, secret || is_secret(key));
            }
        }
        _ => {}
    }
}

/// `apiKey`, `x-api-key`, `access_token`, `AWS_SECRET_ACCESS_KEY`, `DB_PASSWD`, `GITHUB_PAT`,
/// `headers` and the like: the words the text pattern in [`SECRETS`] looks for, anywhere in the
/// key, and a key that ends in `key` or `pat`.
fn is_secret(key: &str) -> bool {
    let key: String = key.chars().filter(char::is_ascii_alphanumeric).collect();
    let key = key.to_ascii_lowercase();
    SECRET_WORDS.iter().any(|word| key.contains(word))
        || ["key", "pat"].iter().any(|end| key.ends_with(end))
}

/// The words that make a field's value a secret.
const SECRET_WORDS: [&str; 10] = [
    "token",
    "secret",
    "password",
    "passwd",
    "pwd",
    "authorization",
    "cookie",
    "header",
    "credential",
    "privatekey",
];

/// What a secret in free text looks like, and what it is replaced with.
static SECRETS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        // A PEM private key, to its end or the end of the text.
        (
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)",
            "[REDACTED PRIVATE KEY]",
        ),
        // `NAME=value`, `NAME: value` and `"name": "value"` where the name ends in a secret word.
        (
            r#"(?i)\b([A-Z0-9_.-]*(?:api[_-]?key|secret|token|password|passwd|pwd|access[_-]?key|private[_-]?key|credentials?))("?\s*[:=]\s*["']?)[^\s"',;]{4,}"#,
            "${1}${2}[REDACTED]",
        ),
        // `Authorization: Bearer …` and the like.
        (r"(?i)\b(bearer|basic|token)\s+[A-Za-z0-9._~+/=-]{16,}", "${1} [REDACTED]"),
        // `scheme://user:password@host`.
        (r"\b([a-z][a-z0-9+.-]*://)[^\s:/@]+:[^\s@/]+@", "${1}[REDACTED]@"),
        // Keys and tokens with a known shape.
        (
            concat!(
                r"\b(?:sk-ant-[A-Za-z0-9_-]{16,}|sk-(?:proj-)?[A-Za-z0-9_-]{20,}",
                r"|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{20,}",
                r"|xox[abposr]-[A-Za-z0-9-]{10,}|(?:AKIA|ASIA)[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{35}",
                r"|hf_[A-Za-z0-9]{30,}|npm_[A-Za-z0-9]{36}|(?:sk|rk)_live_[0-9A-Za-z]{20,}",
                r"|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})"
            ),
            "[REDACTED]",
        ),
    ]
    .into_iter()
    .filter_map(|(pattern, with)| Regex::new(pattern).ok().map(|re| (re, with)))
    .collect()
});

/// `text` with the keys, tokens and passwords in it replaced.
#[must_use]
pub fn redact(text: &str) -> String {
    SECRETS.iter().fold(text.to_string(), |text, (re, with)| {
        re.replace_all(&text, *with).into_owned()
    })
}

/// The first `max` characters, and how many more there were.
pub(super) fn cut(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((end, _)) => format!("{}…(+{} chars)", &s[..end], s[end..].chars().count()),
        None => s.to_string(),
    }
}

/// JSON that keeps an object's fields in the order they were written.
#[derive(Debug)]
pub enum Json {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Serialize for Json {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Json::Null => serializer.serialize_unit(),
            Json::Bool(b) => serializer.serialize_bool(*b),
            Json::Number(n) => n.serialize(serializer),
            Json::String(s) => serializer.serialize_str(s),
            Json::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Json::Object(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, value) in fields {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("JSON")
    }
    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }
    fn visit_bool<E>(self, v: bool) -> Result<Json, E> {
        Ok(Json::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Json, E> {
        Ok(Json::Number(v.into()))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Json, E> {
        Ok(Json::Number(v.into()))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Json, E> {
        Ok(serde_json::Number::from_f64(v).map_or(Json::Null, Json::Number))
    }
    fn visit_str<E>(self, v: &str) -> Result<Json, E> {
        Ok(Json::String(v.into()))
    }
    fn visit_string<E>(self, v: String) -> Result<Json, E> {
        Ok(Json::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Json::Array(items))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut fields = Vec::new();
        while let Some(field) = map.next_entry()? {
            fields.push(field);
        }
        Ok(Json::Object(fields))
    }
}
