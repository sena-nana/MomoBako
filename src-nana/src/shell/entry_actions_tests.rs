//! 来源条目动作的声明匹配、载荷和禁用，对照 Vue `sourceEntryActions.ts`：清单声明换成菜单项，载荷照 Vue 带上
//! `managedCacheRoot`，下载照 `downloadTargets` 分到本地和到其他资源库，来源认证或本地缓存不可用时禁用。

use std::collections::BTreeMap;

use super::*;

fn row() -> FileRow {
    FileRow::from_entry(&crate::backend::services::repository::FileBrowserEntry {
        path: "song".into(),
        name: "song".into(),
        kind: "file".into(),
        extension: Some("mp3".into()),
        size_bytes: None,
        size_label: None,
        modified_at: None,
        asset_id: None,
        status: None,
        thumbnail_path: None,
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: None,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata: BTreeMap::new(),
        is_virtual: false,
        provider_id: Some("netease".into()),
        provider_item_id: Some("9".into()),
        source_payload: Some(serde_json::json!({"entryKind": "track", "songId": 9, "cookie": "hide"})),
        local_absolute_path: None,
    })
}

fn directory() -> FileRow {
    let mut directory = row();
    directory.kind = "directory".into();
    directory.name = "夜曲合集".into();
    directory.path = "playlists/night".into();
    directory.source_payload = Some(serde_json::json!({"entryKind": "playlist-folder", "playlistId": 3, "playlistName": "夜曲"}));
    directory
}

/// 当前资源库可用、缓存在 D:/cache、还有别的资源库可下载。
fn ready() -> SourceContext {
    SourceContext { managed_cache_root: Some("D:/cache".into()), source_unavailable: false, has_target_repository: true }
}

fn manifest() -> PluginManifest {
    PluginManifest {
        plugin_id: "momobako.netease.source".into(),
        package_format_version: None,
        package_hash: None,
        provenance: None,
        trust_level: None,
        deployment: None,
        target_triple: None,
        legacy_plugin_ids: Vec::new(),
        name: "网易云".into(),
        version: "0.2.0".into(),
        r#type: None,
        kind: "netease-cloud-music".into(),
        category: "source".into(),
        description: String::new(),
        capabilities: Vec::new(),
        enabled: true,
        sdk: "backend".into(),
        entry: Value::Null,
        contributes: serde_json::json!({
            "source": {
                "media": { "clearCacheMethod": "media.clearTrackCache" },
                "entryActions": [
                    { "actionId": "clear-track-cache", "label": "清理播放缓存", "scope": "file", "entryKind": "track", "operation": "clear-cache", "method": "media.clearTrackCache" },
                    { "actionId": "refresh-track-playback", "label": "重新获取播放资源", "scope": "file", "entryKind": "track", "operation": "refresh-playback", "method": "media.prepareTrackPlayback" },
                    { "actionId": "download-track", "label": "下载单曲", "scope": "file", "entryKind": "track", "operation": "download-entry", "method": "media.downloadTrackPackage" },
                    { "actionId": "download-playlist", "label": "下载歌单", "scope": "directory", "entryKind": "playlist-folder", "operation": "download-directory", "method": "media.downloadPlaylistPackage", "targets": ["writable-repository"] },
                    { "actionId": "create-audio-playlist", "label": "创建播放列表", "scope": "directory", "entryKind": "playlist-folder", "operation": "playlist-from-directory" }
                ]
            }
        }),
        source: "builtin".into(),
        runtime: "native-dylib".into(),
        permissions: Vec::new(),
        requires: Vec::new(),
        optional: Vec::new(),
        hooks: Vec::new(),
        compat: Default::default(),
        status: "ready".into(),
        dependency_status: Default::default(),
        disable_reason: None,
        degraded: false,
        degradation_reason: None,
        archive_path: None,
    }
}

fn labels(actions: &[FilePluginAction]) -> Vec<&str> {
    actions.iter().map(|action| action.label.as_str()).collect()
}

/// 单曲：清理缓存和重新获取播放资源直接调用插件，载荷照 Vue 带上本地缓存目录（`managedCacheRoot`），网易云插件
/// 据此清到、取到真正的播放缓存；不公开 cookie。下载没声明去处，分「到本地」「到其他资源库」两项，都先问去处。
#[test]
fn track_actions_carry_the_managed_cache_root_and_split_downloads_by_target() {
    let actions = actions_for(&[manifest()], &row(), &ready());
    assert_eq!(labels(&actions), ["清理播放缓存", "重新获取播放资源", "下载单曲到本地", "下载单曲到其他资源库"]);
    for action in &actions[..2] {
        let FilePluginDispatch::Call { payload, .. } = &action.dispatch else { panic!("不是直接调用：{:?}", action.dispatch) };
        assert_eq!(payload["managedCacheRoot"], "D:/cache", "{}", action.label);
        assert_eq!(payload["songId"], 9);
        assert!(payload["sourcePayload"]["cookie"].is_null(), "不公开 cookie");
    }
    let FilePluginDispatch::Call { method, payload, .. } = &actions[1].dispatch else { panic!("不是直接调用") };
    assert_eq!(method, "media.prepareTrackPlayback");
    assert_eq!(payload["forceRefresh"], true);
    match &actions[2].dispatch {
        FilePluginDispatch::AskFolder { method, payload, progress, .. } => {
            assert_eq!(method, "media.downloadTrackPackage");
            assert_eq!(payload["managedCacheRoot"], "D:/cache");
            assert_eq!(payload["playlistName"], "song", "没有歌单名时用条目名");
            assert!(payload.get("destination").is_none(), "去处选好以后再写");
            assert_eq!(progress.preparing, "准备下载 song");
        }
        other => panic!("到本地不是先问目录：{other:?}"),
    }
    match &actions[3].dispatch {
        FilePluginDispatch::AskRepository { parent_path, payload, .. } => {
            assert_eq!(parent_path, &None, "单曲导入到目标资源库的根目录");
            assert_eq!(payload["managedCacheRoot"], "D:/cache");
        }
        other => panic!("到其他资源库不是先选资源库：{other:?}"),
    }
    assert_ne!(actions[2].value, actions[3].value, "两项的值不同");

    let no_cache = SourceContext { managed_cache_root: None, ..ready() };
    let actions = actions_for(&[manifest()], &row(), &no_cache);
    let FilePluginDispatch::Call { payload, .. } = &actions[0].dispatch else { panic!("不是直接调用") };
    assert!(payload["managedCacheRoot"].is_null(), "没有缓存目录时照 Vue 写 null");
}

/// 歌单目录：声明了只到其他资源库，只出一项、不加去处后缀；下载的文件导入到以歌单命名的文件夹。
#[test]
fn a_declared_target_is_the_only_download_entry() {
    let actions = actions_for(&[manifest()], &directory(), &ready());
    assert_eq!(labels(&actions), ["下载歌单", "创建播放列表"]);
    match &actions[0].dispatch {
        FilePluginDispatch::AskRepository { method, payload, progress, parent_path, .. } => {
            assert_eq!(method, "media.downloadPlaylistPackage");
            assert_eq!(payload["playlistId"], 3);
            assert_eq!(payload["playlistName"], "夜曲");
            assert_eq!(progress.preparing, "准备下载目录");
            assert_eq!(parent_path.as_deref(), Some("夜曲"), "导入到以歌单命名的文件夹");
        }
        other => panic!("不是先选资源库：{other:?}"),
    }
}

/// 来源认证或本地缓存不可用（登录过期、没有登录、缓存没就绪，或者条目自己标了登录过期）：照 Vue `actionDisabled`
/// 全部禁用，原因是「来源认证或本地缓存不可用」；没有能下载到的其他资源库时只有「到其他资源库」禁用。
#[test]
fn unavailable_sources_disable_every_action_with_the_reason() {
    let unavailable = SourceContext { source_unavailable: true, ..ready() };
    let actions = actions_for(&[manifest()], &row(), &unavailable);
    assert_eq!(actions.len(), 4);
    assert!(actions.iter().all(|action| action.unavailable == Some(SOURCE_UNAVAILABLE)), "{actions:?}");
    let actions = actions_for(&[manifest()], &directory(), &unavailable);
    assert!(actions.iter().all(|action| action.unavailable == Some(SOURCE_UNAVAILABLE)), "创建播放列表也禁用");

    let mut expired = row();
    expired.source_payload = Some(serde_json::json!({"entryKind": "track", "songId": 9, "loginExpired": true}));
    let actions = actions_for(&[manifest()], &expired, &ready());
    assert!(actions.iter().all(|action| action.unavailable == Some(SOURCE_UNAVAILABLE)), "条目自己标了登录过期");
    let mut flagged = row();
    flagged.metadata.insert("loginExpired".into(), Value::Bool(true));
    assert!(actions_for(&[manifest()], &flagged, &ready()).iter().all(|action| action.unavailable.is_some()), "元数据里标了也算");

    let alone = SourceContext { has_target_repository: false, ..ready() };
    let actions = actions_for(&[manifest()], &row(), &alone);
    let reasons: Vec<_> = actions.iter().map(|action| action.unavailable).collect();
    assert_eq!(reasons, [None, None, None, Some(NO_TARGET_REPOSITORY)], "只有到其他资源库禁用");
}

/// 「创建播放列表」照 Vue `runPlaylistAction` 在本地建播放集：标识是「来源-歌单」，名称取来源的歌单名，
/// 播放器类型取动作声明的；没有声明时是音频顺序播放，没有歌单名时用目录名。不再调用插件。
#[test]
fn playlist_from_directory_builds_a_local_playlist() {
    let mut directory = directory();
    let actions = actions_for(&[manifest()], &directory, &ready());
    assert_eq!(
        actions[1].dispatch,
        FilePluginDispatch::CreatePlaylist(SourcePlaylist {
            playlist_id: Some("netease-3".into()),
            name: "夜曲".into(),
            player_type_id: AUDIO_SEQUENCE_TYPE.into(),
            path: "playlists/night".into(),
        })
    );

    directory.source_payload = Some(serde_json::json!({"entryKind": "playlist-folder"}));
    directory.provider_id = None;
    let actions = actions_for(&[manifest()], &directory, &ready());
    let FilePluginDispatch::CreatePlaylist(playlist) = &actions[1].dispatch else { panic!("不是本地建播放集") };
    assert_eq!(playlist.playlist_id.as_deref(), Some("source-9"), "没有歌单标识时用来源条目标识，来源缺省是 source");
    assert_eq!(playlist.name, "夜曲合集", "没有歌单名时用目录名");

    directory.provider_item_id = None;
    let actions = actions_for(&[manifest()], &directory, &ready());
    let FilePluginDispatch::CreatePlaylist(playlist) = &actions[1].dispatch else { panic!("不是本地建播放集") };
    assert_eq!(playlist.playlist_id, None, "两样标识都没有：点了只报错");
}

/// 插件 SDK 只约定了五种 operation 和两种下载去处。约定以外的 operation 不出菜单，声明了 method 也不调用（Vue
/// 一律当成「创建播放列表」，拿单曲也会建播放集，是缺陷）；没写 scope 的声明也不出（Vue 要求 scope 和条目类型
/// 相同）；约定以外的下载去处不出那一项。
#[test]
fn declarations_outside_the_sdk_contract_are_not_offered() {
    let mut plugin = manifest();
    plugin.contributes = serde_json::json!({
        "source": {
            "media": { "clearCacheMethod": "media.clearTrackCache" },
            "entryActions": [
                { "actionId": "rename-remote", "label": "改远端名", "scope": "file", "entryKind": "track", "operation": "rename-remote", "method": "media.renameTrack" },
                { "actionId": "mystery", "label": "神秘动作", "scope": "file", "entryKind": "track", "operation": "mystery" },
                { "actionId": "no-scope", "label": "清理播放缓存", "entryKind": "track", "operation": "clear-cache" },
                { "actionId": "clear", "label": "清理播放缓存", "scope": "file", "entryKind": "track", "operation": "clear-cache" },
                { "actionId": "download", "label": "下载", "scope": "file", "entryKind": "track", "operation": "download-entry", "method": "media.download", "targets": ["cloud", "local-directory"] }
            ]
        }
    });
    let actions = actions_for(&[plugin], &row(), &ready());
    let values: Vec<&str> = actions.iter().map(|action| action.value.as_str()).collect();
    assert_eq!(values, ["plugin/momobako.netease.source/clear", "plugin/momobako.netease.source/download"], "只剩约定里、写了 scope 的");
    assert_eq!(actions[1].label, "下载", "剩一个去处时不加后缀");
    assert!(matches!(actions[1].dispatch, FilePluginDispatch::AskFolder { .. }));
}
