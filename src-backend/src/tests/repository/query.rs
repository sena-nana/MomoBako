//! Query-domain repository tests are being migrated out of `services::repository::mod`.
//!
//! 资源库列表：某个资源库的后端插件没注册时照样列出它，但不打开、不重建它的存储，也不拖垮整张列表。

use std::fs;

use crate::services::repository::test_support::{
    create_local_repository_record_for_external_tests, create_test_state,
    register_repository_without_backend_plugin,
};

#[test]
fn list_repositories_keeps_listing_when_a_backend_plugin_is_unregistered() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("list-unregistered-backend");
    let ready_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    let orphan_root = root.join("orphan");
    fs::create_dir_all(&orphan_root).expect("orphan repository root should be created");
    let orphan_id = "repo-orphaned-backend";
    register_repository_without_backend_plugin(
        &state,
        orphan_id,
        &orphan_root,
        "user.removed-source",
    );

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
