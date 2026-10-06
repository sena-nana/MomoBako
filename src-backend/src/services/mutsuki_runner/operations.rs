//! Mutsuki 长任务到现有 RepositoryService 的领域调用适配。

use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard, TryLockError},
    thread,
    time::Duration,
};

use mutsuki_runtime_contracts::{DomainEvent, RunnerResult, RuntimeError, ScalarValue, Task};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;

use super::protocols::*;
use crate::services::repository::{
    download_playlist_with_progress_cancellable, CancellationCheck, DownloaderPlaylistRequest,
    EntryPlaybackProgressEvent, EntryPlaybackRequest, EntryPlaybackSourceResponse,
    FileDeleteRequest,
};
use crate::services::runtime::{sync_watched_paths, RepositoryRuntime};

const ERROR_INVALID_PAYLOAD: &str = "momobako.invalid_payload";
const ERROR_OPERATION_FAILED: &str = "momobako.operation_failed";

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum EntryDeleteOperationRequest {
    Single(FileDeleteRequest),
    Batch {
        #[serde(rename = "repoId")]
        repo_id: String,
        paths: Vec<String>,
        mode: Option<String>,
    },
}

/// 复用 RepositoryRuntime 的写锁与 watcher 同步语义。
pub(super) struct RepositoryTaskExecutor<'a> {
    runtime: &'a RepositoryRuntime,
    cancellation: &'a dyn CancellationCheck,
}

impl<'a> RepositoryTaskExecutor<'a> {
    pub(super) fn new(
        runtime: &'a RepositoryRuntime,
        cancellation: &'a dyn CancellationCheck,
    ) -> Self {
        Self {
            runtime,
            cancellation,
        }
    }

    /// 在资源库全局写锁内执行普通领域写操作。
    fn write<T>(
        &self,
        operation: impl FnOnce(&crate::services::repository::RepositoryState) -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = acquire_cancellable_lock(&self.runtime.write_lock, self.cancellation)?;
        self.cancellation.checkpoint()?;
        operation(&self.runtime.repository_state)
    }

    /// 执行会改变资源库集合的操作，并刷新 watcher 监听根目录。
    fn collection_write<T>(
        &self,
        operation: impl FnOnce(&crate::services::repository::RepositoryState) -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = acquire_cancellable_lock(&self.runtime.write_lock, self.cancellation)?;
        self.cancellation.checkpoint()?;
        let response = operation(&self.runtime.repository_state)?;
        sync_watched_paths(&self.runtime.repository_state, &self.runtime.watcher_handle)?;
        self.cancellation.checkpoint()?;
        Ok(response)
    }

    /// 调用已有 RepositoryService，并把结果编码为 Core inline output。
    pub(super) fn execute(
        &self,
        task: &Task,
        events: &mut Vec<DomainEvent>,
    ) -> Result<RunnerResult, RuntimeError> {
        match task.protocol_id.as_str() {
            PROTOCOL_REPOSITORY_CREATE => finish_operation(
                task,
                events,
                self.collection_write(|state| state.create_repository(decode_request(task)?)),
            ),
            PROTOCOL_REPOSITORY_IMPORT => finish_operation(
                task,
                events,
                self.collection_write(|state| state.import_repository(decode_request(task)?)),
            ),
            PROTOCOL_REPOSITORY_ATTACH => finish_operation(
                task,
                events,
                self.collection_write(|state| {
                    state.attach_repository_folder(decode_request(task)?)
                }),
            ),
            PROTOCOL_REPOSITORY_RELOCATE => finish_operation(
                task,
                events,
                self.collection_write(|state| state.relocate_repository(decode_request(task)?)),
            ),
            PROTOCOL_REPOSITORY_EXPORT => finish_operation(
                task,
                events,
                self.write(|state| state.export_repository(decode_request(task)?)),
            ),
            PROTOCOL_REPOSITORY_SYNC => finish_operation(
                task,
                events,
                self.write(|state| {
                    state.sync_repository_cancellable(decode_request(task)?, self.cancellation)
                }),
            ),
            PROTOCOL_ENTRY_IMPORT => finish_operation(
                task,
                events,
                self.write(|state| {
                    state.import_entries_cancellable(decode_request(task)?, self.cancellation)
                }),
            ),
            PROTOCOL_ARCHIVE_IMPORT => finish_operation(
                task,
                events,
                self.write(|state| {
                    state.import_archive_entries_cancellable(
                        decode_request(task)?,
                        self.cancellation,
                    )
                }),
            ),
            PROTOCOL_EAGLE_IMPORT => finish_operation(
                task,
                events,
                self.write(|state| {
                    state.import_eagle_library_cancellable(decode_request(task)?, self.cancellation)
                }),
            ),
            PROTOCOL_ENTRY_COPY => finish_operation(
                task,
                events,
                self.write(|state| {
                    state.copy_entries_cancellable(decode_request(task)?, self.cancellation)
                }),
            ),
            PROTOCOL_ENTRY_MOVE => finish_operation(
                task,
                events,
                self.write(|state| {
                    state.move_entries_cancellable(decode_request(task)?, self.cancellation)
                }),
            ),
            PROTOCOL_ENTRY_DELETE => {
                let request = decode_request::<EntryDeleteOperationRequest>(task)
                    .map_err(|error| operation_error(task, error))?;
                finish_operation(
                    task,
                    events,
                    self.write(|state| match request {
                        EntryDeleteOperationRequest::Single(request) => {
                            serde_json::to_value(state.delete_entry(request)?)
                                .map_err(|error| error.to_string())
                        }
                        EntryDeleteOperationRequest::Batch {
                            repo_id,
                            paths,
                            mode,
                        } => {
                            if paths.is_empty() {
                                return Err("no paths were provided for batch deletion".to_string());
                            }
                            let deleted_count = paths.len();
                            let mut snapshot = None;
                            for path in paths {
                                self.cancellation.checkpoint()?;
                                snapshot = Some(state.delete_entry(FileDeleteRequest {
                                    repo_id: repo_id.clone(),
                                    path,
                                    mode: mode.clone(),
                                })?);
                            }
                            self.cancellation.checkpoint()?;
                            Ok(json!({
                                "repoId": repo_id,
                                "deletedCount": deleted_count,
                                "snapshot": snapshot,
                            }))
                        }
                    }),
                )
            }
            PROTOCOL_REPOSITORY_ACTION_RUN => finish_operation(
                task,
                events,
                self.write(|state| state.run_repository_action(decode_request(task)?)),
            ),
            PROTOCOL_THUMBNAIL_REQUEST => finish_operation(
                task,
                events,
                self.write(|state| state.ensure_thumbnail(decode_request(task)?)),
            ),
            PROTOCOL_PLAYBACK_PREPARE => self.prepare_playback(task, events),
            PROTOCOL_PLAYLIST_DOWNLOAD => self.download_playlist(task, events),
            protocol_id => Err(runtime_error(
                "task.unsupported",
                protocol_id,
                "内建长任务 runner 不支持该协议。",
            )),
        }
    }

    /// 准备播放源，并把插件进度转换为 Mutsuki 领域事件。
    fn prepare_playback(
        &self,
        task: &Task,
        events: &mut Vec<DomainEvent>,
    ) -> Result<RunnerResult, RuntimeError> {
        let request = decode_request::<EntryPlaybackRequest>(task)
            .map_err(|error| operation_error(task, error))?;
        let mut response = self
            .write(|state| {
                let mut emit = |progress: EntryPlaybackProgressEvent| {
                    self.cancellation.checkpoint()?;
                    push_progress_event(events, task, progress)
                };
                let response =
                    state.prepare_entry_playback_source_with_progress(request, &mut emit)?;
                self.cancellation.checkpoint()?;
                Ok(response)
            })
            .map_err(|error| operation_error(task, error))?;
        self.cancellation
            .checkpoint()
            .map_err(|error| operation_error(task, error))?;
        self.attach_preview_urls(&mut response)
            .map_err(|error| operation_error(task, error))?;
        encode_result(task, response, events)
    }

    /// 下载播放列表，并保留现有下载器的逐项进度数据。
    fn download_playlist(
        &self,
        task: &Task,
        events: &mut Vec<DomainEvent>,
    ) -> Result<RunnerResult, RuntimeError> {
        let request = decode_request::<DownloaderPlaylistRequest>(task)
            .map_err(|error| operation_error(task, error))?;
        let mut emit = |progress| {
            self.cancellation.checkpoint()?;
            push_progress_event(events, task, progress)
        };
        let response = download_playlist_with_progress_cancellable(
            &self.runtime.repository_state,
            request,
            self.cancellation,
            &mut emit,
        )
        .map_err(|error| operation_error(task, error))?;
        encode_result(task, response, events)
    }

    /// 为本地播放文件注册预览 URL，保持原 ViewModel 的返回契约。
    fn attach_preview_urls(
        &self,
        response: &mut EntryPlaybackSourceResponse,
    ) -> Result<(), String> {
        self.cancellation.checkpoint()?;
        if response.source_url.is_none() {
            let path = response
                .local_path
                .as_deref()
                .or(response.temp_file_path.as_deref())
                .map(PathBuf::from);
            if let Some(path) = path {
                self.cancellation.checkpoint()?;
                let token = self
                    .runtime
                    .repository_state
                    .register_preview_source_path(path, &response.media_type)?;
                response.source_url = Some(self.runtime.preview_source_url(&token));
            }
        }
        attach_text_preview(
            self.runtime,
            self.cancellation,
            &mut response.lyric_source_url,
            response.lyric_path.as_deref(),
        )?;
        attach_text_preview(
            self.runtime,
            self.cancellation,
            &mut response.word_lyric_source_url,
            response.word_lyric_path.as_deref(),
        )
    }
}

fn acquire_cancellable_lock<'a>(
    lock: &'a Mutex<()>,
    cancellation: &dyn CancellationCheck,
) -> Result<MutexGuard<'a, ()>, String> {
    loop {
        cancellation.checkpoint()?;
        match lock.try_lock() {
            Ok(guard) => {
                cancellation.checkpoint()?;
                return Ok(guard);
            }
            Err(TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(10)),
            Err(TryLockError::Poisoned(_)) => {
                return Err("repository write lock poisoned".to_string())
            }
        }
    }
}

fn finish_operation(
    task: &Task,
    events: &mut Vec<DomainEvent>,
    operation: Result<impl Serialize, String>,
) -> Result<RunnerResult, RuntimeError> {
    let output = operation.map_err(|error| operation_error(task, error))?;
    encode_result(task, output, events)
}

/// 解析任务 payload，错误保留协议和 serde 原因。
fn decode_request<T: DeserializeOwned>(task: &Task) -> Result<T, String> {
    serde_json::from_value(task.payload.to_value())
        .map_err(|error| format!("invalid {} payload: {error}", task.protocol_id))
}

/// 将领域响应写入任务的 inline output。
fn encode_result(
    task: &Task,
    output: impl Serialize,
    events: &mut Vec<DomainEvent>,
) -> Result<RunnerResult, RuntimeError> {
    let output = serde_json::to_value(output).map_err(|error| {
        runtime_error(
            ERROR_OPERATION_FAILED,
            &task.protocol_id,
            format!("序列化长任务结果失败：{error}"),
        )
    })?;
    let mut result = RunnerResult::completed(&task.task_id);
    result.output = Some(output);
    result.events = std::mem::take(events);
    Ok(result)
}

/// 将原业务进度事件保存在任务结果中，由 Core 统一关联 task handle。
fn push_progress_event(
    events: &mut Vec<DomainEvent>,
    task: &Task,
    progress: impl Serialize,
) -> Result<(), String> {
    let payload = serde_json::to_value(progress).map_err(|error| error.to_string())?;
    events.push(DomainEvent {
        event_id: format!("{}:progress:{}", task.task_id, events.len() + 1),
        kind: "momobako.task.progress".to_string(),
        payload: json!({
            "taskId": task.task_id,
            "protocolId": task.protocol_id,
            "progress": payload,
        }),
    });
    Ok(())
}

fn attach_text_preview(
    runtime: &RepositoryRuntime,
    cancellation: &dyn CancellationCheck,
    target_url: &mut Option<String>,
    source_path: Option<&str>,
) -> Result<(), String> {
    if target_url.is_some() {
        return Ok(());
    }
    let Some(source_path) = source_path else {
        return Ok(());
    };
    cancellation.checkpoint()?;
    let token = runtime
        .repository_state
        .register_preview_source_path(PathBuf::from(source_path), "text/plain; charset=utf-8")?;
    *target_url = Some(runtime.preview_source_url(&token));
    Ok(())
}

fn operation_error(task: &Task, error: String) -> RuntimeError {
    let code = if error.starts_with("invalid ") {
        ERROR_INVALID_PAYLOAD
    } else {
        ERROR_OPERATION_FAILED
    };
    runtime_error(code, &task.protocol_id, error)
}

fn runtime_error(code: &str, protocol_id: &str, message: impl Into<String>) -> RuntimeError {
    let message = message.into();
    let mut error = RuntimeError::new(
        code,
        "momobako.builtin.long_task",
        format!("protocol.{protocol_id}"),
    );
    error
        .evidence
        .insert("message".to_string(), ScalarValue::String(message));
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::repository::RepositoryFolderRequest;
    use serde_json::Value;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    struct TestCancellation(Arc<AtomicBool>);

    impl CancellationCheck for TestCancellation {
        fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::Acquire)
        }
    }

    #[test]
    fn invalid_payload_is_structured() {
        let task = Task::new(
            "task-invalid",
            PROTOCOL_REPOSITORY_ATTACH,
            Value::String("not-an-object".to_string()),
        );
        let error = decode_request::<RepositoryFolderRequest>(&task)
            .map_err(|cause| operation_error(&task, cause))
            .expect_err("invalid payload should fail");
        assert_eq!(error.code, ERROR_INVALID_PAYLOAD);
        assert_eq!(
            error.evidence.get("message"),
            Some(&ScalarValue::String(
                "invalid momobako.repository.attach payload: invalid type: string \"not-an-object\", expected struct RepositoryFolderRequest"
                    .to_string()
            ))
        );
    }

    #[test]
    fn cancellation_while_waiting_for_write_lock_prevents_acquisition() {
        let lock = Arc::new(Mutex::new(()));
        let held = lock.lock().expect("test lock should be available");
        let requested = Arc::new(AtomicBool::new(false));
        let worker_lock = lock.clone();
        let worker_requested = requested.clone();
        let worker = thread::spawn(move || {
            acquire_cancellable_lock(&worker_lock, &TestCancellation(worker_requested)).map(|_| ())
        });

        thread::sleep(Duration::from_millis(25));
        requested.store(true, Ordering::Release);
        drop(held);

        assert_eq!(
            worker.join().expect("worker should exit").unwrap_err(),
            "repository operation cancelled"
        );
        assert!(lock.try_lock().is_ok());
    }
}
