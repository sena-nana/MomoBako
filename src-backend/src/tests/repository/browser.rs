//! Browser-domain repository tests are being migrated out of `services::repository::mod`.
//!
//! 导入外部文件时目标文件夹里已有同名条目：默认整批拒绝、一个都不复制；`skipExisting` 时跳过同名的、导入其余的，
//! 已有的文件不覆盖，跳过的按目标资源库里的相对路径写进结果的 `skippedEntries`。

use std::fs;
use std::path::Path;

use serde_json::json;

use crate::services::repository::test_support::{
    create_local_repository_record_for_external_tests, create_test_state,
};
use crate::services::repository::FileImportRequest;

/// 在资源库外面的 `folder` 里写好要导入的文件，返回它们的绝对路径。
fn incoming(root: &Path, folder: &str, files: &[(&str, &[u8])]) -> Vec<String> {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).expect("incoming folder should be created");
    files
        .iter()
        .map(|(name, bytes)| {
            let path = dir.join(name);
            fs::write(&path, bytes).expect("incoming file should be written");
            path.to_string_lossy().to_string()
        })
        .collect()
}

fn request(
    repo_id: &str,
    parent_path: Option<&str>,
    source_paths: Vec<String>,
    skip_existing: bool,
) -> FileImportRequest {
    FileImportRequest {
        repo_id: repo_id.to_string(),
        parent_path: parent_path.map(str::to_string),
        source_paths,
        skip_existing,
    }
}

#[test]
fn import_entries_reject_the_whole_batch_when_a_name_exists() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("import-existing-reject");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    fs::write(repo_root.join("song.mp3"), b"old").expect("existing file should be written");
    let sources = incoming(
        &root,
        "incoming",
        &[("song.mp3", b"new"), ("other.mp3", b"other")],
    );

    let error = state
        .import_entries(request(&repo_id, None, sources, false))
        .expect_err("an existing name rejects the whole batch");

    assert!(error.contains("entry already exists: song.mp3"), "{error}");
    assert_eq!(
        fs::read(repo_root.join("song.mp3")).expect("existing file stays"),
        b"old"
    );
    assert!(
        !repo_root.join("other.mp3").exists(),
        "nothing in the batch is imported"
    );
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn import_entries_skip_existing_names_and_import_the_rest() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("import-existing-skip");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    fs::write(repo_root.join("song.mp3"), b"old").expect("existing file should be written");
    let sources = incoming(
        &root,
        "incoming",
        &[("song.mp3", b"new"), ("other.mp3", b"other")],
    );

    let response = state
        .import_entries(request(&repo_id, None, sources, true))
        .expect("skipping existing names should import the rest");

    assert_eq!(response.skipped_entries, ["song.mp3"]);
    assert_eq!(
        fs::read(repo_root.join("song.mp3")).expect("existing file stays"),
        b"old",
        "existing files are never overwritten"
    );
    assert_eq!(
        fs::read(repo_root.join("other.mp3")).expect("new file is imported"),
        b"other"
    );
    assert!(response
        .snapshot
        .entries
        .iter()
        .any(|entry| entry.path == "other.mp3"));
    assert!(response
        .snapshot
        .entries
        .iter()
        .any(|entry| entry.path == "song.mp3"));

    let encoded = serde_json::to_value(&response).expect("response should encode");
    assert_eq!(
        encoded["repoId"],
        json!(repo_id),
        "snapshot fields stay at the top level"
    );
    assert!(encoded["entries"].is_array());
    assert_eq!(encoded["skippedEntries"], json!(["song.mp3"]));
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn import_entries_report_skipped_paths_inside_the_target_folder() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("import-existing-folder");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    fs::create_dir_all(repo_root.join("夜曲")).expect("target folder should be created");
    fs::write(repo_root.join("夜曲").join("晴天.mp3"), b"old")
        .expect("existing file should be written");
    let mut sources = incoming(
        &root,
        "first",
        &[("晴天.mp3", b"new"), ("七里香.mp3", b"seven")],
    );
    sources.extend(incoming(&root, "second", &[("七里香.mp3", b"again")]));

    let response = state
        .import_entries(request(&repo_id, Some("夜曲"), sources, true))
        .expect("skipping existing names should import the rest");

    assert_eq!(
        response.skipped_entries,
        ["夜曲/晴天.mp3", "夜曲/七里香.mp3"],
        "a name repeated within the batch is skipped too"
    );
    assert_eq!(
        fs::read(repo_root.join("夜曲").join("晴天.mp3")).expect("existing file stays"),
        b"old"
    );
    assert_eq!(
        fs::read(repo_root.join("夜曲").join("七里香.mp3")).expect("first copy wins"),
        b"seven"
    );
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn import_entries_succeed_when_every_name_is_skipped() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("import-existing-all");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    fs::write(repo_root.join("song.mp3"), b"old").expect("existing file should be written");
    let sources = incoming(&root, "incoming", &[("song.mp3", b"new")]);

    let response = state
        .import_entries(request(&repo_id, None, sources, true))
        .expect("an all-skipped batch still succeeds");

    assert_eq!(response.skipped_entries, ["song.mp3"]);
    assert_eq!(
        fs::read(repo_root.join("song.mp3")).expect("existing file stays"),
        b"old"
    );
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn import_requests_default_to_rejecting_and_responses_omit_empty_skips() {
    let decoded: FileImportRequest =
        serde_json::from_value(json!({ "repoId": "repo", "parentPath": null, "sourcePaths": [] }))
            .expect("old requests without skipExisting still decode");
    assert!(!decoded.skip_existing);

    let (state, root, repo_root, _thumbnail_root) = create_test_state("import-no-skips");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    let sources = incoming(&root, "incoming", &[("fresh.mp3", b"fresh")]);
    let response = state
        .import_entries(request(&repo_id, None, sources, false))
        .expect("a batch without conflicts imports");
    let encoded = serde_json::to_value(&response).expect("response should encode");
    assert!(
        encoded.get("skippedEntries").is_none(),
        "nothing skipped, the snapshot shape is unchanged"
    );
    assert!(repo_root.join("fresh.mp3").is_file());
    fs::remove_dir_all(root).expect("test temp root should be removed");
}
