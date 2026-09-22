//! Column encodings: enums as their serde (snake_case) names, structured values
//! as JSON text, counts as SQLite integers.

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::StoreError;

/// A unit enum as its serde name (e.g. `TaskStatus::Running` → `"running"`).
pub(super) fn name<T: Serialize>(value: &T) -> Result<String, StoreError> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(s) => Ok(s),
        other => Err(StoreError::Corrupt(format!(
            "expected a unit enum, got {other}"
        ))),
    }
}

/// Parses a unit enum from its serde name.
pub(super) fn parse<T: DeserializeOwned>(name: &str) -> Result<T, StoreError> {
    Ok(serde_json::from_value(serde_json::Value::String(
        name.to_string(),
    ))?)
}

pub(super) fn parse_opt<T: DeserializeOwned>(name: Option<&str>) -> Result<Option<T>, StoreError> {
    name.map(parse).transpose()
}

pub(super) fn json<T: Serialize>(value: &T) -> Result<String, StoreError> {
    Ok(serde_json::to_string(value)?)
}

pub(super) fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, StoreError> {
    Ok(serde_json::from_str(text)?)
}

/// A count or index as an SQLite integer.
pub(super) fn int<T: TryInto<i64>>(value: T) -> Result<i64, StoreError> {
    value
        .try_into()
        .map_err(|_| StoreError::Corrupt("integer out of range".into()))
}

/// An SQLite integer back as a count or index.
pub(super) fn uint<T: TryFrom<i64>>(value: i64) -> Result<T, StoreError> {
    T::try_from(value).map_err(|_| StoreError::Corrupt(format!("invalid stored integer {value}")))
}
