//! How the app's types are written to, and read from, SQLite columns.

use serde::Serialize;
use serde::de::DeserializeOwned;

pub(super) type Res<T> = Result<T, String>;

pub(super) fn err(e: impl std::fmt::Display) -> String {
    format!("Database error: {e}")
}

/// Enums are stored as their serde name, e.g. `"claude"`.
pub(super) fn enc<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        other => other.map(|v| v.to_string()).unwrap_or_default(),
    }
}

pub(super) fn dec<T: DeserializeOwned>(s: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub(super) fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "[]".into())
}

pub(super) fn from_json<T: DeserializeOwned + Default>(s: String) -> T {
    serde_json::from_str(&s).unwrap_or_default()
}
