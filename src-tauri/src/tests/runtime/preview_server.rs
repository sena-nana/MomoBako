use crate::services::{
    repository::{
        install_local_filesystem_test_plugin_archive, FileReadRequest, RepositoryMutationRequest,
        RepositoryState,
    },
    runtime::preview_server::{
        ByteRange, parse_byte_range, preview_token_from_url, start_preview_server,
    },
};
use crate::services::runtime::{
    RepositoryWatcher, build_external_connection_status, start_structure_refresh_worker,
};
use crate::viewmodels::RepositoryQueryViewModel;
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn plugin_data_dir(service_root: &std::path::Path, plugin_id: &str) -> PathBuf {
    service_root.join("plugin-data").join(plugin_id.replace('.', "-"))
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "momobako-runtime-{label}-{}-{unique}",
        std::process::id()
    ))
}

#[test]
fn preview_token_from_url_accepts_registered_route() {
    let token = "a".repeat(64);
    let url = format!("/preview/{token}?v=1");

    assert_eq!(preview_token_from_url(&url), Some(token.as_str()));
}

#[test]
fn preview_token_from_url_rejects_invalid_tokens() {
    assert_eq!(preview_token_from_url("/preview/not-hex"), None);
    assert_eq!(
        preview_token_from_url(&format!("/preview/{}", "g".repeat(64))),
        None
    );
    assert_eq!(
        preview_token_from_url(&format!("/other/{}", "0".repeat(64))),
        None
    );
}

#[test]
fn preview_parse_byte_range_accepts_standard_and_suffix_ranges() {
    assert_eq!(
        parse_byte_range("bytes=2-5", 10),
        Some(ByteRange { start: 2, end: 5 })
    );
    assert_eq!(
        parse_byte_range("bytes=7-", 10),
        Some(ByteRange { start: 7, end: 9 })
    );
    assert_eq!(
        parse_byte_range("bytes=-4", 10),
        Some(ByteRange { start: 6, end: 9 })
    );
    assert_eq!(
        parse_byte_range("bytes=8-99", 10),
        Some(ByteRange { start: 8, end: 9 })
    );
}

#[test]
fn preview_parse_byte_range_rejects_unsatisfiable_ranges() {
    assert_eq!(parse_byte_range("items=0-1", 10), None);
    assert_eq!(parse_byte_range("bytes=10-12", 10), None);
    assert_eq!(parse_byte_range("bytes=5-4", 10), None);
    assert_eq!(parse_byte_range("bytes=0-1,4-5", 10), None);
    assert_eq!(parse_byte_range("bytes=-0", 10), None);
}

#[test]
fn preview_server_serves_registered_source_file() {
    let root = unique_temp_dir("preview-server");
    let service_root = root.join("state");
    let repo_root = root.join("repo");
    fs::create_dir_all(&repo_root).expect("repo root should be created");
    fs::write(repo_root.join("model.glb"), b"glb-body").expect("preview source should be written");
    install_local_filesystem_test_plugin_archive(&service_root);

    let state = RepositoryState::from_root(service_root);
    let repo_id = state
        .create_repository(RepositoryMutationRequest {
            repo_id: Some("repo-preview-server".to_string()),
            name: "Preview Server".to_string(),
            path: repo_root.to_string_lossy().to_string(),
            backend_plugin_id: None,
            backend_config: None,
            skip_initial_sync: false,
        })
        .expect("repository should be created")
        .repository
        .repo_id;
    let response = state
        .prepare_preview_file_source(FileReadRequest {
            repo_id,
            path: "model.glb".to_string(),
        })
        .expect("preview source should be prepared");

    let addr = start_preview_server(Arc::new(state)).expect("preview server should start");
    let mut stream = TcpStream::connect(addr).expect("preview server should accept connections");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should be set");
    write!(
        stream,
        "GET /preview/{} HTTP/1.0\r\nHost: localhost\r\n\r\n",
        response.token
    )
    .expect("request should be written");

    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .expect("response should be readable");

    assert!(raw.starts_with("HTTP/1.0 200 OK"));
    assert!(raw.contains("Content-Type: model/gltf-binary"));
    assert!(raw.ends_with("glb-body"));
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn preview_server_serves_registered_source_file_range() {
    let root = unique_temp_dir("preview-server-range");
    let service_root = root.join("state");
    let repo_root = root.join("repo");
    fs::create_dir_all(&repo_root).expect("repo root should be created");
    fs::write(repo_root.join("clip.mp4"), b"media-body")
        .expect("preview source should be written");
    install_local_filesystem_test_plugin_archive(&service_root);

    let state = RepositoryState::from_root(service_root);
    let repo_id = state
        .create_repository(RepositoryMutationRequest {
            repo_id: Some("repo-preview-range".to_string()),
            name: "Preview Range".to_string(),
            path: repo_root.to_string_lossy().to_string(),
            backend_plugin_id: None,
            backend_config: None,
            skip_initial_sync: false,
        })
        .expect("repository should be created")
        .repository
        .repo_id;
    let response = state
        .prepare_preview_file_source(FileReadRequest {
            repo_id,
            path: "clip.mp4".to_string(),
        })
        .expect("preview source should be prepared");

    let addr = start_preview_server(Arc::new(state)).expect("preview server should start");
    let mut stream = TcpStream::connect(addr).expect("preview server should accept connections");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should be set");
    write!(
        stream,
        "GET /preview/{} HTTP/1.0\r\nHost: localhost\r\nRange: bytes=6-9\r\n\r\n",
        response.token
    )
    .expect("request should be written");

    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .expect("response should be readable");

    assert!(raw.starts_with("HTTP/1.0 206 Partial Content"));
    assert!(raw.contains("Content-Type: video/mp4"));
    assert!(raw.contains("Content-Range: bytes 6-9/10"));
    assert!(raw.ends_with("body"));
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn repository_query_prepare_preview_file_source_returns_preview_url() {
    let root = unique_temp_dir("preview-viewmodel-url");
    let service_root = root.join("state");
    let repo_root = root.join("repo");
    fs::create_dir_all(&repo_root).expect("repo root should be created");
    fs::write(repo_root.join("model.glb"), b"glb-body")
        .expect("preview source should be written");
    install_local_filesystem_test_plugin_archive(&service_root);

    let repository_state = Arc::new(RepositoryState::from_root(service_root.clone()));
    repository_state
        .ensure_initialized()
        .expect("repository state should initialize");
    let write_lock = Arc::new(Mutex::new(()));
    let structure_refresh_tx =
        start_structure_refresh_worker(repository_state.clone(), write_lock.clone())
            .expect("structure refresh worker should start");
    repository_state
        .set_structure_refresh_sender(structure_refresh_tx)
        .expect("structure refresh sender should register");
    let watcher_handle = RepositoryWatcher::start(repository_state.clone(), write_lock.clone())
        .expect("repository watcher should start");
    let preview_addr =
        start_preview_server(repository_state.clone()).expect("preview server should start");
    let runtime = crate::services::runtime::RepositoryRuntime {
        repository_state,
        watcher_handle,
        write_lock,
        preview_addr: preview_addr.clone(),
        external_connection: build_external_connection_status(
            &service_root,
            "127.0.0.1:0",
            "test-token",
            "0",
        ),
    };
    let view_model = RepositoryQueryViewModel::new(runtime.clone());

    let repo_id = runtime
        .repository_state
        .create_repository(RepositoryMutationRequest {
            repo_id: Some("repo-preview-viewmodel".to_string()),
            name: "Preview ViewModel".to_string(),
            path: repo_root.to_string_lossy().to_string(),
            backend_plugin_id: None,
            backend_config: None,
            skip_initial_sync: false,
        })
        .expect("repository should be created")
        .repository
        .repo_id;
    let response =
        tauri::async_runtime::block_on(view_model.prepare_preview_file_source(FileReadRequest {
            repo_id,
            path: "model.glb".to_string(),
        }))
        .expect("preview source should be prepared");

    assert_eq!(
        response.source_url.as_deref(),
        Some(format!("http://{preview_addr}/preview/{}", response.token).as_str())
    );
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn repository_runtime_start_cleans_stale_office_helper_state() {
    let root = unique_temp_dir("runtime-start-office-helper");
    let previous = std::env::current_dir().expect("cwd should resolve");
    std::env::set_current_dir(&root).expect("cwd should switch to temp root");

    let service_root = root.join(".service-data");
    install_local_filesystem_test_plugin_archive(&service_root);
    let plugin_dir = plugin_data_dir(&service_root, "momobako.service.office-convert");
    let helper_dir = plugin_dir.join("helpers").join("libreoffice");
    fs::create_dir_all(&helper_dir).expect("office helper dir should be created");
    fs::write(helper_dir.join("pid.txt"), "invalid").expect("pid state should be written");
    fs::write(helper_dir.join("status.json"), "{}").expect("status state should be written");
    fs::write(helper_dir.join("port.txt"), "23119").expect("port state should be written");
    fs::write(helper_dir.join("session.txt"), "office").expect("session state should be written");
    fs::write(helper_dir.join("office-convert-helper.ps1"), "Write-Host helper")
        .expect("helper script should be written");

    let runtime = crate::services::runtime::RepositoryRuntime::start()
        .expect("repository runtime should start");

    assert!(!helper_dir.join("pid.txt").exists());
    assert!(!helper_dir.join("status.json").exists());
    assert!(!helper_dir.join("port.txt").exists());
    assert!(!helper_dir.join("session.txt").exists());
    assert!(!helper_dir.join("office-convert-helper.ps1").exists());

    runtime.shutdown_helpers();
    std::env::set_current_dir(previous).expect("cwd should restore");
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn repository_runtime_start_cleans_stale_aria2_helper_state() {
    let root = unique_temp_dir("runtime-start-aria2-helper");
    let previous = std::env::current_dir().expect("cwd should resolve");
    std::env::set_current_dir(&root).expect("cwd should switch to temp root");

    let service_root = root.join(".service-data");
    install_local_filesystem_test_plugin_archive(&service_root);
    let plugin_dir = plugin_data_dir(&service_root, "momobako.service.downloader");
    let helper_dir = plugin_dir.join("helpers").join("aria2");
    fs::create_dir_all(&helper_dir).expect("aria2 helper dir should be created");
    fs::write(helper_dir.join("pid.txt"), "invalid").expect("pid state should be written");
    fs::write(helper_dir.join("status.json"), "{}").expect("status state should be written");
    fs::write(helper_dir.join("session.txt"), "aria2-session")
        .expect("session state should be written");

    let runtime = crate::services::runtime::RepositoryRuntime::start()
        .expect("repository runtime should start");

    assert!(!helper_dir.join("pid.txt").exists());
    assert!(!helper_dir.join("status.json").exists());
    assert!(!helper_dir.join("session.txt").exists());

    runtime.shutdown_helpers();
    std::env::set_current_dir(previous).expect("cwd should restore");
    fs::remove_dir_all(root).expect("test temp root should be removed");
}
