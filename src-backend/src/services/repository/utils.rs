//! Shared formatting, time, and error helpers for repository services.

use super::*;

const SQLITE_BUSY_TIMEOUT_MS: u64 = 5_000;

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

fn configure_sqlite_connection_defaults(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.busy_timeout(Duration::from_millis(SQLITE_BUSY_TIMEOUT_MS))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn ensure_sqlite_journal_mode(
    connection: &Connection,
    target_mode: &str,
) -> Result<(), rusqlite::Error> {
    let current_mode: String =
        connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if current_mode.eq_ignore_ascii_case(target_mode) {
        return Ok(());
    }
    connection.pragma_update(None, "journal_mode", target_mode)?;
    Ok(())
}

pub(super) fn configure_registry_connection(
    connection: &Connection,
) -> Result<(), rusqlite::Error> {
    configure_sqlite_connection_defaults(connection)?;
    match ensure_sqlite_journal_mode(connection, "WAL") {
        Ok(()) => Ok(()),
        Err(error) => {
            if should_fallback_repository_journal_mode(&error) {
                ensure_sqlite_journal_mode(connection, "DELETE")?;
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}

pub(super) fn configure_repository_connection(
    connection: &Connection,
) -> Result<(), rusqlite::Error> {
    configure_sqlite_connection_defaults(connection)?;
    match ensure_sqlite_journal_mode(connection, "WAL") {
        Ok(()) => Ok(()),
        Err(error) => {
            if should_fallback_repository_journal_mode(&error) {
                ensure_sqlite_journal_mode(connection, "DELETE")?;
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}

pub(super) fn should_fallback_repository_journal_mode(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(sqlite_error, _)
            if sqlite_error.code == rusqlite::ffi::ErrorCode::FileLockingProtocolFailed
    ) || error.to_string().contains("locking protocol")
}

#[cfg(test)]
pub(super) fn is_database_locked_error(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(sqlite_error, _)
            if sqlite_error.code == rusqlite::ffi::ErrorCode::DatabaseBusy
                || sqlite_error.code == rusqlite::ffi::ErrorCode::DatabaseLocked
    ) || is_database_locked_error_message(&error.to_string())
}

pub(super) fn is_database_locked_error_message(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("database is locked")
        || normalized.contains("database table is locked")
        || normalized.contains("database is busy")
}

pub(super) fn open_registry_connection(registry_path: &Path) -> Result<Connection, String> {
    let connection = Connection::open(registry_path).map_err(db_error)?;
    configure_registry_connection(&connection).map_err(db_error)?;
    Ok(connection)
}

pub(super) fn open_repository_database_connection(
    database_path: &Path,
) -> Result<Connection, String> {
    let connection = Connection::open(database_path).map_err(db_error)?;
    ensure_repository_schema_current(&connection).map_err(db_error)?;
    Ok(connection)
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
