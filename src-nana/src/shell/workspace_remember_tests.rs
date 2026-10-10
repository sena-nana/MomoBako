//! 上次打开的资源库，对照 Vue `readLastActiveRepositoryId` / `rememberLastActiveRepository`：从冷启动的读列表
//! 和切换列表这些真实入口走，送回服务结果，看选中了哪个仓库、加载日志写了什么、排没排写文件，以及写下去的文件
//! 下一次冷启动读不读得回来。

use std::fs;
use std::path::PathBuf;

use crate::shell::data_load_tests::{browser, snapshot, summary};
use crate::shell::{ShellMessage, ShellViewModel, SidebarMessage, StartupStatus, SyncCounts, WorkspaceEffect};

use super::LAST_ACTIVE_REPOSITORY_KEY;

/// 每个测试自己的临时目录。
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("momobako-last-repo-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("临时目录");
    dir
}

/// 写一份记着 `repo_id` 的文件，和产品写出来的格式一样。
fn remembered_file(dir: &std::path::Path, repo_id: &str) -> PathBuf {
    let file = dir.join("last-repository.json");
    fs::write(&file, serde_json::json!({ LAST_ACTIVE_REPOSITORY_KEY: repo_id }).to_string()).expect("写记住的仓库");
    file
}

/// 冷启动：先读记住的仓库文件，再照产品启动读资源库列表。
fn cold_start(file: &std::path::Path, repositories: &[(&str, &str)]) -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.load_last_repository_file(file);
    let generation = model.workspace.prepare_initial_list();
    let items = repositories.iter().map(|(repo_id, status)| summary(repo_id, status)).collect();
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(items) });
    model
}

/// 把启动或换仓库剩下的三步答完：同步、摘要、首屏目录。
fn finish_loading(model: &mut ShellViewModel, repo_id: &str) {
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySynced { generation, result: Ok(SyncCounts::default()) });
    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation, result: Ok(snapshot(repo_id)) });
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(repo_id))));
}

fn synced(effects: &[WorkspaceEffect]) -> Option<&str> {
    effects.iter().find_map(|effect| match effect {
        WorkspaceEffect::SyncRepository { repo_id, .. } => Some(repo_id.as_str()),
        _ => None,
    })
}

fn persists(effects: &[WorkspaceEffect]) -> bool {
    effects.contains(&WorkspaceEffect::PersistLastRepository)
}

/// 某条加载日志的明细。
fn log_detail(model: &ShellViewModel, message: &str) -> String {
    let log = model.workspace.startup.logs.iter().find(|log| log.message == message);
    log.and_then(|log| log.detail.clone()).unwrap_or_default()
}

/// 记着 repo-b，列表第一项是 repo-a：照 Vue 打开 repo-b，加载日志的「流程开始」和「已选定资源库」都写出上次打开的
/// 仓库。还是同一个仓库，读完摘要不重写文件。
#[test]
fn a_cold_start_opens_the_remembered_repository() {
    let dir = temp_dir("cold");
    let file = remembered_file(&dir, "repo-b");
    let mut model = cold_start(&file, &[("repo-a", "ready"), ("repo-b", "ready")]);
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some("repo-b"));
    assert_eq!(synced(&model.workspace.take_effects()), Some("repo-b"), "同步记着的仓库");
    assert_eq!(log_detail(&model, "首屏启动流程开始。"), "totalSteps: 4 · rememberedRepoId: repo-b");
    assert!(
        log_detail(&model, "首屏启动已选定资源库。").starts_with("repositoryCount: 2 · rememberedRepoId: repo-b · repositoryStatus: ready · repositoryName: 仓库 repo-b"),
        "{}",
        log_detail(&model, "首屏启动已选定资源库。")
    );

    finish_loading(&mut model, "repo-b");
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    assert_eq!(model.repository_id.as_deref(), Some("repo-b"));
    assert!(!persists(&model.workspace.take_effects()), "记着的就是它，不重写");
    let _ = fs::remove_dir_all(dir);
}

/// 记着的仓库已经不在列表里：退回第一项，读完它的摘要以后记住它，写下去的文件下一次冷启动读得回来。
#[test]
fn a_remembered_repository_that_is_gone_falls_back_to_the_first() {
    let dir = temp_dir("gone");
    let file = remembered_file(&dir, "gone");
    let mut model = cold_start(&file, &[("repo-a", "ready"), ("repo-b", "ready")]);
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some("repo-a"));
    assert_eq!(synced(&model.workspace.take_effects()), Some("repo-a"));
    assert!(log_detail(&model, "首屏启动已选定资源库。").contains("rememberedRepoId: gone"), "日志照实写记着的 id");

    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySynced { generation, result: Ok(SyncCounts::default()) });
    assert!(!persists(&model.workspace.take_effects()), "同步完还不记，Vue 读完摘要才记");
    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation, result: Ok(snapshot("repo-a")) });
    assert!(persists(&model.workspace.take_effects()), "读完摘要记住 repo-a");
    model.workspace.save_last_repository_file(&file);

    let reopened = cold_start(&file, &[("repo-a", "ready"), ("repo-b", "ready")]);
    assert_eq!(reopened.workspace.last_active_repo_id.as_deref(), Some("repo-a"));
    assert_eq!(reopened.workspace.active_repo_id.as_deref(), Some("repo-a"));
    let _ = fs::remove_dir_all(dir);
}

/// 从切换列表换到 beta：同步完还不记，读完 beta 的摘要才记住并写文件；下一次冷启动直接打开 beta。
#[test]
fn switching_writes_the_new_repository_after_its_summary() {
    let dir = temp_dir("switch");
    let file = remembered_file(&dir, "alpha");
    let mut model = cold_start(&file, &[("alpha", "ready"), ("beta", "ready")]);
    finish_loading(&mut model, "alpha");
    model.workspace.take_effects();

    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenRepositorySwitcher));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectRepositoryFromSwitcher("beta".into())));
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some("beta"));
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySynced { generation, result: Ok(SyncCounts::default()) });
    assert!(!persists(&model.workspace.take_effects()), "同步完还不记");
    assert_eq!(model.workspace.last_active_repo_id.as_deref(), Some("alpha"));
    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation, result: Ok(snapshot("beta")) });
    assert!(persists(&model.workspace.take_effects()), "读完 beta 的摘要写文件");
    model.workspace.save_last_repository_file(&file);
    let raw = fs::read_to_string(&file).expect("记住的仓库");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&raw).expect("JSON")[LAST_ACTIVE_REPOSITORY_KEY], "beta");

    let reopened = cold_start(&file, &[("alpha", "ready"), ("beta", "ready")]);
    assert_eq!(reopened.workspace.active_repo_id.as_deref(), Some("beta"), "下次冷启动打开 beta");
    let _ = fs::remove_dir_all(dir);
}

/// 换到缺失的仓库：不同步，立即记住它（Vue `selectRepository` 的缺失分支）。
#[test]
fn switching_to_a_missing_repository_remembers_it_at_once() {
    let dir = temp_dir("missing");
    let file = remembered_file(&dir, "alpha");
    let mut model = cold_start(&file, &[("alpha", "ready"), ("gone", "missing")]);
    finish_loading(&mut model, "alpha");
    model.workspace.take_effects();

    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenRepositorySwitcher));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectRepositoryFromSwitcher("gone".into())));
    let effects = model.workspace.take_effects();
    assert_eq!(synced(&effects), None, "缺失的仓库不同步");
    assert!(persists(&effects));
    assert_eq!(model.workspace.last_active_repo_id.as_deref(), Some("gone"));
    let _ = fs::remove_dir_all(dir);
}

/// 列表空了：照 Vue `resetWorkspaceSelection({ clearRememberedRepository: true })` 清掉，写下的文件不带这个键。
#[test]
fn an_empty_list_forgets_the_remembered_repository() {
    let dir = temp_dir("empty");
    let file = remembered_file(&dir, "repo-a");
    let mut model = cold_start(&file, &[]);
    assert!(persists(&model.workspace.take_effects()));
    assert_eq!(model.workspace.last_active_repo_id, None);
    model.workspace.save_last_repository_file(&file);
    let raw = fs::read_to_string(&file).expect("记住的仓库");
    assert_eq!(raw, "{}");
    let reopened = cold_start(&file, &[("repo-a", "ready")]);
    assert_eq!(reopened.workspace.last_active_repo_id, None);
    let _ = fs::remove_dir_all(dir);
}

/// 文件损坏、值不是字符串或是空白：当作没有记住，冷启动照常打开第一项。
#[test]
fn an_unreadable_file_means_nothing_is_remembered() {
    let dir = temp_dir("corrupt");
    let file = dir.join("last-repository.json");
    for raw in ["{not json", r#"{"momobako.lastActiveRepositoryId": 7}"#, r#"{"momobako.lastActiveRepositoryId": "  "}"#, "[]"] {
        fs::write(&file, raw).expect("写坏文件");
        let model = cold_start(&file, &[("repo-a", "ready"), ("repo-b", "ready")]);
        assert_eq!(model.workspace.last_active_repo_id, None, "{raw}");
        assert_eq!(model.workspace.active_repo_id.as_deref(), Some("repo-a"), "{raw}");
    }
    let missing = cold_start(&dir.join("absent.json"), &[("repo-b", "ready")]);
    assert_eq!(missing.workspace.active_repo_id.as_deref(), Some("repo-b"), "没有文件时打开第一项");
    let _ = fs::remove_dir_all(dir);
}

/// 写不进去（父路径是个文件）：只记日志，状态不变，接着走启动。
#[test]
fn a_failed_write_does_not_disturb_startup() {
    let dir = temp_dir("readonly");
    let blocker = dir.join("blocker");
    fs::write(&blocker, "不是目录").expect("占位文件");
    let file = blocker.join("last-repository.json");
    let mut model = cold_start(&file, &[("repo-a", "ready")]);
    finish_loading(&mut model, "repo-a");
    assert!(persists(&model.workspace.take_effects()));
    model.workspace.save_last_repository_file(&file);
    assert!(!file.exists());
    assert_eq!(model.workspace.last_active_repo_id.as_deref(), Some("repo-a"));
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    let _ = fs::remove_dir_all(dir);
}
