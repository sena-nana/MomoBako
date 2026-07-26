//! 应用级日志中心，负责统一落盘、内存缓存与实时事件广播。

use crate::services::repository::{
    SystemLogLocation, SystemLogPage, SystemLogQuery, SystemLogRecord, SystemLogSource,
    SystemLogWriteRequest,
};
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const LOG_EVENT_NAME: &str = "system://log-recorded";
const LOG_DIR_NAME: &str = "logs";
const CURRENT_LOG_FILE_NAME: &str = "system.current.jsonl";
const ARCHIVED_LOG_FILE_PREFIX: &str = "system.";
const ARCHIVED_LOG_FILE_SUFFIX: &str = ".jsonl";
const MAX_IN_MEMORY_LOGS: usize = 500;
const MAX_LOG_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ARCHIVED_LOG_FILES: usize = 5;

static GLOBAL_LOGGER: OnceLock<Mutex<Option<Arc<AppLogger>>>> = OnceLock::new();

#[derive(Default)]
struct LoggerState {
    recent: VecDeque<SystemLogRecord>,
    app_handle: Option<AppHandle>,
}

/// 应用全局日志器。
pub struct AppLogger {
    service_root: PathBuf,
    sequence: AtomicU64,
    state: Mutex<LoggerState>,
}

impl AppLogger {
    /// 创建日志器并回填最近日志缓存。
    pub fn new(service_root: PathBuf) -> Result<Self, String> {
        let logger = Self {
            service_root,
            sequence: AtomicU64::new(0),
            state: Mutex::new(LoggerState::default()),
        };
        logger.load_recent_logs_from_disk()?;
        Ok(logger)
    }

    /// 绑定 Tauri 应用句柄，供日志事件推送使用。
    pub fn set_app_handle(&self, app_handle: AppHandle) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "logger state lock poisoned".to_string())?;
        state.app_handle = Some(app_handle);
        Ok(())
    }

    /// 写入一条标准化日志。
    pub fn write(&self, request: SystemLogWriteRequest) -> Result<SystemLogRecord, String> {
        let record = self.build_record(request)?;
        let raw = serde_json::to_string(&record).map_err(|error| error.to_string())?;
        let app_handle = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "logger state lock poisoned".to_string())?;
            self.rotate_if_needed(raw.len() as u64 + 1)?;
            self.append_raw_line(&raw)?;
            push_recent_log(&mut state.recent, record.clone());
            state.app_handle.clone()
        };
        if let Some(app_handle) = app_handle {
            let _ = app_handle.emit(LOG_EVENT_NAME, &record);
        }
        Ok(record)
    }

    /// 查询日志列表。
    pub fn list(&self, query: Option<SystemLogQuery>) -> Result<SystemLogPage, String> {
        let query = query.unwrap_or_default();
        let limit = query.limit.unwrap_or(200).clamp(1, 500);
        let mut records = self.load_all_logs_from_disk()?;
        records.sort_by(|left, right| {
            right
                .timestamp
                .cmp(&left.timestamp)
                .then_with(|| right.id.cmp(&left.id))
        });
        records.retain(|record| record_matches_query(record, &query));
        let next_cursor = if records.len() > limit {
            records.get(limit).map(|record| record.timestamp.clone())
        } else {
            None
        };
        records.truncate(limit);
        Ok(SystemLogPage {
            records,
            next_cursor,
        })
    }

    /// 清空内存缓存与所有落盘文件。
    pub fn clear(&self) -> Result<(), String> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "logger state lock poisoned".to_string())?;
            state.recent.clear();
        }
        let log_dir = self.log_dir();
        if !log_dir.is_dir() {
            return Ok(());
        }
        for path in self.log_paths() {
            if path.is_file() {
                fs::remove_file(path).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    fn build_record(&self, request: SystemLogWriteRequest) -> Result<SystemLogRecord, String> {
        let timestamp = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| error.to_string())?;
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis();
        Ok(SystemLogRecord {
            id: format!("log-{timestamp_ms}-{sequence}"),
            timestamp,
            level: request.level.trim().to_string(),
            category: request.category.trim().to_string(),
            action: request.action.trim().to_string(),
            message: request.message.trim().to_string(),
            source: SystemLogSource {
                kind: request.source_kind.unwrap_or_else(|| "host".to_string()),
                label: request.source_label,
                plugin_id: request.plugin_id,
                repo_id: request.repo_id,
            },
            location: SystemLogLocation {
                module_path: request
                    .location
                    .as_ref()
                    .and_then(|value| value.module_path.clone()),
                file: request
                    .location
                    .as_ref()
                    .and_then(|value| value.file.clone()),
                line: request.location.as_ref().and_then(|value| value.line),
            },
            context: request.context.unwrap_or_else(|| serde_json::json!({})),
        })
    }

    fn load_recent_logs_from_disk(&self) -> Result<(), String> {
        let mut records = self.load_all_logs_from_disk()?;
        records.sort_by(|left, right| {
            left.timestamp
                .cmp(&right.timestamp)
                .then_with(|| left.id.cmp(&right.id))
        });
        let mut state = self
            .state
            .lock()
            .map_err(|_| "logger state lock poisoned".to_string())?;
        state.recent.clear();
        let keep_from = records.len().saturating_sub(MAX_IN_MEMORY_LOGS);
        for record in records.into_iter().skip(keep_from) {
            state.recent.push_back(record);
        }
        Ok(())
    }

    fn load_all_logs_from_disk(&self) -> Result<Vec<SystemLogRecord>, String> {
        let mut records = Vec::new();
        for path in self.log_paths() {
            if !path.is_file() {
                continue;
            }
            let file = File::open(&path).map_err(|error| error.to_string())?;
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let line = line.map_err(|error| error.to_string())?;
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(record) = serde_json::from_str::<SystemLogRecord>(&line) {
                    records.push(record);
                }
            }
        }
        Ok(records)
    }

    fn rotate_if_needed(&self, pending_len: u64) -> Result<(), String> {
        let current_path = self.current_log_path();
        if !current_path.is_file() {
            return Ok(());
        }
        let current_len = fs::metadata(&current_path)
            .map_err(|error| error.to_string())?
            .len();
        if current_len + pending_len <= MAX_LOG_FILE_BYTES {
            return Ok(());
        }
        let log_dir = self.log_dir();
        fs::create_dir_all(&log_dir).map_err(|error| error.to_string())?;
        let overflow_path = self.archived_log_path(MAX_ARCHIVED_LOG_FILES);
        if overflow_path.is_file() {
            fs::remove_file(&overflow_path).map_err(|error| error.to_string())?;
        }
        for index in (1..MAX_ARCHIVED_LOG_FILES).rev() {
            let from = self.archived_log_path(index);
            let to = self.archived_log_path(index + 1);
            if from.is_file() {
                fs::rename(from, to).map_err(|error| error.to_string())?;
            }
        }
        if current_path.is_file() {
            fs::rename(current_path, self.archived_log_path(1))
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn append_raw_line(&self, raw: &str) -> Result<(), String> {
        let log_dir = self.log_dir();
        fs::create_dir_all(&log_dir).map_err(|error| error.to_string())?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.current_log_path())
            .map_err(|error| error.to_string())?;
        writeln!(file, "{raw}").map_err(|error| error.to_string())
    }

    fn log_dir(&self) -> PathBuf {
        self.service_root.join(LOG_DIR_NAME)
    }

    fn current_log_path(&self) -> PathBuf {
        self.log_dir().join(CURRENT_LOG_FILE_NAME)
    }

    fn archived_log_path(&self, index: usize) -> PathBuf {
        self.log_dir().join(format!(
            "{ARCHIVED_LOG_FILE_PREFIX}{index}{ARCHIVED_LOG_FILE_SUFFIX}"
        ))
    }

    fn log_paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![self.current_log_path()];
        for index in 1..=MAX_ARCHIVED_LOG_FILES {
            paths.push(self.archived_log_path(index));
        }
        paths
    }
}

fn push_recent_log(buffer: &mut VecDeque<SystemLogRecord>, record: SystemLogRecord) {
    buffer.push_back(record);
    while buffer.len() > MAX_IN_MEMORY_LOGS {
        buffer.pop_front();
    }
}

fn record_matches_query(record: &SystemLogRecord, query: &SystemLogQuery) -> bool {
    if let Some(before) = query.before.as_deref() {
        if record.timestamp.as_str() >= before {
            return false;
        }
    }
    if !query.levels.is_empty() && !query.levels.iter().any(|level| level == &record.level) {
        return false;
    }
    if !query.source_kinds.is_empty()
        && !query
            .source_kinds
            .iter()
            .any(|kind| kind == &record.source.kind)
    {
        return false;
    }
    if let Some(plugin_id) = query.plugin_id.as_deref() {
        if record.source.plugin_id.as_deref() != Some(plugin_id) {
            return false;
        }
    }
    if let Some(repo_id) = query.repo_id.as_deref() {
        if record.source.repo_id.as_deref() != Some(repo_id) {
            return false;
        }
    }
    if let Some(term) = query.query.as_deref() {
        let needle = term.trim().to_lowercase();
        if !needle.is_empty() {
            let context = serde_json::to_string(&record.context).unwrap_or_default();
            let haystack = [
                record.category.as_str(),
                record.action.as_str(),
                record.message.as_str(),
                record.source.label.as_deref().unwrap_or_default(),
                record.source.plugin_id.as_deref().unwrap_or_default(),
                record.source.repo_id.as_deref().unwrap_or_default(),
                record.location.module_path.as_deref().unwrap_or_default(),
                record.location.file.as_deref().unwrap_or_default(),
                context.as_str(),
            ]
            .join("\n")
            .to_lowercase();
            if !haystack.contains(&needle) {
                return false;
            }
        }
    }
    true
}

pub fn init_app_logger(service_root: PathBuf) -> Result<Arc<AppLogger>, String> {
    let logger = Arc::new(AppLogger::new(service_root)?);
    set_global_logger(Some(logger.clone()))?;
    Ok(logger)
}

pub fn set_global_logger(logger: Option<Arc<AppLogger>>) -> Result<(), String> {
    let slot = GLOBAL_LOGGER.get_or_init(|| Mutex::new(None));
    let mut guard = slot
        .lock()
        .map_err(|_| "global logger lock poisoned".to_string())?;
    *guard = logger;
    Ok(())
}

pub fn global_logger() -> Option<Arc<AppLogger>> {
    GLOBAL_LOGGER
        .get()
        .and_then(|slot| slot.lock().ok().and_then(|guard| guard.clone()))
}

pub fn write_log(request: SystemLogWriteRequest) -> Result<SystemLogRecord, String> {
    let logger = global_logger().ok_or_else(|| "app logger is unavailable".to_string())?;
    logger.write(request)
}

pub fn list_logs(query: Option<SystemLogQuery>) -> Result<SystemLogPage, String> {
    let logger = global_logger().ok_or_else(|| "app logger is unavailable".to_string())?;
    logger.list(query)
}

pub fn clear_logs() -> Result<(), String> {
    let logger = global_logger().ok_or_else(|| "app logger is unavailable".to_string())?;
    logger.clear()
}

/// 使用调用点元信息输出宿主日志。
#[macro_export]
macro_rules! app_log {
    ($level:expr, $category:expr, $action:expr, $message:expr) => {{
        let _ = $crate::services::logging::write_log(
            $crate::services::repository::SystemLogWriteRequest {
                level: $level.to_string(),
                category: $category.to_string(),
                action: $action.to_string(),
                message: $message.to_string(),
                context: None,
                repo_id: None,
                plugin_id: None,
                source_kind: Some("host".to_string()),
                source_label: None,
                location: Some($crate::services::repository::SystemLogLocationInput {
                    module_path: Some(module_path!().to_string()),
                    file: Some(file!().to_string()),
                    line: Some(line!()),
                }),
            },
        );
    }};
    ($level:expr, $category:expr, $action:expr, $message:expr, $context:expr) => {{
        let _ = $crate::services::logging::write_log(
            $crate::services::repository::SystemLogWriteRequest {
                level: $level.to_string(),
                category: $category.to_string(),
                action: $action.to_string(),
                message: $message.to_string(),
                context: Some($context),
                repo_id: None,
                plugin_id: None,
                source_kind: Some("host".to_string()),
                source_label: None,
                location: Some($crate::services::repository::SystemLogLocationInput {
                    module_path: Some(module_path!().to_string()),
                    file: Some(file!().to_string()),
                    line: Some(line!()),
                }),
            },
        );
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        std::env::temp_dir().join(format!("momobako-log-test-{name}-{unique}"))
    }

    fn sample_request(message: &str) -> SystemLogWriteRequest {
        SystemLogWriteRequest {
            level: "info".to_string(),
            category: "test".to_string(),
            action: "write".to_string(),
            message: message.to_string(),
            context: Some(serde_json::json!({ "value": message })),
            repo_id: Some("repo-1".to_string()),
            plugin_id: Some("plugin-1".to_string()),
            source_kind: Some("backend-plugin".to_string()),
            source_label: Some("测试插件".to_string()),
            location: Some(crate::services::repository::SystemLogLocationInput {
                module_path: Some("tests.module".to_string()),
                file: Some("tests.rs".to_string()),
                line: Some(42),
            }),
        }
    }

    #[test]
    fn write_list_and_clear_logs() {
        let root = test_root("write-list-clear");
        let logger = AppLogger::new(root.clone()).expect("logger should initialize");
        let first = logger
            .write(sample_request("hello"))
            .expect("first record should write");
        let second = logger
            .write(sample_request("world"))
            .expect("second record should write");

        let page = logger
            .list(Some(SystemLogQuery {
                limit: Some(10),
                ..SystemLogQuery::default()
            }))
            .expect("logs should list");
        assert_eq!(page.records.len(), 2);
        assert_eq!(page.records[0].id, second.id);
        assert_eq!(page.records[1].id, first.id);

        logger.clear().expect("logs should clear");
        let cleared = logger
            .list(Some(SystemLogQuery {
                limit: Some(10),
                ..SystemLogQuery::default()
            }))
            .expect("cleared logs should list");
        assert!(cleared.records.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn list_filters_logs_by_query_fields() {
        let root = test_root("filters");
        let logger = AppLogger::new(root.clone()).expect("logger should initialize");
        logger
            .write(sample_request("alpha"))
            .expect("alpha should write");
        let mut beta = sample_request("beta");
        beta.level = "error".to_string();
        beta.repo_id = Some("repo-2".to_string());
        beta.plugin_id = Some("plugin-2".to_string());
        beta.source_kind = Some("frontend-plugin".to_string());
        logger.write(beta).expect("beta should write");

        let filtered = logger
            .list(Some(SystemLogQuery {
                limit: Some(10),
                levels: vec!["error".to_string()],
                source_kinds: vec!["frontend-plugin".to_string()],
                plugin_id: Some("plugin-2".to_string()),
                repo_id: Some("repo-2".to_string()),
                query: Some("beta".to_string()),
                before: None,
            }))
            .expect("filtered logs should list");
        assert_eq!(filtered.records.len(), 1);
        assert_eq!(filtered.records[0].message, "beta");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rotate_log_files_when_current_file_is_full() {
        let root = test_root("rotation");
        let logger = AppLogger::new(root.clone()).expect("logger should initialize");
        let log_dir = logger.log_dir();
        fs::create_dir_all(&log_dir).expect("log dir should create");
        fs::write(
            logger.current_log_path(),
            vec![b'x'; (MAX_LOG_FILE_BYTES + 16) as usize],
        )
        .expect("current log should seed");
        logger
            .write(sample_request("rotate"))
            .expect("record should rotate and write");
        assert!(logger.archived_log_path(1).is_file());
        assert!(logger.current_log_path().is_file());
        let _ = fs::remove_dir_all(root);
    }
}
