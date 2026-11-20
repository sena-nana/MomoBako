//! 缩略图缓存、预览与播放源准备、缩略图命名与 ffmpeg 参数的测试。

use super::*;

#[test]
fn ensure_thumbnail_reuses_existing_cache_path() {
    let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-reuse");
    write_test_image(&repo_root.join("cover.png"));
    let repo_id = create_repository_for_path(&state, &repo_root);
    let snapshot = state
        .load_snapshot(&repo_id)
        .expect("snapshot should load after sync");
    let asset_id = snapshot.assets[0].asset_id.clone();
    let thumbnail_dir = thumbnail_root.join(thumbnail_repository_dir_name(
        &repo_id,
        &repo_root.to_string_lossy(),
    ));
    fs::create_dir_all(&thumbnail_dir).expect("thumbnail dir should be created");
    let thumbnail_path = thumbnail_dir.join(thumbnail_file_name(
        &repo_id,
        &repo_root.to_string_lossy(),
        "cover.png",
        "file",
        "generated",
    ));
    write_test_image(&thumbnail_path);

    let storage_paths = ensure_repository_storage_paths(
        &state.root,
        &repo_id,
        &repo_root,
        LOCAL_FILESYSTEM_PLUGIN_ID,
    )
    .expect("storage paths should resolve");
    let connection =
        Connection::open(storage_paths.database_path).expect("repository db should open");
    connection
        .execute(
            "UPDATE assets SET thumbnail_path = ?3 WHERE repo_id = ?1 AND asset_id = ?2",
            params![
                repo_id,
                asset_id,
                thumbnail_path.to_string_lossy().to_string()
            ],
        )
        .expect("asset thumbnail path should update");

    let response = state
        .ensure_thumbnail(ThumbnailRequest {
            repo_id,
            path: "cover.png".to_string(),
            action: None,
            source_path: None,
            source_url: None,
            image_bytes: None,
            media_type: None,
        })
        .expect("thumbnail should be ensured");

    assert_eq!(
        response.thumbnail_path,
        Some(thumbnail_path.to_string_lossy().to_string())
    );

    drop(connection);
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn ensure_repository_storage_paths_use_local_root_capability_for_custom_filesystem_plugin() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("custom-local-root");
    let runtime_root = runtime_plugins_dir(&state.root);
    write_test_plugin_archive_with_manifest(
        &runtime_root.join("custom-local-root.momoplug"),
        test_plugin_manifest_json(
            "user.custom-local-root",
            "Custom Local Root",
            serde_json::json!({
                "kind": "filesystem",
                "category": "source",
                "type": {
                    "layer": "source",
                    "kind": "filesystem"
                },
                "capabilities": ["browse", "read", "write", "localRootPath"],
                "runtime": "manifest-only",
                "source": "user"
            }),
        ),
    );

    let storage_paths = ensure_repository_storage_paths(
        &state.root,
        "repo-custom-local-root",
        &repo_root,
        "user.custom-local-root",
    )
    .expect("storage paths should resolve");

    assert_eq!(storage_paths.metadata_dir, repo_root.join(REPO_META_DIR));

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn ensure_thumbnail_migrates_existing_cache_path_to_repository_metadata_dir() {
    let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-migrate");
    write_test_image(&repo_root.join("cover.png"));
    let repo_id = create_repository_for_path(&state, &repo_root);
    let snapshot = state
        .load_snapshot(&repo_id)
        .expect("snapshot should load after sync");
    let asset_id = snapshot.assets[0].asset_id.clone();
    let legacy_root = root.join("legacy-thumbnails");
    let legacy_dir = legacy_root.join(thumbnail_repository_dir_name(
        &repo_id,
        &repo_root.to_string_lossy(),
    ));
    fs::create_dir_all(&legacy_dir).expect("legacy thumbnail dir should be created");
    let legacy_thumbnail_path = legacy_dir.join(thumbnail_file_name(
        &repo_id,
        &repo_root.to_string_lossy(),
        "cover.png",
        "file",
        "generated",
    ));
    write_test_image(&legacy_thumbnail_path);

    let storage_paths = ensure_repository_storage_paths(
        &state.root,
        &repo_id,
        &repo_root,
        LOCAL_FILESYSTEM_PLUGIN_ID,
    )
    .expect("storage paths should resolve");
    let connection =
        Connection::open(storage_paths.database_path).expect("repository db should open");
    connection
        .execute(
            "UPDATE assets SET thumbnail_path = ?3 WHERE repo_id = ?1 AND asset_id = ?2",
            params![
                repo_id,
                asset_id,
                legacy_thumbnail_path.to_string_lossy().to_string()
            ],
        )
        .expect("asset thumbnail path should update");

    let response = state
        .ensure_thumbnail(ThumbnailRequest {
            repo_id: repo_id.clone(),
            path: "cover.png".to_string(),
            action: None,
            source_path: None,
            source_url: None,
            image_bytes: None,
            media_type: None,
        })
        .expect("thumbnail should be ensured");
    let thumbnail_path = response
        .thumbnail_path
        .as_deref()
        .map(Path::new)
        .expect("thumbnail path should be returned");

    assert!(thumbnail_path.starts_with(&thumbnail_root));
    assert!(thumbnail_path.is_file());
    assert_ne!(thumbnail_path, legacy_thumbnail_path.as_path());
    let stored_path: String = connection
        .query_row(
            "SELECT thumbnail_path FROM assets WHERE repo_id = ?1 AND asset_id = ?2",
            params![repo_id, asset_id],
            |row| row.get(0),
        )
        .expect("asset thumbnail path should load");
    assert_eq!(stored_path, thumbnail_path.to_string_lossy());

    drop(connection);
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn ensure_thumbnail_migrates_custom_entry_cache_path_to_repository_metadata_dir() {
    let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-custom-migrate");
    fs::create_dir_all(repo_root.join("Shots")).expect("directory should be created");
    let repo_id = create_repository_for_path(&state, &repo_root);
    let legacy_root = root.join("legacy-thumbnails");
    let legacy_dir = legacy_root.join(thumbnail_repository_dir_name(
        &repo_id,
        &repo_root.to_string_lossy(),
    ));
    fs::create_dir_all(&legacy_dir).expect("legacy thumbnail dir should be created");
    let legacy_thumbnail_path = legacy_dir.join(thumbnail_file_name(
        &repo_id,
        &repo_root.to_string_lossy(),
        "Shots",
        "directory",
        "custom",
    ));
    write_test_image(&legacy_thumbnail_path);

    let storage_paths = ensure_repository_storage_paths(
        &state.root,
        &repo_id,
        &repo_root,
        LOCAL_FILESYSTEM_PLUGIN_ID,
    )
    .expect("storage paths should resolve");
    let connection =
        Connection::open(storage_paths.database_path).expect("repository db should open");
    upsert_entry_thumbnail_record(
        &connection,
        &repo_id,
        "Shots",
        "directory",
        &legacy_thumbnail_path.to_string_lossy(),
        true,
    )
    .expect("entry thumbnail should be seeded");

    let response = state
        .ensure_thumbnail(ThumbnailRequest {
            repo_id: repo_id.clone(),
            path: "Shots".to_string(),
            action: None,
            source_path: None,
            source_url: None,
            image_bytes: None,
            media_type: None,
        })
        .expect("thumbnail should be ensured");
    let thumbnail_path = response
        .thumbnail_path
        .as_deref()
        .map(Path::new)
        .expect("thumbnail path should be returned");

    assert!(response.thumbnail_custom);
    assert!(thumbnail_path.starts_with(&thumbnail_root));
    assert!(thumbnail_path.is_file());
    assert_ne!(thumbnail_path, legacy_thumbnail_path.as_path());
    let stored_path: String = connection
        .query_row(
            "SELECT thumbnail_path FROM entry_thumbnails WHERE repo_id = ?1 AND path = ?2 AND kind = ?3",
            params![repo_id, "Shots", "directory"],
            |row| row.get(0),
        )
        .expect("entry thumbnail path should load");
    assert_eq!(stored_path, thumbnail_path.to_string_lossy());

    drop(connection);
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn ensure_thumbnail_writes_cache_under_repository_metadata_dir() {
    let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-repo-meta");
    write_test_image(&repo_root.join("cover.png"));
    let repo_id = create_repository_for_path(&state, &repo_root);

    let response = state
        .ensure_thumbnail(ThumbnailRequest {
            repo_id,
            path: "cover.png".to_string(),
            action: None,
            source_path: None,
            source_url: None,
            image_bytes: None,
            media_type: None,
        })
        .expect("thumbnail should be generated");
    let thumbnail_path = response
        .thumbnail_path
        .as_deref()
        .map(Path::new)
        .expect("thumbnail path should be returned");

    assert!(thumbnail_path.starts_with(&thumbnail_root));
    assert!(thumbnail_path.is_file());
    assert_eq!(count_files(&thumbnail_root), 1);

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn ensure_thumbnail_saves_remote_source_url_as_custom_thumbnail() {
    let (state, root, repo_root, thumbnail_root) = create_test_state("thumb-remote-source");
    fs::write(repo_root.join("track.mp3"), b"fake audio").expect("track file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);
    let mut body = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([220, 80, 40])))
        .write_to(&mut body, image::ImageFormat::Png)
        .expect("test thumbnail image should encode");
    let source_url = serve_test_http_body(body.into_inner());

    let response = state
        .ensure_thumbnail(ThumbnailRequest {
            repo_id: repo_id.clone(),
            path: "track.mp3".to_string(),
            action: Some("save".to_string()),
            source_path: None,
            source_url: Some(source_url),
            image_bytes: None,
            media_type: None,
        })
        .expect("remote thumbnail should be saved");
    let thumbnail_path = response
        .thumbnail_path
        .as_deref()
        .map(Path::new)
        .expect("thumbnail path should be returned");

    assert!(response.thumbnail_custom);
    assert!(thumbnail_path.starts_with(&thumbnail_root));
    assert!(thumbnail_path.is_file());
    assert!(response
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("thumbnailPalette"))
        .is_some());

    let snapshot = state
        .load_file_browser(FileBrowserRequest {
            repo_id,
            directory_path: Some(String::new()),
            include_tree: Some(false),
            special_location: None,
            offset: None,
            limit: None,
        })
        .expect("file browser should load");
    let entry = snapshot
        .entries
        .iter()
        .find(|item| item.path == "track.mp3")
        .expect("track entry should be listed");
    assert!(entry.thumbnail_custom);
    assert_eq!(
        entry.thumbnail_path.as_deref(),
        Some(thumbnail_path.to_string_lossy().as_ref())
    );

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn ensure_thumbnail_extracts_palette_metadata() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("thumb-palette");
    write_test_image(&repo_root.join("cover.png"));
    let repo_id = create_repository_for_path(&state, &repo_root);

    let response = state
        .ensure_thumbnail(ThumbnailRequest {
            repo_id: repo_id.clone(),
            path: "cover.png".to_string(),
            action: None,
            source_path: None,
            source_url: None,
            image_bytes: None,
            media_type: None,
        })
        .expect("thumbnail should be generated");
    let palette = response
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("thumbnailPalette"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .expect("thumbnail palette should be returned");

    assert!(!palette.is_empty());
    assert!(palette
        .iter()
        .all(|item| item.as_str().is_some_and(|value| value.starts_with('#'))));

    let snapshot = state
        .load_file_browser(FileBrowserRequest {
            repo_id,
            directory_path: Some(String::new()),
            include_tree: Some(false),
            special_location: None,
            offset: None,
            limit: None,
        })
        .expect("file browser should load");
    let entry = snapshot
        .entries
        .iter()
        .find(|item| item.path == "cover.png")
        .expect("cover entry should be listed");
    assert_eq!(
        entry.metadata.get("thumbnailPalette"),
        Some(&serde_json::Value::Array(palette))
    );

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn ensure_thumbnail_returns_null_for_unsupported_types() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("thumb-unsupported");
    fs::write(repo_root.join("note.txt"), "plain text").expect("text file should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);

    let response = state
        .ensure_thumbnail(ThumbnailRequest {
            repo_id,
            path: "note.txt".to_string(),
            action: None,
            source_path: None,
            source_url: None,
            image_bytes: None,
            media_type: None,
        })
        .expect("unsupported thumbnail request should succeed");

    assert_eq!(response.thumbnail_path, None);

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn prepare_preview_file_source_reports_local_path_and_media_type() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("preview-prepare");
    let source_path = repo_root.join("model.glb");
    fs::write(&source_path, b"glb").expect("preview source should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);
    let response = state
        .prepare_preview_file_source(FileReadRequest {
            repo_id,
            path: "model.glb".to_string(),
        })
        .expect("preview source should be prepared");

    assert_eq!(response.media_type, "model/gltf-binary");
    assert_eq!(response.size_bytes, 3);
    assert!(!response.token.is_empty());
    let local_path = response.local_path.expect("local path should be reported");
    assert_eq!(
        fs::read(local_path).expect("local path should read"),
        b"glb"
    );
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn prepare_entry_playback_source_uses_backend_stat_for_unindexed_virtual_tracks() {
    let _lock = playback_test_lock();
    let (state, root, repo_root, _thumbnail_root) =
        create_test_state("prepare-entry-playback-unindexed-virtual");
    let repo_id = create_repository_without_initial_sync(&state, &repo_root);
    let expected_repo_id = repo_id.clone();

    fn stat_hook(
        _repo: &RepositoryRecord,
        _repo_root: &Path,
        entry_path: &str,
    ) -> Option<Result<FileSystemEntry, String>> {
        if entry_path != "Created/lazy-track.mp3" {
            return None;
        }
        Some(Ok(FileSystemEntry {
            path: entry_path.to_string(),
            name: "lazy-track.mp3".to_string(),
            kind: FileSystemEntryKind::File,
            extension: Some("mp3".to_string()),
            size_bytes: None,
            modified_at: Some("2026-06-14T00:00:00Z".to_string()),
            is_virtual: true,
            provider_id: Some("netease-cloud-music".to_string()),
            provider_item_id: Some("3001".to_string()),
            source_payload: Some(serde_json::json!({
                "provider": "netease-cloud-music",
                "songId": "3001",
                "accountCookie": "MUSIC_U=lazy-cookie",
                "accountId": "42",
                "level": "exhigh"
            })),
            local_absolute_path: None,
            status: None,
            shared_asset_id: None,
            tags: None,
            thumbnail_local_absolute_path: None,
        }))
    }

    fn playback_hook(payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let expected_repo_id = std::env::var("MOMOBKO_TEST_EXPECTED_REPO_ID")
            .expect("expected repo id should be provided");
        assert_eq!(payload["songId"], serde_json::json!(3001));
        assert!(payload.get("accountCookie").is_none());
        assert_eq!(payload["level"], serde_json::json!("exhigh"));
        assert_eq!(payload["repoId"], serde_json::json!(expected_repo_id));
        assert_eq!(
            payload["entryPath"],
            serde_json::json!("Created/lazy-track.mp3")
        );
        Ok(serde_json::json!({
            "localPath": "C:/Mock/Temp/lazy-track.mp3",
            "tempFilePath": "C:/Mock/Temp/lazy-track.mp3",
            "mediaType": "audio/mpeg"
        }))
    }

    std::env::set_var("MOMOBKO_TEST_EXPECTED_REPO_ID", &expected_repo_id);
    set_test_backend_stat_entry_hook(Some(stat_hook));
    set_test_downloader_playback_hook(Some(playback_hook));
    let mut ignore_progress = |_| Ok(());
    let response = state
        .prepare_entry_playback_source_with_progress(
            EntryPlaybackRequest {
                repo_id: repo_id.clone(),
                path: "Created/lazy-track.mp3".to_string(),
            },
            &mut ignore_progress,
        )
        .expect("unindexed virtual playback source should resolve from backend stat");
    set_test_downloader_playback_hook(None);
    set_test_backend_stat_entry_hook(None);
    std::env::remove_var("MOMOBKO_TEST_EXPECTED_REPO_ID");

    assert_eq!(response.repo_id, repo_id);
    assert_eq!(response.path, "Created/lazy-track.mp3");
    assert_eq!(response.media_type, "audio/mpeg");
    assert_eq!(
        response.local_path.as_deref(),
        Some("C:/Mock/Temp/lazy-track.mp3")
    );

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn prepare_preview_file_source_rejects_deleted_source_file() {
    let (state, root, repo_root, _thumbnail_root) = create_test_state("preview-deleted");
    let source_path = repo_root.join("model.glb");
    fs::write(&source_path, b"glb").expect("preview source should be written");
    let repo_id = create_repository_for_path(&state, &repo_root);
    fs::remove_file(source_path).expect("preview source should be removed");

    let error = state
        .prepare_preview_file_source(FileReadRequest {
            repo_id,
            path: "model.glb".to_string(),
        })
        .expect_err("deleted preview source should fail");

    assert!(error.contains("file not found"), "{error}");
    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn thumbnail_file_name_stays_within_windows_component_limit() {
    let asset_id = format!(
        "asset-{}",
        slugify_repo_id("startIcon.png", LONG_RELATIVE_PATH)
    );
    let repo_dir = thumbnail_repository_dir_name(&asset_id, LONG_RELATIVE_PATH);
    let file_name = thumbnail_file_name(
        &asset_id,
        LONG_RELATIVE_PATH,
        LONG_RELATIVE_PATH,
        "file",
        "generated",
    );

    assert!(repo_dir.len() <= 255);
    assert!(file_name.len() <= 255);
    assert!(file_name.ends_with(".jpg"));
    assert_eq!(file_name.len(), 68);
}

#[test]
fn thumbnail_cache_names_are_stable() {
    let repo_dir = thumbnail_repository_dir_name("repo-cubism", "C:/Assets/Cubism");
    let file_name = thumbnail_file_name(
        "repo-cubism",
        "C:/Assets/Cubism",
        LONG_RELATIVE_PATH,
        "file",
        "generated",
    );

    assert_eq!(
        repo_dir,
        thumbnail_repository_dir_name("repo-cubism", "C:/Assets/Cubism")
    );
    assert_eq!(
        file_name,
        thumbnail_file_name(
            "repo-cubism",
            "C:/Assets/Cubism",
            LONG_RELATIVE_PATH,
            "file",
            "generated",
        )
    );
}

#[test]
fn thumbnail_cache_names_differ_for_different_paths() {
    let first = thumbnail_file_name(
        "repo-cubism",
        "C:/Assets/Cubism",
        LONG_RELATIVE_PATH,
        "file",
        "generated",
    );
    let second = thumbnail_file_name(
        "repo-cubism",
        "C:/Assets/Cubism",
        "CubismSdkForNative-5-r.5/Samples/OpenGL/Demo/proj.harmonyos.cmake/Full/entry/src/ohosTest/resources/base/media/icon.png",
        "file",
        "generated",
    );

    assert_ne!(first, second);
}

#[test]
fn video_thumbnail_ffmpeg_args_write_a_single_image() {
    let source_path = Path::new("C:/Assets/video.mp4");
    let thumbnail_path = Path::new("C:/Cache/thumbnail.jpg");
    let args = video_thumbnail_ffmpeg_args(source_path, thumbnail_path);

    assert!(args.windows(2).any(|items| items == ["-frames:v", "1"]));
    assert!(args.windows(2).any(|items| items == ["-update", "1"]));

    let update_index = args
        .iter()
        .position(|item| item == "-update")
        .expect("missing -update");
    let output_index = args
        .iter()
        .position(|item| item == thumbnail_path.as_os_str())
        .expect("missing output path");
    assert!(update_index < output_index);
}

#[test]
fn audio_thumbnail_ffmpeg_args_extract_the_first_embedded_cover_stream() {
    let source_path = Path::new("C:/Assets/track.flac");
    let thumbnail_path = Path::new("C:/Cache/thumbnail.jpg");
    let args = audio_thumbnail_ffmpeg_args(source_path, thumbnail_path);

    assert!(args.windows(2).any(|items| items == ["-map", "0:v:0"]));
    assert!(args.windows(2).any(|items| items == ["-frames:v", "1"]));
    assert!(args.windows(2).any(|items| items == ["-update", "1"]));

    let map_index = args
        .iter()
        .position(|item| item == "-map")
        .expect("missing -map");
    let output_index = args
        .iter()
        .position(|item| item == thumbnail_path.as_os_str())
        .expect("missing output path");
    assert!(map_index < output_index);
}

#[test]
fn audio_cover_probe_args_select_video_streams_as_json() {
    let source_path = Path::new("C:/Assets/track.mp3");
    let args = audio_cover_probe_args(source_path);

    assert!(args
        .windows(2)
        .any(|items| items == ["-select_streams", "v"]));
    assert!(args
        .windows(2)
        .any(|items| items == ["-show_entries", "stream=index"]));
    assert!(args.windows(2).any(|items| items == ["-of", "json"]));
    assert_eq!(args.last(), Some(&source_path.as_os_str().to_os_string()));
}

#[test]
fn audio_cover_probe_output_reports_missing_streams() {
    assert!(!audio_cover_probe_output_has_stream(br#"{"streams":[]}"#)
        .expect("probe output should parse"));
    assert!(!audio_cover_probe_output_has_stream(br#"{}"#).expect("probe output should parse"));
}

#[test]
fn audio_cover_probe_output_reports_present_streams() {
    assert!(
        audio_cover_probe_output_has_stream(br#"{"streams":[{"index":1}]}"#)
            .expect("probe output should parse")
    );
}
