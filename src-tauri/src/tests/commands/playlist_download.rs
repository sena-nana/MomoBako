use crate::{execute_playlist_download_with_progress, services::repository as repository_service};
use std::sync::{Mutex, OnceLock};

static TRACK_PACKAGE_CALLS: OnceLock<Mutex<Vec<serde_json::Value>>> = OnceLock::new();

fn record_track_package_call(payload: serde_json::Value) -> Result<serde_json::Value, String> {
    TRACK_PACKAGE_CALLS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("track package call log lock should succeed")
        .push(payload.clone());
    let song_id = payload
        .get("songId")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    if song_id == 2002 {
        return Err("mock download failed".to_string());
    }
    Ok(serde_json::json!({
        "songId": song_id,
        "paths": [format!("C:/Mock/{song_id}.mp3")]
    }))
}

#[test]
fn execute_playlist_download_with_progress_reports_events_and_partial_failures() {
    crate::services::repository::set_test_downloader_track_package_hook(Some(
        record_track_package_call,
    ));
    TRACK_PACKAGE_CALLS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("track package call log lock should succeed")
        .clear();

    let request = repository_service::DownloaderPlaylistRequest {
        playlist_id: 9001,
        playlist_name: Some("夜跑歌单".to_string()),
        tracks: vec![
            repository_service::DownloaderPlaylistTrackRequest {
                song_id: 2001,
                song_name: Some("稻香".to_string()),
                source_payload: Some(serde_json::json!({
                    "songId": 2001,
                    "songName": "稻香",
                    "accountId": "123456"
                })),
            },
            repository_service::DownloaderPlaylistTrackRequest {
                song_id: 2002,
                song_name: Some("孤勇者".to_string()),
                source_payload: None,
            },
        ],
        destination: repository_service::DownloaderDestinationRequest {
            kind: "localFolder".to_string(),
            path: Some("C:/Mock/Playlist".to_string()),
            repo_id: None,
            parent_path: None,
        },
        source_payload: Some(serde_json::json!({
            "playlistId": 9001,
            "accountId": "123456"
        })),
        managed_cache_root: Some("C:/Mock/NeteaseCache".to_string()),
        level: Some("lossless".to_string()),
    };

    let mut events = Vec::new();
    let response = execute_playlist_download_with_progress(
        std::path::Path::new("C:/Mock/.service-data"),
        request,
        &mut |event| {
            events.push(event);
            Ok(())
        },
    )
    .expect("playlist download should produce a partial-success response");

    crate::services::repository::set_test_downloader_track_package_hook(None);

    let calls = TRACK_PACKAGE_CALLS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("track package call log lock should succeed")
        .clone();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0]["songId"], serde_json::json!(2001));
    assert_eq!(calls[0]["level"], serde_json::json!("lossless"));
    assert_eq!(
        calls[0]["managedCacheRoot"],
        serde_json::json!("C:/Mock/NeteaseCache")
    );
    assert_eq!(
        calls[0]["sourcePayload"]["songName"],
        serde_json::json!("稻香")
    );
    assert_eq!(calls[1]["songId"], serde_json::json!(2002));
    assert_eq!(calls[1]["level"], serde_json::json!("lossless"));
    assert_eq!(
        calls[1]["sourcePayload"]["playlistId"],
        serde_json::json!(9001)
    );
    assert_eq!(
        calls[1]["destination"]["path"],
        serde_json::json!("C:/Mock/Playlist")
    );

    assert_eq!(events.len(), 4);
    assert_eq!(events[0].phase, "start");
    assert_eq!(events[0].total, 2);
    assert_eq!(events[1].phase, "track");
    assert_eq!(events[1].current_song_id, Some(2001));
    assert_eq!(events[1].current_song_name.as_deref(), Some("稻香"));
    assert_eq!(events[1].completed, 1);
    assert_eq!(events[1].failed, 0);
    assert_eq!(events[2].phase, "track");
    assert_eq!(events[2].current_song_id, Some(2002));
    assert_eq!(events[2].current_song_name.as_deref(), Some("孤勇者"));
    assert_eq!(events[2].completed, 1);
    assert_eq!(events[2].failed, 1);
    assert_eq!(events[2].error.as_deref(), Some("mock download failed"));
    assert_eq!(events[3].phase, "complete");
    assert_eq!(events[3].completed, 1);
    assert_eq!(events[3].failed, 1);

    assert_eq!(response["playlistId"], serde_json::json!(9001));
    assert_eq!(response["summary"]["total"], serde_json::json!(2));
    assert_eq!(response["summary"]["succeeded"], serde_json::json!(1));
    assert_eq!(response["summary"]["failed"], serde_json::json!(1));
    assert_eq!(response["completed"].as_array().map(Vec::len), Some(1));
    assert_eq!(response["failed"].as_array().map(Vec::len), Some(1));
}
