//! aria2 运行时的内部测试。

use super::*;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    fn new(name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "momobako-aria2-runtime-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("test root should be created");
        Self { root }
    }

    fn path(&self, child: &str) -> PathBuf {
        self.root.join(child)
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn start_mock_aria2_rpc_server(
    handler: impl Fn(&serde_json::Value) -> serde_json::Value + Send + 'static,
) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("mock rpc server should bind");
    let address = listener
        .local_addr()
        .expect("mock rpc server address should resolve");
    std::thread::spawn(move || {
        for _ in 0..4 {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).unwrap_or(0);
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request_text = String::from_utf8_lossy(&request);
            let content_length = request_text
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| index + 4)
                .unwrap_or(request.len());
            let mut body = request[header_end..].to_vec();
            while body.len() < content_length {
                let read = stream.read(&mut buffer).unwrap_or(0);
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&buffer[..read]);
            }
            let payload = serde_json::from_slice::<serde_json::Value>(&body)
                .expect("mock rpc request body should decode");
            let response_body =
                serde_json::to_vec(&handler(&payload)).expect("mock rpc response should encode");
            let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response_body.len()
                );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&response_body);
        }
    });
    format!("http://{}/jsonrpc", address)
}

fn write_mock_running_status(paths: &Aria2Paths, plugin_data_dir: &Path, rpc_url: &str) {
    fs::create_dir_all(&paths.helper_dir).expect("helper dir should be created");
    fs::write(&paths.pid_path, std::process::id().to_string()).expect("pid file should be written");
    save_status(
        &paths.status_path,
        &Aria2StatusRecord {
            running: true,
            pid: Some(std::process::id()),
            executable_path: Some("C:/Mock/aria2c.exe".to_string()),
            version: Some("1.37.0".to_string()),
            rpc_url: Some(rpc_url.to_string()),
            secret: Some("secret".to_string()),
            source: Some("test".to_string()),
            updated_at: Some("2026-07-01T10:00:00Z".to_string()),
            error: None,
            download_url: "https://example.test/aria2.zip".to_string(),
            bundled_archive_path: Some(
                plugin_data_dir
                    .join("downloads")
                    .join("aria2-runtime.zip")
                    .to_string_lossy()
                    .to_string(),
            ),
        },
    )
    .expect("status should be written");
}

#[test]
fn update_task_from_status_payload_maps_complete_to_completed() {
    let mut record = DownloadTaskRecord {
        task_id: "gid-1".to_string(),
        gid: "gid-1".to_string(),
        url: "http://localhost/test.mp3".to_string(),
        destination_path: "C:/Temp/test.mp3".to_string(),
        metadata: None,
        status: "queued".to_string(),
        created_at: "2026-07-01T10:00:00Z".to_string(),
        finished_at: None,
        total_length: None,
        completed_length: None,
        error: None,
    };
    update_task_from_status_payload(
        &mut record,
        &serde_json::json!({
            "status": "complete",
            "totalLength": "123",
            "completedLength": "123"
        }),
    )
    .expect("status payload should update task");
    assert_eq!(record.status, "completed");
    assert_eq!(record.total_length, Some(123));
    assert_eq!(record.completed_length, Some(123));
    assert!(record.finished_at.is_some());
}

#[test]
fn update_task_from_status_payload_maps_error_to_failed() {
    let mut record = DownloadTaskRecord {
        task_id: "gid-2".to_string(),
        gid: "gid-2".to_string(),
        url: "http://localhost/test.zip".to_string(),
        destination_path: "C:/Temp/test.zip".to_string(),
        metadata: None,
        status: "active".to_string(),
        created_at: "2026-07-01T10:00:00Z".to_string(),
        finished_at: None,
        total_length: None,
        completed_length: None,
        error: None,
    };
    update_task_from_status_payload(
        &mut record,
        &serde_json::json!({
            "status": "error",
            "totalLength": "123",
            "completedLength": "12",
            "errorMessage": "network failed"
        }),
    )
    .expect("status payload should update task");
    assert_eq!(record.status, "failed");
    assert_eq!(record.total_length, Some(123));
    assert_eq!(record.completed_length, Some(12));
    assert_eq!(record.error.as_deref(), Some("network failed"));
    assert!(record.finished_at.is_some());
}

#[test]
fn runtime_status_returns_idle_snapshot_and_counts_persisted_tasks() {
    let workspace = TestWorkspace::new("idle-snapshot");
    let plugin_data_dir = workspace.path("plugin-data");
    let paths = runtime_paths(&plugin_data_dir);
    fs::create_dir_all(&paths.tasks_dir).expect("tasks dir should be created");
    fs::write(
            task_path(&paths.tasks_dir, "task-1"),
            r#"{"taskId":"task-1","gid":"gid-1","url":"http://127.0.0.1/file-1.zip","destinationPath":"C:/Temp/file-1.zip","status":"queued","createdAt":"2026-07-01T10:00:00Z"}"#,
        )
        .expect("first task should be written");
    fs::write(
            task_path(&paths.tasks_dir, "task-2"),
            r#"{"taskId":"task-2","gid":"gid-2","url":"http://127.0.0.1/file-2.zip","destinationPath":"C:/Temp/file-2.zip","status":"active","createdAt":"2026-07-01T10:01:00Z"}"#,
        )
        .expect("second task should be written");

    let snapshot = runtime_status(&Aria2Config {
        host_runtime: None,
        plugin_data_dir: &plugin_data_dir,
        download_url: "https://example.test/aria2.zip",
    })
    .expect("runtime status should resolve idle snapshot");

    assert_eq!(snapshot.queue_size, 2);
    assert_eq!(snapshot.downloads_dir, plugin_data_dir.join("downloads"));
    assert_eq!(
        snapshot.helper_dir,
        plugin_data_dir.join("helpers").join("aria2")
    );
    assert!(!snapshot.status.running);
    assert_eq!(
        snapshot.status.download_url,
        "https://example.test/aria2.zip"
    );
    assert_eq!(
        snapshot.status.bundled_archive_path.as_deref(),
        Some(
            plugin_data_dir
                .join("downloads")
                .join("aria2-runtime.zip")
                .to_string_lossy()
                .as_ref()
        )
    );
}

#[test]
fn cleanup_stale_state_removes_pid_and_status_for_dead_process() {
    let workspace = TestWorkspace::new("cleanup-stale");
    let plugin_data_dir = workspace.path("plugin-data");
    let paths = runtime_paths(&plugin_data_dir);
    fs::create_dir_all(&paths.helper_dir).expect("helper dir should be created");
    fs::write(&paths.pid_path, "999999").expect("pid file should be written");
    fs::write(
            &paths.status_path,
            r#"{"running":true,"pid":999999,"rpcUrl":"http://127.0.0.1:16831/jsonrpc","secret":"secret","downloadUrl":"https://example.test/aria2.zip"}"#,
        )
        .expect("status file should be written");

    cleanup_stale_state(
        &Aria2Config {
            host_runtime: None,
            plugin_data_dir: &plugin_data_dir,
            download_url: "https://example.test/aria2.zip",
        },
        &paths,
    )
    .expect("cleanup should remove stale process state");

    assert!(!paths.pid_path.exists());
    assert!(!paths.status_path.exists());
}

#[test]
fn managed_task_id_stays_stable_for_same_url_and_destination() {
    let destination_path = Path::new("C:/Temp/runtime.zip");
    let left = managed_task_id("https://example.test/runtime.zip", destination_path);
    let right = managed_task_id("https://example.test/runtime.zip", destination_path);

    assert_eq!(left, right);
}

#[test]
fn enqueue_download_reuses_existing_queued_task_record() {
    let destination_path = Path::new("C:/Temp/file-1.zip");
    let record = DownloadTaskRecord {
        task_id: managed_task_id("http://127.0.0.1/file-1.zip", destination_path),
        gid: "gid-existing".to_string(),
        url: "http://127.0.0.1/file-1.zip".to_string(),
        destination_path: destination_path.to_string_lossy().to_string(),
        metadata: Some(serde_json::json!({ "kind": "queued" })),
        status: "queued".to_string(),
        created_at: "2026-07-01T10:00:00Z".to_string(),
        finished_at: None,
        total_length: None,
        completed_length: None,
        error: None,
    };

    assert!(should_reuse_existing_task(
        &record,
        "http://127.0.0.1/file-1.zip",
        destination_path,
    ));
}

#[test]
fn enqueue_download_reuses_completed_task_only_when_file_exists() {
    let workspace = TestWorkspace::new("reuse-completed-task");
    let destination_path = workspace.path("downloads/runtime.zip");
    fs::create_dir_all(
        destination_path
            .parent()
            .expect("downloads dir should exist"),
    )
    .expect("downloads dir should be created");
    fs::write(&destination_path, b"runtime").expect("completed file should be written");
    let record = DownloadTaskRecord {
        task_id: managed_task_id("https://example.test/runtime.zip", &destination_path),
        gid: "gid-completed".to_string(),
        url: "https://example.test/runtime.zip".to_string(),
        destination_path: destination_path.to_string_lossy().to_string(),
        metadata: Some(serde_json::json!({ "kind": "runtime" })),
        status: "completed".to_string(),
        created_at: "2026-07-01T10:00:00Z".to_string(),
        finished_at: Some("2026-07-01T10:01:00Z".to_string()),
        total_length: Some(7),
        completed_length: Some(7),
        error: None,
    };

    assert!(should_reuse_existing_task(
        &record,
        "https://example.test/runtime.zip",
        &destination_path,
    ));

    fs::remove_file(&destination_path).expect("completed file should be removed");

    assert!(!should_reuse_existing_task(
        &record,
        "https://example.test/runtime.zip",
        &destination_path,
    ));
}

#[test]
fn enqueue_download_recreates_failed_task_record() {
    let destination_path = Path::new("C:/Temp/runtime.zip");
    let record = DownloadTaskRecord {
        task_id: managed_task_id("https://example.test/runtime.zip", destination_path),
        gid: "gid-failed".to_string(),
        url: "https://example.test/runtime.zip".to_string(),
        destination_path: destination_path.to_string_lossy().to_string(),
        metadata: Some(serde_json::json!({ "kind": "runtime" })),
        status: "failed".to_string(),
        created_at: "2026-07-01T10:00:00Z".to_string(),
        finished_at: Some("2026-07-01T10:00:30Z".to_string()),
        total_length: Some(100),
        completed_length: Some(30),
        error: Some("network failed".to_string()),
    };

    assert!(!should_reuse_existing_task(
        &record,
        "https://example.test/runtime.zip",
        destination_path,
    ));
}

#[test]
fn remove_download_deletes_persisted_task_record() {
    let workspace = TestWorkspace::new("remove-task-record");
    let plugin_data_dir = workspace.path("plugin-data");
    let paths = runtime_paths(&plugin_data_dir);
    fs::create_dir_all(&paths.tasks_dir).expect("tasks dir should be created");
    let rpc_url = start_mock_aria2_rpc_server(|payload| {
        let method = payload
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let result = if method == "aria2.getVersion" {
            serde_json::json!({ "version": "1.37.0" })
        } else {
            serde_json::json!("ok")
        };
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": "momobako",
            "result": result,
        })
    });
    write_mock_running_status(&paths, &plugin_data_dir, &rpc_url);
    let task_id = "task-remove";
    let task_path = task_path(&paths.tasks_dir, task_id);
    fs::write(
            &task_path,
            r#"{"taskId":"task-remove","gid":"gid-remove","url":"http://127.0.0.1/file.zip","destinationPath":"C:/Temp/file.zip","status":"queued","createdAt":"2026-07-01T10:00:00Z"}"#,
        )
        .expect("task file should be written");

    let config = Aria2Config {
        host_runtime: None,
        plugin_data_dir: &plugin_data_dir,
        download_url: "https://example.test/aria2.zip",
    };

    remove_download(&config, task_id).expect("remove download should succeed");

    assert!(!task_path.exists());
}

#[test]
fn await_download_marks_task_failed_when_timeout_is_reached() {
    let workspace = TestWorkspace::new("await-download-timeout");
    let plugin_data_dir = workspace.path("plugin-data");
    let paths = runtime_paths(&plugin_data_dir);
    fs::create_dir_all(&paths.tasks_dir).expect("tasks dir should be created");
    let task_id = "task-timeout";
    let record = DownloadTaskRecord {
        task_id: task_id.to_string(),
        gid: "gid-timeout".to_string(),
        url: "http://127.0.0.1/file.zip".to_string(),
        destination_path: "C:/Temp/file.zip".to_string(),
        metadata: Some(serde_json::json!({ "kind": "timeout-test" })),
        status: "active".to_string(),
        created_at: "2026-07-01T10:00:00Z".to_string(),
        finished_at: None,
        total_length: None,
        completed_length: None,
        error: None,
    };
    save_task(&paths.tasks_dir, &record).expect("task should be persisted");
    let rpc_url = start_mock_aria2_rpc_server(|payload| {
        let method = payload
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let result = if method == "aria2.getVersion" {
            serde_json::json!({ "version": "1.37.0" })
        } else {
            serde_json::json!({
                "gid": "gid-timeout",
                "status": "active",
                "totalLength": "100",
                "completedLength": "20"
            })
        };
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": "momobako",
            "result": result,
        })
    });
    write_mock_running_status(&paths, &plugin_data_dir, &rpc_url);

    let result = await_download(
        &Aria2Config {
            host_runtime: None,
            plugin_data_dir: &plugin_data_dir,
            download_url: "https://example.test/aria2.zip",
        },
        task_id,
        Duration::from_millis(0),
    )
    .expect("await download should return timeout record");

    assert_eq!(result.status, "failed");
    assert_eq!(
        result.error.as_deref(),
        Some("download task timed out: task-timeout")
    );
    assert!(result.finished_at.is_some());

    let persisted = load_task(&paths.tasks_dir, task_id).expect("timed out task should persist");
    assert_eq!(persisted.status, "failed");
    assert_eq!(
        persisted.error.as_deref(),
        Some("download task timed out: task-timeout")
    );
    assert!(persisted.finished_at.is_some());
}
