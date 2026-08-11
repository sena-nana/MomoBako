//! 路径、时间和文件名工具。

use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub(crate) fn normalize_path(value: &str) -> String {
    value
        .trim()
        .replace('\\', "/")
        .trim_matches('/')
        .to_string()
}

pub(crate) fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

pub(crate) fn unique_name(
    base: &str,
    id: i64,
    names: &mut std::collections::BTreeSet<String>,
) -> String {
    if names.insert(base.to_string()) {
        return base.to_string();
    }
    let fallback = format!("{base} [{id}]");
    names.insert(fallback.clone());
    fallback
}

pub(crate) fn sanitize_name(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect::<String>()
        .trim()
        .to_string();
    if normalized.is_empty() {
        "Untitled".to_string()
    } else {
        normalized
    }
}

pub(crate) fn millis_to_rfc3339(value: Option<i64>) -> Result<String, String> {
    match value {
        Some(timestamp) if timestamp > 0 => {
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(timestamp) * 1_000_000)
                .map_err(|error| error.to_string())?
                .format(&Rfc3339)
                .map_err(|error| error.to_string())
        }
        _ => now_rfc3339(),
    }
}

pub(crate) fn now_rfc3339() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| error.to_string())
}

pub(crate) fn value_to_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|item| i64::try_from(item).ok()))
        .or_else(|| {
            value
                .as_str()
                .and_then(|item| item.trim().parse::<i64>().ok())
        })
}

pub(crate) fn io_error(error: std::io::Error) -> String {
    error.to_string()
}
pub(crate) fn http_error(error: impl ToString) -> String {
    error.to_string()
}
pub(crate) fn time_error(error: impl ToString) -> String {
    error.to_string()
}
