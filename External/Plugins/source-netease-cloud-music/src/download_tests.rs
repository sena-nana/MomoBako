//! 下载落盘的测试：本地目录里已有的文件不覆盖、不重下，结果报出保留了哪些；同一次下载的文件名不互相顶掉；
//! 取不齐或写不进去时不留下这一首写了一半的文件。网络部分用闭包代替。

use std::{
    cell::Cell,
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use momobako_mutsuki_plugin_sdk::PluginRuntimeContext;

use super::{claim_file_name, resolve_download_target, save_track, track_result, SavedTrack};
use crate::{
    files::WritePolicy,
    models::{DownloadDestination, RuntimeContext, SongItem},
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

/// 测试独占的临时目录，测试结束时删掉。
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "momobako-netease-{name}-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("临时目录");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn runtime(data_dir: &Path) -> RuntimeContext {
    let host = PluginRuntimeContext {
        plugin_id: "momobako.netease.source".to_string(),
        plugin_data_dir: data_dir.to_string_lossy().to_string(),
        ..PluginRuntimeContext::default()
    };
    RuntimeContext::new(host, data_dir.to_path_buf(), None)
}

/// 目录里现在有哪些文件，和它们的内容。
fn contents(dir: &Path) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = fs::read_dir(dir)
        .expect("读目录")
        .map(|entry| {
            let entry = entry.expect("目录项");
            let text = fs::read_to_string(entry.path()).unwrap_or_default();
            (entry.file_name().to_string_lossy().to_string(), text)
        })
        .collect();
    entries.sort();
    entries
}

fn lyrics(lrc: &str, yrc: &str) -> Result<(Option<String>, Option<String>), String> {
    Ok((Some(lrc.to_string()), Some(yrc.to_string())))
}

/// 下载到本地目录：同名的音频已经在就不下载、原样保留，已有的逐字歌词也保留，缺的普通歌词写下；结果分别列出写下
/// 的和保留的。目录里不多出别的文件。
#[test]
fn existing_files_in_a_local_folder_are_kept_and_not_downloaded_again() {
    let folder = TempDir::new("keep");
    let data = TempDir::new("keep-data");
    fs::write(folder.path().join("周杰伦 - 晴天.mp3"), "用户自己的音频").expect("种下音频");
    fs::write(folder.path().join("周杰伦 - 晴天.yrc"), "用户自己的逐字歌词").expect("种下逐字歌词");
    let audio_fetched = Cell::new(false);

    let saved = save_track(
        &runtime(data.path()),
        folder.path(),
        "周杰伦 - 晴天",
        WritePolicy::KeepExisting,
        || {
            audio_fetched.set(true);
            Ok("新下载的音频".as_bytes().to_vec())
        },
        || lyrics("新歌词", "新逐字歌词"),
    )
    .expect("保留已有文件不算失败");

    assert!(!audio_fetched.get(), "已有的音频不再下载");
    assert_eq!(
        contents(folder.path()),
        [
            ("周杰伦 - 晴天.lrc".to_string(), "新歌词".to_string()),
            ("周杰伦 - 晴天.mp3".into(), "用户自己的音频".into()),
            ("周杰伦 - 晴天.yrc".into(), "用户自己的逐字歌词".into()),
        ]
    );
    assert_eq!(saved.written, [folder.path().join("周杰伦 - 晴天.lrc")]);
    assert_eq!(
        saved.kept,
        [
            folder.path().join("周杰伦 - 晴天.mp3"),
            folder.path().join("周杰伦 - 晴天.yrc")
        ]
    );
}

/// 本地目录里没有同名文件：音频和歌词都写下，没有保留的。
#[test]
fn a_fresh_folder_gets_every_file() {
    let folder = TempDir::new("fresh");
    let data = TempDir::new("fresh-data");
    let saved = save_track(
        &runtime(data.path()),
        folder.path(),
        "A - B",
        WritePolicy::KeepExisting,
        || Ok(b"audio".to_vec()),
        || Ok((Some("lrc".to_string()), None)),
    )
    .expect("写下");
    assert_eq!(
        contents(folder.path()),
        [("A - B.lrc".to_string(), "lrc".to_string()), ("A - B.mp3".into(), "audio".into())]
    );
    assert_eq!(saved.written.len(), 2);
    assert!(saved.kept.is_empty());
}

/// 插件自己的暂存目录照旧替换之前暂存的同名文件，临时文件不留下。
#[test]
fn the_staging_folder_replaces_what_it_staged_before() {
    let folder = TempDir::new("replace");
    let data = TempDir::new("replace-data");
    fs::write(folder.path().join("A - B.mp3"), "上次暂存的").expect("种下暂存文件");
    let saved = save_track(
        &runtime(data.path()),
        folder.path(),
        "A - B",
        WritePolicy::Replace,
        || Ok(b"new".to_vec()),
        || Ok((None, None)),
    )
    .expect("替换");
    assert_eq!(contents(folder.path()), [("A - B.mp3".to_string(), "new".to_string())]);
    assert_eq!(
        saved,
        SavedTrack {
            written: vec![folder.path().join("A - B.mp3")],
            kept: Vec::new()
        }
    );
}

/// 音频或歌词取不到时这一首什么也不写，已有的文件原样留着。
#[test]
fn nothing_is_written_when_a_part_cannot_be_fetched() {
    let folder = TempDir::new("fetch-failed");
    let data = TempDir::new("fetch-failed-data");
    fs::write(folder.path().join("A - B.lrc"), "旧歌词").expect("种下歌词");
    let error = save_track(
        &runtime(data.path()),
        folder.path(),
        "A - B",
        WritePolicy::KeepExisting,
        || Ok(b"audio".to_vec()),
        || Err("网络超时".to_string()),
    )
    .expect_err("歌词取不到");
    assert_eq!(error, "网络超时");
    assert_eq!(contents(folder.path()), [("A - B.lrc".to_string(), "旧歌词".to_string())]);

    let error = save_track(
        &runtime(data.path()),
        folder.path(),
        "C - D",
        WritePolicy::KeepExisting,
        || Err("未获取到可下载音频地址".to_string()),
        || lyrics("lrc", "yrc"),
    )
    .expect_err("音频取不到");
    assert_eq!(error, "未获取到可下载音频地址");
    assert_eq!(contents(folder.path()), [("A - B.lrc".to_string(), "旧歌词".to_string())]);
}

/// 写到一半失败时交回错误，删掉这一首这次写下的文件；别的文件和目录不碰。这里音频和普通歌词写下以后，逐字歌词的
/// 临时文件位置被一个目录占着，写不进去。
#[test]
fn a_failed_write_removes_only_what_this_track_wrote() {
    let folder = TempDir::new("write-failed");
    let data = TempDir::new("write-failed-data");
    fs::write(folder.path().join("别的文件.txt"), "不相干").expect("种下别的文件");
    fs::create_dir(folder.path().join("A - B.yrc.download")).expect("占住临时文件的位置");
    let error = save_track(
        &runtime(data.path()),
        folder.path(),
        "A - B",
        WritePolicy::Replace,
        || Ok(b"audio".to_vec()),
        || lyrics("lrc", "yrc"),
    )
    .expect_err("逐字歌词写不进去");
    assert!(error.contains("A - B.yrc.download"), "错误里写明是哪个文件：{error}");
    assert_eq!(
        contents(folder.path()),
        [("A - B.yrc.download".to_string(), String::new()), ("别的文件.txt".into(), "不相干".into())],
        "写下的音频和普通歌词删掉了，别的文件和占位的目录还在"
    );
    assert!(folder.path().join("A - B.yrc.download").is_dir());

    let missing = folder.path().join("不存在的目录");
    let error = save_track(
        &runtime(data.path()),
        &missing,
        "C - D",
        WritePolicy::KeepExisting,
        || Ok(b"audio".to_vec()),
        || lyrics("lrc", "yrc"),
    )
    .expect_err("目录不存在写不进去");
    assert!(error.contains("C - D.mp3"), "错误里写明是哪个文件：{error}");
    assert!(!missing.exists(), "不替用户建目录");
}

/// 同一次下载里两首歌渲染出同一个文件名（只差大小写也算）时，后来的加上歌曲标识，不会把前一首当成已有文件跳过。
#[test]
fn file_names_within_one_download_do_not_collide() {
    let mut claimed = BTreeSet::new();
    assert_eq!(claim_file_name("A - B", 1, &mut claimed), "A - B");
    assert_eq!(claim_file_name("a - b", 2, &mut claimed), "a - b [2]");
    assert_eq!(claim_file_name("A - B", 3, &mut claimed), "A - B [3]");
    assert_eq!(claim_file_name("C - D", 4, &mut claimed), "C - D");
}

/// 一首歌的结果：保留的同名文件写在 `skippedEntries`，没有保留的时不写这个键。
#[test]
fn a_track_result_lists_kept_files_only_when_there_are_some() {
    let song: SongItem = serde_json::from_value(serde_json::json!({ "id": 186016, "name": "晴天" }))
        .expect("歌曲");
    let written = PathBuf::from("D:/Music/周杰伦 - 晴天.lrc");
    let kept = PathBuf::from("D:/Music/周杰伦 - 晴天.mp3");
    let result = track_result(
        &song,
        &SavedTrack {
            written: vec![written.clone()],
            kept: vec![kept.clone()],
        },
    );
    assert_eq!(result["songId"], 186016);
    assert_eq!(result["songName"], "晴天");
    assert_eq!(result["paths"], serde_json::json!([written.to_string_lossy()]));
    assert_eq!(result["skippedEntries"], serde_json::json!([kept.to_string_lossy()]));

    let clean = track_result(&song, &SavedTrack { written: vec![written], kept: Vec::new() });
    assert!(clean.get("skippedEntries").is_none());
}

/// 本地目录保留已有文件；下载到其他资源库时写进宿主给这次下载新建的暂存目录（同样不覆盖），宿主没给时放进插件
/// 自己的旧暂存目录，暂存过的同名文件可以替换。
#[test]
fn downloads_land_where_the_destination_says() {
    let data = TempDir::new("targets");
    let runtime = runtime(data.path());
    let local = DownloadDestination {
        kind: "localFolder".into(),
        path: Some(" D:/Music ".into()),
        repo_id: None,
        parent_path: None,
        staging_path: None,
    };
    let target = resolve_download_target(&runtime, &local, None).expect("本地目录");
    assert_eq!((target.root, target.policy), (PathBuf::from("D:/Music"), WritePolicy::KeepExisting));

    let staged = DownloadDestination {
        kind: "repository".into(),
        path: Some("D:/third".into()),
        repo_id: Some("third-repo".into()),
        parent_path: None,
        staging_path: Some("D:/MomoCache/netease/.momo/cache/download-staging/third-repo/run-1".into()),
    };
    let target = resolve_download_target(&runtime, &staged, Some("D:/MomoCache/netease")).expect("宿主的暂存目录");
    assert_eq!(
        (target.root, target.policy),
        (
            PathBuf::from("D:/MomoCache/netease/.momo/cache/download-staging/third-repo/run-1"),
            WritePolicy::KeepExisting
        )
    );

    let legacy = DownloadDestination { staging_path: Some("  ".into()), ..staged };
    let target = resolve_download_target(&runtime, &legacy, Some("D:/MomoCache/netease")).expect("旧暂存目录");
    assert_eq!(
        (target.root, target.policy),
        (
            PathBuf::from("D:/MomoCache/netease/.momo/cache/download-staging/third-repo"),
            WritePolicy::Replace
        )
    );

    let blank = DownloadDestination { path: Some("  ".into()), ..local };
    assert!(resolve_download_target(&runtime, &blank, None).is_err(), "本地目录没有路径时不下载");
}

fn playlist_payload(tracks: serde_json::Value, name: Option<&str>) -> crate::models::DownloadPlaylistPackagePayload {
    serde_json::from_value(serde_json::json!({
        "playlistId": 9001,
        "playlistName": name,
        "tracks": tracks,
        "destination": { "kind": "localFolder", "path": "D:/Music" }
    }))
    .expect("歌单载荷")
}

fn detail(tracks: serde_json::Value, track_ids: &[i64]) -> crate::models::PlaylistDetailItem {
    let ids: Vec<serde_json::Value> = track_ids.iter().map(|id| serde_json::json!({ "id": id })).collect();
    serde_json::from_value(serde_json::json!({
        "id": 9001, "name": "夜曲", "trackCount": track_ids.len(), "tracks": tracks, "trackIds": ids
    }))
    .expect("歌单详情")
}

/// 载荷没带曲目时曲目只能从歌单详情来：取不到就把原因交回去，整个下载报错，不再什么也不下却交回成功。
#[test]
fn a_playlist_without_tracks_fails_with_the_reason_its_detail_could_not_be_read() {
    let payload = playlist_payload(serde_json::json!([]), Some("夜曲"));
    let warned = Cell::new(false);
    let result = super::playlist_detail(
        &payload,
        || Err("网易云上没有歌单 9001，可能已被删除或设为私密".to_string()),
        |_| warned.set(true),
    );
    assert_eq!(result.expect_err("取不到曲目"), "网易云上没有歌单 9001，可能已被删除或设为私密");
    assert!(!warned.get());

    let read = super::playlist_detail(&payload, || Ok(detail(serde_json::json!([]), &[1, 2])), |_| {});
    assert_eq!(read.expect("取到详情").map(|detail| detail.track_ids.len()), Some(2));
}

/// 载荷带了曲目（Vue 列出来的）时详情只用来补歌单名：带了名字就不取；没名字又取不到时记一条警告、照常下载。
#[test]
fn a_playlist_with_tracks_only_needs_its_detail_for_the_name() {
    let tracks = serde_json::json!([{ "songId": 1, "songName": "晴天" }]);
    let named = playlist_payload(tracks.clone(), Some("夜曲"));
    let fetched = Cell::new(false);
    let result = super::playlist_detail(
        &named,
        || {
            fetched.set(true);
            Err("不该取".to_string())
        },
        |_| {},
    );
    assert!(result.expect("不取详情").is_none());
    assert!(!fetched.get(), "带了曲目和名字就不取详情");

    let unnamed = playlist_payload(tracks, None);
    let warning = std::cell::RefCell::new(String::new());
    let result = super::playlist_detail(
        &unnamed,
        || Err("网易云接口请求超时（>30 秒），请检查网络后重试".to_string()),
        |error| *warning.borrow_mut() = error.to_string(),
    );
    assert!(result.expect("照常下载").is_none());
    assert_eq!(warning.into_inner(), "网易云接口请求超时（>30 秒），请检查网络后重试");
}

/// 每首歌的详情先用歌单详情里带的，没带到的一次批量取；批量取不到时记一条警告，留给逐首下载时再取。
#[test]
fn song_details_come_from_the_playlist_first_then_one_batch() {
    let tracks: Vec<crate::models::DownloadPlaylistTrackPayload> = serde_json::from_value(serde_json::json!([
        { "songId": 1 }, { "songId": 2 }, { "songId": 3 }
    ]))
    .expect("曲目");
    let detail = detail(serde_json::json!([{ "id": 1, "name": "晴天" }, { "id": 99, "name": "不在下载里" }]), &[1, 2, 3]);
    let asked = std::cell::RefCell::new(Vec::new());
    let songs = super::known_songs(
        Some(&detail),
        &tracks,
        |ids| {
            asked.borrow_mut().push(ids.to_vec());
            Ok(vec![serde_json::from_value(serde_json::json!({ "id": 2, "name": "七里香" })).expect("歌曲")])
        },
        |_| panic!("批量取到了"),
    );
    assert_eq!(asked.into_inner(), [vec![2, 3]], "只批量取一次，只取歌单详情里没带的");
    let mut names: Vec<(i64, String)> = songs.into_iter().map(|(id, song)| (id, song.name)).collect();
    names.sort();
    assert_eq!(names, [(1, "晴天".to_string()), (2, "七里香".into())]);

    let warned = Cell::new(false);
    let songs = super::known_songs(Some(&detail), &tracks, |_| Err("限流".to_string()), |_| warned.set(true));
    assert!(warned.get());
    assert_eq!(songs.keys().copied().collect::<Vec<_>>(), [1], "取不到的留给逐首下载");
}

/// 失败的一首写上歌名：先用载荷带来的，再用取到的歌曲详情里的。
#[test]
fn a_failed_track_names_the_song_when_it_can() {
    let bare: crate::models::DownloadPlaylistTrackPayload =
        serde_json::from_value(serde_json::json!({ "songId": 7 })).expect("曲目");
    let entry = super::failed_track(&bare, Some("七里香".into()), "未获取到可下载音频地址");
    assert_eq!(entry, serde_json::json!({ "songId": 7, "songName": "七里香", "error": "未获取到可下载音频地址" }));
    let named: crate::models::DownloadPlaylistTrackPayload =
        serde_json::from_value(serde_json::json!({ "songId": 7, "songName": "载荷里的名字" })).expect("曲目");
    assert_eq!(super::failed_track(&named, Some("七里香".into()), "x")["songName"], "载荷里的名字");
    assert!(super::failed_track(&bare, None, "x")["songName"].is_null());
}
