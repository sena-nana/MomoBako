//! Shared formatting, time, and error helpers for repository services.

use super::*;

pub(super) fn system_time_to_rfc3339(value: SystemTime) -> Result<String, time::error::Format> {
    let datetime: OffsetDateTime = value.into();
    datetime.format(&Rfc3339)
}

pub(super) fn format_size_label(size_bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let size = size_bytes as f64;

    if size >= GB {
        format!("{:.1} GB", size / GB)
    } else if size >= MB {
        format!("{:.1} MB", size / MB)
    } else if size >= KB {
        format!("{:.1} KB", size / KB)
    } else {
        format!("{size_bytes} B")
    }
}

pub(super) fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

pub(super) fn db_error(error: rusqlite::Error) -> String {
    format!("database error: {error}")
}

pub(super) fn io_error(error: std::io::Error) -> String {
    format!("io error: {error}")
}

pub(super) fn is_skippable_filesystem_error(error: &std::io::Error) -> bool {
    matches!(error.kind(), std::io::ErrorKind::PermissionDenied)
}

pub(super) fn path_error(error: std::path::StripPrefixError) -> String {
    format!("path error: {error}")
}

pub(super) fn json_error(error: serde_json::Error) -> String {
    format!("json error: {error}")
}

pub(super) fn time_error(error: time::error::Format) -> String {
    format!("time error: {error}")
}

pub(super) fn safe_prefix(value: &str, max_chars: usize) -> &str {
    let end = value
        .char_indices()
        .nth(max_chars)
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    &value[..end]
}
