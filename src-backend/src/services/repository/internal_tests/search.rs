//! 资源搜索、智能文件夹筛选、仓库动作与插件分类推断的测试。

use super::*;

#[test]
fn search_assets_filters_current_repository_metadata_and_formats() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("search-filters");
    fs::write(repo_root.join("cover.psd"), b"cover").expect("cover file should be written");
    fs::write(repo_root.join("alt.psd"), b"alternate").expect("alt file should be written");
    fs::write(repo_root.join("icon.png"), b"icon").expect("icon file should be written");
    fs::write(repo_root.join("deleted.psd"), b"deleted").expect("deleted file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);

    let repo = state
        .load_repository_record(&repo_id)
        .expect("repository record should load");
    let connection = state
        .open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )
        .expect("repository connection should open");
    let now = now_rfc3339();
    let cover_asset_id: String = connection
        .query_row(
            "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'cover.psd'",
            [repo_id.as_str()],
            |row| row.get(0),
        )
        .expect("cover asset id should load");
    let alt_asset_id: String = connection
        .query_row(
            "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'alt.psd'",
            [repo_id.as_str()],
            |row| row.get(0),
        )
        .expect("alt asset id should load");
    let deleted_asset_id: String = connection
        .query_row(
            "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = 'deleted.psd'",
            [repo_id.as_str()],
            |row| row.get(0),
        )
        .expect("deleted asset id should load");

    for (asset_id, key, value) in [
        (&cover_asset_id, "color", serde_json::json!("红色")),
        (&cover_asset_id, "shape", serde_json::json!("方形")),
        (&cover_asset_id, "rating", serde_json::json!(5)),
        (&alt_asset_id, "color", serde_json::json!("蓝色")),
        (&alt_asset_id, "shape", serde_json::json!("圆形")),
        (&alt_asset_id, "rating", serde_json::json!(2)),
        (&deleted_asset_id, "color", serde_json::json!("红色")),
        (&deleted_asset_id, "shape", serde_json::json!("方形")),
        (&deleted_asset_id, "rating", serde_json::json!(5)),
    ] {
        connection
            .execute(
                r#"
                INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                VALUES (?1, ?2, ?3, ?4, 1, ?5)
                "#,
                params![asset_id, key, infer_value_type(&value), value.to_string(), now],
            )
            .expect("metadata should be written");
    }
    for (asset_id, tag) in [(&cover_asset_id, "封面"), (&alt_asset_id, "草稿")] {
        connection
            .execute(
                r#"
                INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag)
                VALUES (?1, ?2, ?3)
                "#,
                params![asset_id, tag, tag.to_lowercase()],
            )
            .expect("tag should be written");
    }
    connection
        .execute(
            "UPDATE assets SET status = 'deleted' WHERE repo_id = ?1 AND path = 'deleted.psd'",
            [repo_id.as_str()],
        )
        .expect("deleted asset should be marked");
    drop(connection);

    let response = state
        .search_assets(SearchRequest {
            query: String::new(),
            repo_id: Some(repo_id.clone()),
            exclude_query: None,
            metadata_key: None,
            metadata_value: None,
            tag: None,
            tags: Some(vec!["封面".to_string()]),
            metadata_filters: Some(vec![
                SearchMetadataFilter {
                    key: "color".to_string(),
                    value: "红色".to_string(),
                },
                SearchMetadataFilter {
                    key: "shape".to_string(),
                    value: "方形".to_string(),
                },
            ]),
            formats: Some(vec!["psd".to_string(), "jpg".to_string()]),
            min_rating: Some(4.0),
            exclude_tags: None,
            exclude_formats: None,
            exclude_metadata_filters: None,
            exclude_path_prefixes: None,
            exclude_number_filters: None,
            exclude_date_filters: None,
            number_filters: None,
            date_filters: None,
            match_mode: None,
            sort: None,
            limit: None,
        })
        .expect("filtered search should complete");

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].path, "cover.psd");

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn search_assets_preserves_virtual_entry_markers() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("search-virtual");
    fs::write(repo_root.join("virtual-track.mp3"), b"track")
        .expect("virtual track file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);

    let repo = state
        .load_repository_record(&repo_id)
        .expect("repository record should load");
    let connection = state
        .open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )
        .expect("repository connection should open");
    connection
        .execute(
            r#"
            UPDATE assets
            SET is_virtual = 1,
                provider_id = ?2,
                provider_item_id = ?3,
                source_payload_json = ?4,
                local_absolute_path = ?5
            WHERE repo_id = ?1 AND path = 'virtual-track.mp3'
            "#,
            params![
                repo_id,
                "netease-cloud-music",
                "123456",
                serde_json::json!({
                    "provider": "netease-cloud-music",
                    "songId": 123456,
                    "playlistId": 42,
                })
                .to_string(),
                Option::<String>::None,
            ],
        )
        .expect("asset should be marked virtual");
    drop(connection);

    let response = state
        .search_assets(SearchRequest {
            query: "virtual-track".to_string(),
            repo_id: Some(repo_id.clone()),
            exclude_query: None,
            metadata_key: None,
            metadata_value: None,
            tag: None,
            tags: None,
            metadata_filters: None,
            formats: Some(vec!["mp3".to_string()]),
            min_rating: None,
            exclude_tags: None,
            exclude_formats: None,
            exclude_metadata_filters: None,
            exclude_path_prefixes: None,
            exclude_number_filters: None,
            exclude_date_filters: None,
            number_filters: None,
            date_filters: None,
            match_mode: None,
            sort: None,
            limit: None,
        })
        .expect("virtual search should complete");

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].path, "virtual-track.mp3");
    assert!(response.results[0].is_virtual);
    assert_eq!(
        response.results[0].provider_id.as_deref(),
        Some("netease-cloud-music")
    );
    assert_eq!(
        response.results[0].provider_item_id.as_deref(),
        Some("123456")
    );
    assert_eq!(
        response.results[0]
            .source_payload
            .as_ref()
            .and_then(|value| value.get("songId"))
            .and_then(serde_json::Value::as_i64),
        Some(123456)
    );
    assert_eq!(response.results[0].local_absolute_path, None);

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn repository_actions_list_run_and_reject_unsafe_states() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("repository-actions");
    fs::write(repo_root.join("cover.png"), b"cover").expect("cover file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);
    let asset_id = asset_id_for_test_path(&state, &repo_id, "cover.png");

    let repo = state
        .load_repository_record(&repo_id)
        .expect("repository record should load");
    let connection = state
        .open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )
        .expect("repository connection should open");
    let now = now_rfc3339();
    connection
        .execute(
            r#"
            INSERT INTO repository_actions (
              action_id, repo_id, source, source_action_id, name, status, enabled,
              raw_json, unsupported_reason, sort_order, created_at, updated_at
            )
            VALUES
              ('action-ready', ?1, 'eagle-importer', 'source-ready', '标记精选', 'ready', 1, '{}', NULL, 0, ?2, ?2),
              ('action-disabled', ?1, 'eagle-importer', 'source-disabled', '停用动作', 'ready', 0, '{}', NULL, 1, ?2, ?2),
              ('action-unsupported', ?1, 'eagle-importer', 'source-unsupported', '未知动作', 'unsupported', 0, '{}', 'unsupported action step: shell', 2, ?2, ?2)
            "#,
            params![repo_id, now],
        )
        .expect("actions should be inserted");
    connection
        .execute(
            r#"
            INSERT INTO repository_action_steps (
              step_id, action_id, repo_id, step_kind, label, status,
              config_json, raw_json, unsupported_reason, sort_order
            )
            VALUES
              ('step-ready-1', 'action-ready', ?1, 'metadata.update', '更新评分', 'ready', '{"metadata":{"rating":5,"comment":"Action run"}}', '{"type":"rating"}', NULL, 0),
              ('step-ready-2', 'action-ready', ?1, 'tagGroups.set', '设置标签', 'ready', '{"tags":["精选"]}', '{"type":"tags"}', NULL, 1),
              ('step-disabled-1', 'action-disabled', ?1, 'metadata.update', '更新评分', 'ready', '{"metadata":{"rating":4}}', '{"type":"rating"}', NULL, 0),
              ('step-unsupported-1', 'action-unsupported', ?1, 'unsupported', '外部脚本', 'unsupported', '{}', '{"type":"shell"}', 'unsupported action step: shell', 0)
            "#,
            [repo_id.as_str()],
        )
        .expect("action steps should be inserted");
    drop(connection);

    let actions = state
        .list_repository_actions(&repo_id)
        .expect("actions should list");
    assert_eq!(
        actions
            .iter()
            .map(|action| action.name.as_str())
            .collect::<Vec<_>>(),
        vec!["标记精选", "停用动作", "未知动作"]
    );
    assert_eq!(actions[0].steps.len(), 2);

    let disabled_error = state
        .run_repository_action(RepositoryActionRunRequest {
            repo_id: repo_id.clone(),
            action_id: "action-disabled".to_string(),
            target_paths: Some(vec!["cover.png".to_string()]),
            asset_ids: None,
        })
        .expect_err("disabled action should be rejected");
    assert!(disabled_error.contains("disabled"));

    let unsupported_error = state
        .set_repository_action_enabled(RepositoryActionEnabledRequest {
            repo_id: repo_id.clone(),
            action_id: "action-unsupported".to_string(),
            enabled: true,
        })
        .expect_err("unsupported action cannot be enabled");
    assert!(unsupported_error.contains("unsupported"));

    let missing_target_error = state
        .run_repository_action(RepositoryActionRunRequest {
            repo_id: repo_id.clone(),
            action_id: "action-ready".to_string(),
            target_paths: None,
            asset_ids: None,
        })
        .expect_err("targetless action should be rejected");
    assert!(missing_target_error.contains("explicit non-empty targetPaths or assetIds"));

    let response = state
        .run_repository_action(RepositoryActionRunRequest {
            repo_id: repo_id.clone(),
            action_id: "action-ready".to_string(),
            target_paths: Some(vec!["cover.png".to_string()]),
            asset_ids: None,
        })
        .expect("ready action should run");
    assert_eq!(response.run.status, "success");
    assert_eq!(
        response
            .action
            .last_run
            .as_ref()
            .map(|run| run.status.as_str()),
        Some("success")
    );

    let detail = state
        .load_asset_detail(&repo_id, &asset_id)
        .expect("asset detail should load after action");
    let metadata = detail
        .metadata
        .iter()
        .map(|entry| (entry.key.as_str(), entry.value.clone()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(metadata.get("rating"), Some(&serde_json::json!(5)));
    assert_eq!(
        metadata.get("comment"),
        Some(&serde_json::json!("Action run"))
    );
    assert_eq!(
        metadata.get("tagGroups"),
        Some(&serde_json::json!(["精选"]))
    );
    assert!(detail
        .revisions
        .iter()
        .any(|revision| revision.source == "repository-action:action-ready"));

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn search_and_smart_folders_apply_exclude_filters_after_include_filters() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("search-excludes");
    fs::create_dir_all(repo_root.join("Archive")).expect("archive directory should be created");
    fs::write(repo_root.join("hero.png"), b"hero").expect("hero file should be written");
    fs::write(repo_root.join("draft.png"), b"draft").expect("draft file should be written");
    fs::write(repo_root.join("Archive/old.png"), b"old").expect("archived file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);

    let repo = state
        .load_repository_record(&repo_id)
        .expect("repository record should load");
    let connection = state
        .open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )
        .expect("repository connection should open");
    let now = now_rfc3339();
    let ids = ["hero.png", "draft.png", "Archive/old.png"]
        .into_iter()
        .map(|path| {
            let asset_id: String = connection
                .query_row(
                    "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2",
                    params![repo_id.as_str(), path],
                    |row| row.get(0),
                )
                .expect("asset id should load");
            (path.to_string(), asset_id)
        })
        .collect::<BTreeMap<_, _>>();
    for (path, width, created_at, note) in [
        (
            "hero.png",
            serde_json::json!(1920),
            serde_json::json!("2024-02-02T00:00:00Z"),
            serde_json::json!("final hero"),
        ),
        (
            "draft.png",
            serde_json::json!(480),
            serde_json::json!("2024-02-02T00:00:00Z"),
            serde_json::json!("draft hero"),
        ),
        (
            "Archive/old.png",
            serde_json::json!(1920),
            serde_json::json!("2024-01-10T00:00:00Z"),
            serde_json::json!("old hero"),
        ),
    ] {
        for (key, value) in [
            ("width", width),
            ("fileCreatedAt", created_at),
            ("note", note),
        ] {
            connection
                .execute(
                    r#"
                    INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                    VALUES (?1, ?2, ?3, ?4, 1, ?5)
                    "#,
                    params![
                        ids[path].as_str(),
                        key,
                        infer_value_type(&value),
                        value.to_string(),
                        now
                    ],
                )
                .expect("metadata should be written");
        }
    }
    drop(connection);

    let response = state
        .search_assets(SearchRequest {
            query: "hero".to_string(),
            repo_id: Some(repo_id.clone()),
            exclude_query: Some("draft".to_string()),
            metadata_key: None,
            metadata_value: None,
            tag: None,
            tags: None,
            metadata_filters: None,
            exclude_tags: None,
            exclude_formats: None,
            exclude_metadata_filters: None,
            exclude_path_prefixes: Some(vec!["Archive".to_string()]),
            exclude_number_filters: Some(vec![SearchNumberFilter {
                key: "width".to_string(),
                min: None,
                max: Some(640.0),
            }]),
            exclude_date_filters: Some(vec![SearchDateFilter {
                key: "fileCreatedAt".to_string(),
                from: Some("2024-01-01T00:00:00Z".to_string()),
                to: Some("2024-01-31T00:00:00Z".to_string()),
            }]),
            number_filters: None,
            date_filters: None,
            formats: Some(vec!["png".to_string()]),
            min_rating: None,
            match_mode: None,
            sort: None,
            limit: None,
        })
        .expect("exclude search should complete");
    assert_eq!(
        response
            .results
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>(),
        vec!["hero.png"]
    );

    state
        .create_smart_folder(SmartFolderMutationRequest {
            repo_id: repo_id.clone(),
            smart_folder_id: Some("smart-hero".to_string()),
            parent_id: None,
            name: "Hero".to_string(),
            filter: SmartFolderFilter {
                query: Some("hero".to_string()),
                formats: Some(vec!["png".to_string()]),
                exclude_query: Some("draft".to_string()),
                exclude_path_prefixes: Some(vec!["Archive".to_string()]),
                exclude_number_filters: Some(vec![SearchNumberFilter {
                    key: "width".to_string(),
                    min: None,
                    max: Some(640.0),
                }]),
                exclude_date_filters: Some(vec![SearchDateFilter {
                    key: "fileCreatedAt".to_string(),
                    from: Some("2024-01-01T00:00:00Z".to_string()),
                    to: Some("2024-01-31T00:00:00Z".to_string()),
                }]),
                ..SmartFolderFilter::default()
            },
        })
        .expect("smart folder should be created");
    let smart_result = state
        .query_smart_folder(&repo_id, "smart-hero")
        .expect("smart folder should query");
    assert_eq!(
        smart_result
            .results
            .iter()
            .map(|item| item.path.as_str())
            .collect::<Vec<_>>(),
        vec!["hero.png"]
    );

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn plugin_manifest_infers_category_for_legacy_kind() {
    let manifest = parse_plugin_manifest(
        r#"{
          "pluginId": "user.legacy-webdav",
          "legacyPluginIds": [],
          "name": "Legacy WebDAV",
          "version": "0.1.0",
          "kind": "webdav",
          "description": "Legacy source plugin.",
          "capabilities": ["browse"],
          "enabled": true,
          "sdk": "backend",
          "entry": {},
          "source": "user",
          "runtime": "manifest-only",
          "permissions": [],
          "compat": {
            "sdkVersion": "1",
            "legacyPluginIds": []
          },
          "status": "ready"
        }"#,
    )
    .expect("legacy manifest should parse");

    assert_eq!(manifest.category, "source");
    assert!(is_repository_backend_plugin(&manifest));
    assert!(manifest.requires.is_empty());
    assert!(manifest.optional.is_empty());
    assert!(manifest.hooks.is_empty());
    assert!(manifest.contributes.is_object());
}

#[test]
fn search_assets_match_mode_or_spans_filter_families_and_metadata_sort_is_typed() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("search-or-sort");
    fs::write(repo_root.join("tag-only.png"), b"tag").expect("tag-only file should be written");
    fs::write(repo_root.join("metadata-only.png"), b"metadata")
        .expect("metadata-only file should be written");
    fs::write(repo_root.join("small.png"), b"small").expect("small file should be written");
    fs::write(repo_root.join("large.png"), b"large").expect("large file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);

    let repo = state
        .load_repository_record(&repo_id)
        .expect("repository record should load");
    let connection = state
        .open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )
        .expect("repository connection should open");
    let now = now_rfc3339();
    let ids = [
        "tag-only.png",
        "metadata-only.png",
        "small.png",
        "large.png",
    ]
    .into_iter()
    .map(|path| {
        let asset_id: String = connection
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2",
                params![repo_id.as_str(), path],
                |row| row.get(0),
            )
            .expect("asset id should load");
        (path.to_string(), asset_id)
    })
    .collect::<BTreeMap<_, _>>();

    connection
        .execute(
            "INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag) VALUES (?1, ?2, ?3)",
            params![ids["tag-only.png"].as_str(), "Poster", "poster"],
        )
        .expect("tag should be written");
    for (path, width, created_at) in [
        (
            "metadata-only.png",
            serde_json::json!(1920),
            serde_json::json!("2024-01-04T00:00:00Z"),
        ),
        (
            "small.png",
            serde_json::json!(800),
            serde_json::json!("2024-01-01T00:00:00Z"),
        ),
        (
            "large.png",
            serde_json::json!(1920),
            serde_json::json!("2024-01-03T00:00:00Z"),
        ),
    ] {
        for (key, value) in [("width", width), ("fileCreatedAt", created_at)] {
            connection
                .execute(
                    r#"
                    INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                    VALUES (?1, ?2, ?3, ?4, 1, ?5)
                    "#,
                    params![
                        ids[path].as_str(),
                        key,
                        infer_value_type(&value),
                        value.to_string(),
                        now
                    ],
                )
                .expect("metadata should be written");
        }
    }
    drop(connection);

    let or_response = state
        .search_assets(SearchRequest {
            query: String::new(),
            repo_id: Some(repo_id.clone()),
            exclude_query: None,
            metadata_key: None,
            metadata_value: None,
            tag: None,
            tags: Some(vec!["Poster".to_string()]),
            metadata_filters: Some(vec![SearchMetadataFilter {
                key: "width".to_string(),
                value: "1920".to_string(),
            }]),
            exclude_tags: None,
            exclude_formats: None,
            exclude_metadata_filters: None,
            exclude_path_prefixes: None,
            exclude_number_filters: None,
            exclude_date_filters: None,
            number_filters: None,
            date_filters: None,
            formats: None,
            min_rating: None,
            match_mode: Some("or".to_string()),
            sort: Some(SearchSort {
                field: "metadata.fileCreatedAt".to_string(),
                direction: "asc".to_string(),
            }),
            limit: None,
        })
        .expect("or search should complete");

    let or_paths = or_response
        .results
        .iter()
        .map(|item| item.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(or_paths.len(), 3);
    assert!(or_paths.contains(&"tag-only.png"));
    assert!(or_paths.contains(&"metadata-only.png"));
    assert!(or_paths.contains(&"large.png"));

    let width_sorted = state
        .search_assets(SearchRequest {
            query: String::new(),
            repo_id: Some(repo_id.clone()),
            exclude_query: None,
            metadata_key: None,
            metadata_value: None,
            tag: None,
            tags: None,
            metadata_filters: None,
            exclude_tags: None,
            exclude_formats: None,
            exclude_metadata_filters: None,
            exclude_path_prefixes: None,
            exclude_number_filters: None,
            exclude_date_filters: None,
            number_filters: None,
            date_filters: None,
            formats: Some(vec!["png".to_string()]),
            min_rating: None,
            match_mode: None,
            sort: Some(SearchSort {
                field: "metadata.width".to_string(),
                direction: "asc".to_string(),
            }),
            limit: None,
        })
        .expect("metadata width sort should complete");

    let sorted_paths = width_sorted
        .results
        .iter()
        .map(|item| item.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        sorted_paths,
        vec![
            "small.png",
            "large.png",
            "metadata-only.png",
            "tag-only.png"
        ]
    );

    let date_sorted = state
        .search_assets(SearchRequest {
            query: String::new(),
            repo_id: Some(repo_id.clone()),
            exclude_query: None,
            metadata_key: None,
            metadata_value: None,
            tag: None,
            tags: None,
            metadata_filters: Some(vec![SearchMetadataFilter {
                key: "width".to_string(),
                value: "1920".to_string(),
            }]),
            exclude_tags: None,
            exclude_formats: None,
            exclude_metadata_filters: None,
            exclude_path_prefixes: None,
            exclude_number_filters: None,
            exclude_date_filters: None,
            number_filters: None,
            date_filters: None,
            formats: None,
            min_rating: None,
            match_mode: None,
            sort: Some(SearchSort {
                field: "metadata.fileCreatedAt".to_string(),
                direction: "asc".to_string(),
            }),
            limit: None,
        })
        .expect("metadata date sort should complete");
    let date_paths = date_sorted
        .results
        .iter()
        .map(|item| item.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(date_paths, vec!["large.png", "metadata-only.png"]);

    let random_sorted = state
        .search_assets(SearchRequest {
            query: String::new(),
            repo_id: Some(repo_id),
            exclude_query: None,
            metadata_key: None,
            metadata_value: None,
            tag: None,
            tags: None,
            metadata_filters: None,
            exclude_tags: None,
            exclude_formats: None,
            exclude_metadata_filters: None,
            exclude_path_prefixes: None,
            exclude_number_filters: None,
            exclude_date_filters: None,
            number_filters: None,
            date_filters: None,
            formats: Some(vec!["png".to_string()]),
            min_rating: None,
            match_mode: None,
            sort: Some(SearchSort {
                field: "random".to_string(),
                direction: "asc".to_string(),
            }),
            limit: None,
        })
        .expect("core random sort should complete");
    let random_paths = random_sorted
        .results
        .iter()
        .map(|item| item.path.as_str())
        .collect::<HashSet<_>>();
    assert_eq!(
        random_paths,
        HashSet::from([
            "large.png",
            "metadata-only.png",
            "small.png",
            "tag-only.png",
        ])
    );

    fs::remove_dir_all(root).expect("test temp root should be removed");
}
