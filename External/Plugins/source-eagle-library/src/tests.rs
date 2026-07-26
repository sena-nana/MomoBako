//! Eagle Library source 插件回归测试。

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};

use super::{
    delete_entry, load_asset_metadata_map, load_json_or_default, load_snapshot,
    write_asset_metadata, write_repository_state, PluginPayload, ACTIONS_FILE_NAME,
    SAVED_FILTERS_FILE_NAME, TAGS_FILE_NAME,
};

/// 验证仓库内置 Eagle 夹具可以生成挂载快照。
#[test]
fn fixture_library_snapshot_is_mountable() {
    let snapshot = load_snapshot(&fixture_library("EagleLibraryRichSample.library")).unwrap();
    assert!(!snapshot.files.is_empty());
    assert!(snapshot.directories.contains_key(""));
    assert!(!snapshot.repository_state.quick_access.is_empty());
}

/// 验证富样例会产出 alias、回收站和仓库级对象。
#[test]
fn rich_sample_snapshot_contains_alias_and_repository_state() {
    let snapshot = load_snapshot(&fixture_library("EagleLibraryRichSample.library")).unwrap();
    let alias_count = snapshot
        .files
        .iter()
        .filter(|file| file.shared_asset_id.as_deref() == Some("MR3DC4GHBD9NT"))
        .count();
    let deleted_count = snapshot
        .files
        .iter()
        .filter(|file| file.status.as_deref() == Some("deleted"))
        .count();
    assert_eq!(snapshot.files.len(), 4);
    assert_eq!(alias_count, 2);
    assert_eq!(deleted_count, 1);
    assert_eq!(snapshot.repository_state.quick_access.len(), 3);
    assert_eq!(snapshot.repository_state.tag_groups.len(), 3);
    assert_eq!(snapshot.repository_state.smart_folders.len(), 2);
    assert_eq!(snapshot.repository_state.repository_actions.len(), 1);
    assert_eq!(snapshot.repository_state.trash_entries.len(), 1);
    assert!(snapshot
        .repository_state
        .directory_metadata_by_path
        .get("Design/Moodboard")
        .is_some_and(|item| item.protected));
}

/// 验证素材元数据回写会更新 Eagle sidecar。
#[test]
fn write_asset_metadata_updates_eagle_sidecar() {
    let repo_root = copy_fixture_library("EagleLibraryRichSample.library");
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "title".to_string(),
        Value::String("重命名后的论文".to_string()),
    );
    metadata.insert(
        "comment".to_string(),
        Value::String("来自测试的注释".to_string()),
    );
    metadata.insert(
        "link".to_string(),
        Value::String("https://example.com/updated".to_string()),
    );
    metadata.insert(
        "tagGroups".to_string(),
        json!([
            {
                "name": "Workflow",
                "tags": ["reviewed", "published"]
            }
        ]),
    );
    metadata.insert("rating".to_string(), json!(5));

    write_asset_metadata(
        &repo_root,
        "Design/论文 - 副本.doc",
        Some("MR3DC4GHBD9NT"),
        &metadata,
        None,
        Some("save"),
    )
    .unwrap();

    let asset = load_asset_metadata_map(&repo_root, "MR3DC4GHBD9NT").unwrap();
    assert_eq!(
        asset.get("name").and_then(Value::as_str),
        Some("重命名后的论文")
    );
    assert_eq!(
        asset.get("annotation").and_then(Value::as_str),
        Some("来自测试的注释")
    );
    assert_eq!(
        asset.get("url").and_then(Value::as_str),
        Some("https://example.com/updated")
    );
    assert_eq!(asset.get("rating").and_then(Value::as_i64), Some(5));
    assert_eq!(
        asset
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| tags.len()),
        Some(2)
    );
}

/// 验证仓库级对象回写会同步顶层 JSON。
#[test]
fn write_repository_state_updates_top_level_json_files() {
    let repo_root = copy_fixture_library("EagleLibraryRichSample.library");
    let payload: PluginPayload = serde_json::from_value(json!({
        "repoRoot": repo_root,
        "directoryMetadataByPath": {
            "Design/Moodboard": {
                "protected": false,
                "passwordTip": null
            }
        },
        "quickAccess": [
            {
                "shortcutId": "qa-folder",
                "label": "Archive",
                "targetKind": "folder",
                "targetPath": "Archive",
                "targetId": null
            }
        ],
        "tagGroups": [
            {
                "tagGroupId": "group-starred",
                "name": "Starred Tags",
                "tags": ["reviewed"]
            }
        ],
        "smartFolders": [
            {
                "smartFolderId": "smart-docs",
                "name": "Docs",
                "filter": {
                    "tags": ["doc"]
                },
                "sortOrder": 0
            }
        ],
        "repositoryActions": [
            {
                "raw": {
                    "id": "action-review",
                    "name": "Mark Reviewed"
                },
                "enabled": false
            }
        ]
    }))
    .unwrap();

    write_repository_state(&repo_root, &payload).unwrap();

    let metadata = load_json_or_default(&repo_root.join("metadata.json"), json!({})).unwrap();
    assert_eq!(
        metadata
            .get("quickAccess")
            .and_then(Value::as_array)
            .map(|items| items.len()),
        Some(1)
    );
    let folders = metadata
        .get("folders")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let moodboard = folders[0]
        .get("children")
        .and_then(Value::as_array)
        .and_then(|children| children.first())
        .and_then(Value::as_object)
        .cloned()
        .unwrap();
    assert!(moodboard.get("password").is_none());
    assert!(moodboard.get("passwordTip").is_none());
    assert!(moodboard.get("passwordTips").is_none());

    let tags_json = load_json_or_default(&repo_root.join(TAGS_FILE_NAME), json!({})).unwrap();
    assert_eq!(
        tags_json
            .get("starredTags")
            .and_then(Value::as_array)
            .map(|items| items.len()),
        Some(1)
    );
    let saved_filters =
        load_json_or_default(&repo_root.join(SAVED_FILTERS_FILE_NAME), json!([])).unwrap();
    assert_eq!(saved_filters.as_array().map(|items| items.len()), Some(1));
    let actions = load_json_or_default(&repo_root.join(ACTIONS_FILE_NAME), json!([])).unwrap();
    assert_eq!(
        actions[0].get("enabled").and_then(Value::as_bool),
        Some(false)
    );
}

/// 验证删除 alias 只移除一条 folder membership。
#[test]
fn delete_alias_entry_keeps_primary_asset_alive() {
    let repo_root = copy_fixture_library("EagleLibraryRichSample.library");
    delete_entry(&repo_root, "Archive/Docs/论文 - 副本.doc", false).unwrap();

    let snapshot = load_snapshot(&repo_root).unwrap();
    let alias_count = snapshot
        .files
        .iter()
        .filter(|file| file.shared_asset_id.as_deref() == Some("MR3DC4GHBD9NT"))
        .count();
    let asset = load_asset_metadata_map(&repo_root, "MR3DC4GHBD9NT").unwrap();
    assert_eq!(alias_count, 1);
    assert_eq!(
        asset
            .get("folders")
            .and_then(Value::as_array)
            .map(|items| items.len()),
        Some(1)
    );
    assert_eq!(asset.get("isDeleted").and_then(Value::as_bool), Some(false));
}

/// 返回仓库内置 Eagle 样例路径。
fn fixture_library(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(name)
}

/// 复制夹具到临时目录，避免测试污染源样例。
fn copy_fixture_library(name: &str) -> PathBuf {
    let source = fixture_library(name);
    let target = std::env::temp_dir().join(format!(
        "momobako-eagle-source-test-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    copy_directory_recursive(&source, &target);
    target
}

/// 递归复制 Eagle 样例目录。
fn copy_directory_recursive(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory_recursive(&source_path, &target_path);
        } else {
            fs::copy(&source_path, &target_path).unwrap();
        }
    }
}

/// 生成测试用唯一后缀，避免并发目录冲突。
fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}
