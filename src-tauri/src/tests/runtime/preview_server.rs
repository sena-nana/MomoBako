use crate::services::{
    repository::{
        install_local_filesystem_test_plugin_archive, FileReadRequest, RepositoryMutationRequest,
        RepositoryState,
    },
    runtime::preview_server::{parse_byte_range, preview_token_from_url, start_preview_server, ByteRange},
};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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
    fs::write(repo_root.join("model.glb"), b"glb-body")
        .expect("preview source should be written");
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
