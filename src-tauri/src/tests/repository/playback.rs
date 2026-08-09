//! Playback-domain repository tests split out from the repository facade module.

use crate::services::repository::test_support::{
    create_repository_without_initial_sync, create_test_state, insert_asset_metadata_number,
    insert_virtual_asset, playback_test_lock, update_repository_backend_config,
};
use crate::services::repository::{
    set_test_downloader_playback_hook, EntryPlaybackRequest, EntryPlaybackSourceResponse,
    RepositoryState,
};
use std::fs;

fn prepare_entry_playback_source(
    state: &RepositoryState,
    request: EntryPlaybackRequest,
) -> Result<EntryPlaybackSourceResponse, String> {
    let mut ignore_progress = |_| Ok(());
    state.prepare_entry_playback_source_with_progress(request, &mut ignore_progress)
}

#[test]
fn prepare_entry_playback_source_delegates_virtual_tracks_to_downloader() {
    let _lock = playback_test_lock();
    let (state, root, repo_root, _thumbnail_root) =
        create_test_state("prepare-entry-playback-virtual");
    let repo_id = create_repository_without_initial_sync(&state, &repo_root);
    insert_virtual_asset(
        &state,
        &repo_id,
        "Created/demo-track.mp3",
        "demo-track.mp3",
        "1001",
        serde_json::json!({
            "provider": "netease-cloud-music",
            "songId": 1001,
            "accountCookie": "MUSIC_U=test-cookie",
            "accountId": "42",
            "level": "lossless"
        }),
    );
    insert_asset_metadata_number(&state, &repo_id, "Created/demo-track.mp3", "songId", "1001");

    let expected_repo_id = repo_id.clone();
    fn test_hook(payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let expected_repo_id = std::env::var("MOMOBKO_TEST_EXPECTED_REPO_ID")
            .expect("expected repo id should be provided");
        assert_eq!(payload["songId"], serde_json::json!(1001));
        assert_eq!(
            payload["accountCookie"],
            serde_json::json!("MUSIC_U=test-cookie")
        );
        assert_eq!(payload["level"], serde_json::json!("lossless"));
        assert_eq!(payload["repoId"], serde_json::json!(expected_repo_id));
        assert_eq!(
            payload["entryPath"],
            serde_json::json!("Created/demo-track.mp3")
        );
        Ok(serde_json::json!({
            "localPath": "C:/Mock/Temp/demo-track.mp3",
            "tempFilePath": "C:/Mock/Temp/demo-track.mp3",
            "lyricPath": "C:/Mock/Temp/demo-track.lrc",
            "wordLyricPath": "C:/Mock/Temp/demo-track.yrc",
            "mediaType": "audio/mpeg",
            "expiresAt": "2026-06-14T01:00:00Z",
            "sizeBytes": 4096,
            "modifiedAt": "2026-06-14T00:30:00Z"
        }))
    }

    std::env::set_var("MOMOBKO_TEST_EXPECTED_REPO_ID", &expected_repo_id);
    set_test_downloader_playback_hook(Some(test_hook));
    let response = prepare_entry_playback_source(
        &state,
        EntryPlaybackRequest {
            repo_id: repo_id.clone(),
            path: "Created/demo-track.mp3".to_string(),
        },
    )
    .expect("virtual playback source should resolve");
    set_test_downloader_playback_hook(None);
    std::env::remove_var("MOMOBKO_TEST_EXPECTED_REPO_ID");

    assert_eq!(response.repo_id, repo_id);
    assert_eq!(response.path, "Created/demo-track.mp3");
    assert_eq!(response.media_type, "audio/mpeg");
    assert_eq!(
        response.local_path.as_deref(),
        Some("C:/Mock/Temp/demo-track.mp3")
    );
    assert_eq!(
        response.temp_file_path.as_deref(),
        Some("C:/Mock/Temp/demo-track.mp3")
    );
    assert_eq!(
        response.lyric_path.as_deref(),
        Some("C:/Mock/Temp/demo-track.lrc")
    );
    assert_eq!(
        response.word_lyric_path.as_deref(),
        Some("C:/Mock/Temp/demo-track.yrc")
    );
    assert_eq!(response.expires_at.as_deref(), Some("2026-06-14T01:00:00Z"));
    assert_eq!(response.size_bytes, Some(4096));
    assert_eq!(
        response.modified_at.as_deref(),
        Some("2026-06-14T00:30:00Z")
    );

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn prepare_entry_playback_source_with_progress_emits_download_and_ready_events() {
    let _lock = playback_test_lock();
    let (state, root, repo_root, _thumbnail_root) =
        create_test_state("prepare-entry-playback-progress");
    let repo_id = create_repository_without_initial_sync(&state, &repo_root);
    insert_virtual_asset(
        &state,
        &repo_id,
        "Created/progress-track.mp3",
        "progress-track.mp3",
        "4001",
        serde_json::json!({
            "provider": "netease-cloud-music",
            "songId": 4001,
            "accountCookie": "MUSIC_U=progress-cookie",
            "accountId": "42",
            "level": "standard"
        }),
    );

    fn test_hook(payload: serde_json::Value) -> Result<serde_json::Value, String> {
        assert_eq!(payload["songId"], serde_json::json!(4001));
        Ok(serde_json::json!({
            "localPath": "C:/Mock/Temp/progress-track.mp3",
            "tempFilePath": "C:/Mock/Temp/progress-track.mp3",
            "mediaType": "audio/mpeg",
            "cached": false
        }))
    }

    set_test_downloader_playback_hook(Some(test_hook));
    let mut events = Vec::new();
    let response = state
        .prepare_entry_playback_source_with_progress(
            EntryPlaybackRequest {
                repo_id: repo_id.clone(),
                path: "Created/progress-track.mp3".to_string(),
            },
            &mut |event| {
                events.push(event);
                Ok(())
            },
        )
        .expect("virtual playback source should resolve with progress");
    set_test_downloader_playback_hook(None);

    assert_eq!(response.repo_id, repo_id);
    assert_eq!(
        events
            .iter()
            .map(|event| event.phase.as_str())
            .collect::<Vec<_>>(),
        vec!["resolve", "download", "preview", "ready"]
    );
    assert_eq!(events.last().map(|event| event.value), Some(100));
    assert_eq!(events.last().and_then(|event| event.cached), Some(false));

    fs::remove_dir_all(root).expect("test temp root should be removed");
}

#[test]
fn prepare_entry_playback_source_prefers_repository_backend_cookie_over_stale_asset_payload() {
    let _lock = playback_test_lock();
    let (state, root, repo_root, _thumbnail_root) =
        create_test_state("prepare-entry-playback-backend-cookie");
    let repo_id = create_repository_without_initial_sync(&state, &repo_root);
    update_repository_backend_config(
        &state,
        &repo_id,
        serde_json::json!({
            "cookie": "MUSIC_U=fresh-cookie"
        }),
    );
    insert_virtual_asset(
        &state,
        &repo_id,
        "Created/stale-track.mp3",
        "stale-track.mp3",
        "1002",
        serde_json::json!({
            "provider": "netease-cloud-music",
            "songId": 1002,
            "accountCookie": "MUSIC_U=stale-cookie",
            "accountId": "42",
            "level": "standard"
        }),
    );

    fn test_hook(payload: serde_json::Value) -> Result<serde_json::Value, String> {
        assert_eq!(payload["songId"], serde_json::json!(1002));
        assert_eq!(
            payload["accountCookie"],
            serde_json::json!("MUSIC_U=fresh-cookie")
        );
        Ok(serde_json::json!({
            "localPath": "C:/Mock/Temp/stale-track.mp3",
            "tempFilePath": "C:/Mock/Temp/stale-track.mp3",
            "mediaType": "audio/mpeg"
        }))
    }

    set_test_downloader_playback_hook(Some(test_hook));
    let response = prepare_entry_playback_source(
        &state,
        EntryPlaybackRequest {
            repo_id: repo_id.clone(),
            path: "Created/stale-track.mp3".to_string(),
        },
    )
    .expect("virtual playback source should resolve");
    set_test_downloader_playback_hook(None);

    assert_eq!(response.repo_id, repo_id);
    assert_eq!(
        response.local_path.as_deref(),
        Some("C:/Mock/Temp/stale-track.mp3")
    );

    fs::remove_dir_all(root).expect("test temp root should be removed");
}
