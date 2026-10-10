//! 外部素材 API 运行时测试：健康检查、令牌校验和可写资源库列表。

use crate::services::{
    repository::test_support::{
        create_local_repository_record_for_external_tests, create_test_state,
    },
    runtime::external_api::start_external_api_server,
};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
    time::Duration,
};

#[test]
fn external_api_serves_health_and_requires_token() {
    // 和生产一样先注册本地文件系统插件、写好 `.momo`，列表才认得这个可写资源库。
    let (state, root, repo_root, _thumbnail_root) = create_test_state("external-api");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);

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

    let mut unauthorized =
        TcpStream::connect(&addr).expect("external API should accept connections");
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
