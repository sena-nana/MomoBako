use crate::services::{
    repository::RepositoryState,
    runtime::external_api::start_external_api_server,
};
use rusqlite::{params, Connection};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const LOCAL_FILESYSTEM_PLUGIN_ID: &str = "momobako.local-filesystem";

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
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
fn external_api_serves_health_and_requires_token() {
    let root = unique_temp_dir("external-api");
    let service_root = root.join("state");
    let repo_root = root.join("repo");
    fs::create_dir_all(&repo_root).expect("repo root should be created");
    let state = RepositoryState::from_root(service_root.clone());
    state
        .ensure_initialized()
        .expect("repository state should initialize");
    let repo_id = "repo-external-api".to_string();
    let registry =
        Connection::open(service_root.join("repositories.db")).expect("registry should open");
    registry
        .execute(
            r#"
            INSERT INTO repositories (
              repo_id, name, path, backend_plugin_id, backend_config_json, status, created_at, updated_at
            )
            VALUES (?1, 'External API Repo', ?2, ?3, '{}', 'ready', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')
            "#,
            params![&repo_id, repo_root.to_string_lossy(), LOCAL_FILESYSTEM_PLUGIN_ID],
        )
        .expect("repository should be registered");
    drop(registry);

    let token = "token-test".to_string();
    let addr = start_external_api_server(Arc::new(state), Arc::new(Mutex::new(())), token.clone())
        .expect("external API should start");

    let mut health = TcpStream::connect(&addr).expect("external API should accept connections");
    health
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should be set");
    write!(
        health,
        "GET /external/v1/health HTTP/1.0\r\nHost: localhost\r\n\r\n"
    )
    .expect("request should be written");
    let mut health_raw = String::new();
    health
        .read_to_string(&mut health_raw)
        .expect("health response should be readable");
    assert!(health_raw.starts_with("HTTP/1.0 200 OK"));
    assert!(health_raw.contains("assets.add.remoteUrl"));

    let mut unauthorized = TcpStream::connect(&addr).expect("external API should accept connections");
    unauthorized
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should be set");
    write!(
        unauthorized,
        "GET /external/v1/repositories HTTP/1.0\r\nHost: localhost\r\n\r\n"
    )
    .expect("request should be written");
    let mut unauthorized_raw = String::new();
    unauthorized
        .read_to_string(&mut unauthorized_raw)
        .expect("unauthorized response should be readable");
    assert!(unauthorized_raw.starts_with("HTTP/1.0 401 Unauthorized"));
    drop(unauthorized);

    let mut authorized = TcpStream::connect(&addr).expect("external API should accept connections");
    authorized
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should be set");
    write!(
        authorized,
        "GET /external/v1/repositories HTTP/1.0\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\r\n"
    )
    .expect("request should be written");
    let mut authorized_raw = String::new();
    authorized
        .read_to_string(&mut authorized_raw)
        .expect("authorized response should be readable");
    assert!(authorized_raw.starts_with("HTTP/1.0 200 OK"));
    assert!(authorized_raw.contains(&repo_id));
    drop(authorized);
    drop(health);
    fs::remove_dir_all(root).expect("test temp root should be removed");
}
