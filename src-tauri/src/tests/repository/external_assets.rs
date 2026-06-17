//! External asset repository tests split out from the repository facade module.

use crate::services::repository::{
    ExternalAddAssetClient, ExternalAddAssetItem, ExternalAddAssetRequest,
};
use crate::services::repository::test_support::{
    create_local_repository_record_for_external_tests, create_test_state, serve_test_http_body,
};
use std::{collections::BTreeMap, fs};

#[test]
fn add_external_assets_imports_remote_url_and_metadata() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("external-add");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    let url = serve_test_http_body(b"external body");
    let response = state.add_external_assets(
        "request-external-add".to_string(),
        ExternalAddAssetRequest {
            repo_id: repo_id.clone(),
            parent_path: None,
            client: Some(ExternalAddAssetClient {
                id: Some("test-client".to_string()),
                name: None,
                version: Some("1".to_string()),
            }),
            items: vec![ExternalAddAssetItem {
                kind: "remoteUrl".to_string(),
                url: Some(url),
                filename: Some("captured.txt".to_string()),
                headers: None,
                metadata: Some(BTreeMap::from([(
                    "sourceUrl".to_string(),
                    serde_json::Value::String("https://example.test/source".to_string()),
                )])),
            }],
        },
    );

    assert_eq!(response.status, "success", "{:?}", response.failed);
    assert_eq!(response.imported.len(), 1);
    assert_eq!(response.imported[0].path, "captured.txt");
    assert!(repo_root.join("captured.txt").is_file());
    let detail = state
        .load_asset_detail(&repo_id, response.imported[0].asset_id.as_deref().unwrap())
        .expect("asset detail should load");
    assert!(detail.metadata.iter().any(|entry| {
        entry.key == "sourceUrl"
            && entry.value
                == serde_json::Value::String("https://example.test/source".to_string())
    }));

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn add_external_assets_reports_partial_and_duplicate_failures() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("external-partial");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    let url = serve_test_http_body(b"first");

    let response = state.add_external_assets(
        "request-external-partial".to_string(),
        ExternalAddAssetRequest {
            repo_id,
            parent_path: None,
            client: None,
            items: vec![
                ExternalAddAssetItem {
                    kind: "remoteUrl".to_string(),
                    url: Some(url),
                    filename: Some("same.txt".to_string()),
                    headers: None,
                    metadata: None,
                },
                ExternalAddAssetItem {
                    kind: "remoteUrl".to_string(),
                    url: Some("https://example.test/second.txt".to_string()),
                    filename: Some("same.txt".to_string()),
                    headers: None,
                    metadata: None,
                },
                ExternalAddAssetItem {
                    kind: "localPath".to_string(),
                    url: None,
                    filename: None,
                    headers: None,
                    metadata: None,
                },
            ],
        },
    );

    assert_eq!(response.status, "partial");
    assert_eq!(response.imported.len(), 1);
    assert_eq!(response.failed.len(), 2);
    assert!(response
        .failed
        .iter()
        .any(|failure| failure.code == "duplicateTarget"));
    assert!(response
        .failed
        .iter()
        .any(|failure| failure.code == "invalidInput"));
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn add_external_assets_rejects_invalid_target_path() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("external-target");
    let repo_id = create_local_repository_record_for_external_tests(&state, &repo_root);
    let response = state.add_external_assets(
        "request-external-target".to_string(),
        ExternalAddAssetRequest {
            repo_id,
            parent_path: Some("missing".to_string()),
            client: None,
            items: vec![ExternalAddAssetItem {
                kind: "remoteUrl".to_string(),
                url: Some("https://example.test/asset.txt".to_string()),
                filename: Some("asset.txt".to_string()),
                headers: None,
                metadata: None,
            }],
        },
    );

    assert_eq!(response.status, "failed");
    assert_eq!(response.failed[0].code, "invalidTargetPath");
    fs::remove_dir_all(root).expect("test temp root should be removed");
}
