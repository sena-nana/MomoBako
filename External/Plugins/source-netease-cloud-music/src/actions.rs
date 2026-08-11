//! Source 对宿主声明的固定动作集合。

/// 返回可由宿主安全编排的动作；不允许插件注入脚本或任意命令。
pub(crate) fn describe() -> serde_json::Value {
    serde_json::json!([
        {
            "actionId": "download-track",
            "label": "下载单曲",
            "scope": "file",
            "entryKind": "track",
            "operation": "download-entry",
            "method": "media.downloadTrackPackage",
            "targets": ["local-directory", "writable-repository"]
        },
        {
            "actionId": "refresh-track-playback",
            "label": "重新获取播放资源",
            "scope": "file",
            "entryKind": "track",
            "operation": "refresh-playback",
            "method": "media.prepareTrackPlayback"
        },
        {
            "actionId": "clear-track-cache",
            "label": "清理播放缓存",
            "scope": "file",
            "entryKind": "track",
            "operation": "clear-cache",
            "method": "media.clearTrackCache"
        },
        {
            "actionId": "download-playlist",
            "label": "下载歌单",
            "scope": "directory",
            "entryKind": "playlist-folder",
            "operation": "download-directory",
            "method": "media.downloadPlaylistPackage",
            "targets": ["local-directory", "writable-repository"]
        },
        {
            "actionId": "create-audio-playlist",
            "label": "创建播放列表",
            "scope": "directory",
            "entryKind": "playlist-folder",
            "operation": "playlist-from-directory",
            "playerTypeId": "momobako.playlist.audio-sequence"
        }
    ])
}
