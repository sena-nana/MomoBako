//! Runtime 启动测试：启动时清理上次残留的 helper 进程状态。

use crate::services::repository::install_local_filesystem_test_plugin_archive;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn plugin_data_dir(service_root: &std::path::Path, plugin_id: &str) -> PathBuf {
    service_root
        .join("plugin-data")
        .join(plugin_id.replace('.', "-"))
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "momobako-runtime-{label}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temp root should be created");
    root
}

// helper 清理测试显式传服务目录，不能靠 `start()` 加改当前目录：cwd 是进程级的，并行测试会互相串目录。
#[test]
fn repository_runtime_start_cleans_stale_office_helper_state() {
    let root = unique_temp_dir("runtime-start-office-helper");
    let service_root = root.join(".service-data");
    install_local_filesystem_test_plugin_archive(&service_root);
    let plugin_dir = plugin_data_dir(&service_root, "momobako.service.office-convert");
    let helper_dir = plugin_dir.join("helpers").join("libreoffice");
    fs::create_dir_all(&helper_dir).expect("office helper dir should be created");
    fs::write(helper_dir.join("pid.txt"), "invalid").expect("pid state should be written");
    fs::write(helper_dir.join("status.json"), "{}").expect("status state should be written");
    fs::write(helper_dir.join("port.txt"), "23119").expect("port state should be written");
    fs::write(helper_dir.join("session.txt"), "office").expect("session state should be written");
    fs::write(
        helper_dir.join("office-convert-helper.ps1"),
        "Write-Host helper",
    )
    .expect("helper script should be written");

    let runtime = crate::services::runtime::RepositoryRuntime::start_at(service_root.clone())
        .expect("repository runtime should start");

    assert!(!helper_dir.join("pid.txt").exists());
    assert!(!helper_dir.join("status.json").exists());
    assert!(!helper_dir.join("port.txt").exists());
    assert!(!helper_dir.join("session.txt").exists());
    assert!(!helper_dir.join("office-convert-helper.ps1").exists());

    runtime.shutdown_helpers();
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn repository_runtime_start_cleans_stale_aria2_helper_state() {
    let root = unique_temp_dir("runtime-start-aria2-helper");
    let service_root = root.join(".service-data");
    install_local_filesystem_test_plugin_archive(&service_root);
    let plugin_dir = plugin_data_dir(&service_root, "momobako.service.downloader");
    let helper_dir = plugin_dir.join("helpers").join("aria2");
    fs::create_dir_all(&helper_dir).expect("aria2 helper dir should be created");
    fs::write(helper_dir.join("pid.txt"), "invalid").expect("pid state should be written");
    fs::write(helper_dir.join("status.json"), "{}").expect("status state should be written");
    fs::write(helper_dir.join("session.txt"), "aria2-session")
        .expect("session state should be written");

    let runtime = crate::services::runtime::RepositoryRuntime::start_at(service_root.clone())
        .expect("repository runtime should start");

    assert!(!helper_dir.join("pid.txt").exists());
    assert!(!helper_dir.join("status.json").exists());
    assert!(!helper_dir.join("session.txt").exists());

    runtime.shutdown_helpers();
    fs::remove_dir_all(root).expect("test temp root should be removed");
}
