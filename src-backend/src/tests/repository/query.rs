//! Query-domain repository tests are being migrated out of `services::repository::mod`.
//!
//! 资源库列表的降级：后端插件没注册、或者 `.momo` 要从后端重建却建不成时照样列出资源库，资源数记 0，
//! 不拖垮整张列表；磁盘上也不能留下「已经重建过」的标记，插件恢复后要能自己重建回来。

use std::fs;
use std::path::Path;

use crate::services::repository::test_support::{
    create_local_repository_record_for_external_tests, create_test_state,
    install_unreachable_filesystem_source_plugin, register_repository_record_only,
};
use crate::services::repository::{
    install_local_filesystem_test_plugin_archive, PluginEnabledRequest, SyncRequest,
};

const LOCAL_FILESYSTEM_PLUGIN_ID: &str = "momobako.local-filesystem";

/// `.momo` 里标记「已经重建过」的元数据文件和数据库都不在。
fn rebuild_markers_absent(repo_root: &Path) -> bool {
    let meta_dir = repo_root.join(".momo");
    !meta_dir.join("repository.json").exists() && !meta_dir.join("metadata.db").exists()
}

#[test]
fn list_repositories_keeps_listing_when_a_backend_plugin_is_unregistered() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("list-unregistered-backend");
    let ready_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    let orphan_root = root.join("orphan");
    fs::create_dir_all(&orphan_root).expect("orphan repository root should be created");
    let orphan_id = "repo-orphaned-backend";
    register_repository_record_only(&state, orphan_id, &orphan_root, "user.removed-source");

    let repositories = state
        .list_repositories()
        .expect("an unregistered backend should not fail the whole listing");

    assert!(repositories
        .iter()
        .any(|repository| repository.repo_id == ready_id));
    let orphan = repositories
        .iter()
        .find(|repository| repository.repo_id == orphan_id)
        .expect("the orphaned repository should still be listed");
    assert_eq!(orphan.backend.kind, "unavailable");
    assert_eq!(orphan.asset_count, 0);
    // 插件不在就不知道存储该放哪，列表不能替它在服务目录里建一份空库。
    assert!(!state
        .root_path()
        .join("repositories")
        .join(orphan_id)
        .exists());

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn list_repositories_waits_for_an_unavailable_backend_before_rebuilding_storage() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("list-disabled-backend");
    fs::write(repo_root.join("track.mp3"), b"demo").expect("test file should be written");
    install_local_filesystem_test_plugin_archive(&state.root_path());
    state
        .ensure_initialized()
        .expect("repository state should initialize");
    // 停用和「运行时不可用」一样接不了调用；资源库登记前才允许停用它的后端插件。
    state
        .set_plugin_enabled(PluginEnabledRequest {
            plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
            enabled: false,
        })
        .expect("backend plugin should be disabled");
    let repo_id = "repo-disabled-backend";
    register_repository_record_only(&state, repo_id, &repo_root, LOCAL_FILESYSTEM_PLUGIN_ID);

    let repositories = state
        .list_repositories()
        .expect("an unavailable backend should not fail the whole listing");
    let summary = repositories
        .iter()
        .find(|repository| repository.repo_id == repo_id)
        .expect("the repository should still be listed");
    assert_eq!(summary.asset_count, 0);
    assert!(rebuild_markers_absent(&repo_root));
    let error = state
        .sync_repository(SyncRequest {
            repo_id: repo_id.to_string(),
        })
        .expect_err("opening the repository should report the unavailable backend");
    assert!(error.contains("backend plugin is unavailable"), "{error}");
    assert!(rebuild_markers_absent(&repo_root));

    state
        .set_plugin_enabled(PluginEnabledRequest {
            plugin_id: LOCAL_FILESYSTEM_PLUGIN_ID.to_string(),
            enabled: true,
        })
        .expect("backend plugin should be enabled again");
    let repositories = state
        .list_repositories()
        .expect("the listing should rebuild storage once the backend is back");
    let summary = repositories
        .iter()
        .find(|repository| repository.repo_id == repo_id)
        .expect("the repository should be listed");
    assert_eq!(summary.asset_count, 1);
    assert!(!rebuild_markers_absent(&repo_root));

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn failed_rebuild_leaves_no_rebuilt_markers_so_the_next_open_retries() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("list-failed-rebuild");
    fs::write(repo_root.join("track.mp3"), b"demo").expect("test file should be written");
    let plugin_id = "test.unreachable-filesystem";
    let repo_id = "repo-unreachable-backend";
    register_repository_record_only(&state, repo_id, &repo_root, plugin_id);
    install_unreachable_filesystem_source_plugin(&state, plugin_id);

    // 插件目录里显示就绪，调用却失败：重建同步失败后既不写元数据也不留新建的库，每次打开都会再试。
    for _ in 0..2 {
        let repositories = state
            .list_repositories()
            .expect("a failed rebuild should not fail the whole listing");
        let summary = repositories
            .iter()
            .find(|repository| repository.repo_id == repo_id)
            .expect("the repository should still be listed");
        assert_eq!(summary.asset_count, 0);
        assert!(rebuild_markers_absent(&repo_root));
    }

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn listing_without_asset_counts_does_not_open_repository_storage() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("list-without-counts");
    install_local_filesystem_test_plugin_archive(&state.root_path());
    let repo_id = "repo-without-counts";
    register_repository_record_only(&state, repo_id, &repo_root, LOCAL_FILESYSTEM_PLUGIN_ID);

    let repositories = state
        .list_repositories_without_asset_counts()
        .expect("registered repositories should list");

    assert!(repositories
        .iter()
        .any(|repository| repository.repo_id == repo_id && repository.status == "ready"));
    // 文件监听每批事件都要列一次资源库，这里不能去打开或重建 `.momo`。
    assert!(!repo_root.join(".momo").exists());

    fs::remove_dir_all(root).expect("test temp root should be removed");
}
