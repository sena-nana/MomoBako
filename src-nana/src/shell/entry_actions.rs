//! 文件右键里的来源插件动作。
//!
//! 能直接调用的方法接到现有 `call_plugin`。下载先要系统目录对话框，
//! 创建来源播放列表先要名称；确认后才带上目录或名称调用。

use serde_json::{Map, Value};

use crate::backend::services::repository::PluginManifest;

use super::files::FileRow;

/// 右键可以画出的一条插件动作。
#[derive(Clone, Debug, PartialEq)]
pub struct FilePluginAction {
    pub value: String,
    pub label: String,
    pub dispatch: FilePluginDispatch,
}

/// 立刻调用插件，或先问目录、名称再调用。
#[derive(Clone, Debug, PartialEq)]
pub enum FilePluginDispatch {
    Call { plugin_id: String, method: String, payload: Value },
    /// 选中文件夹后把 `destination` 写进载荷再调用。
    AskFolder { plugin_id: String, method: String, payload: Value },
    /// 名称非空后把名称和当前仓库写进载荷再调用。
    AskName { plugin_id: String, method: String, payload: Value },
}

/// 按清单 `entryActions` 和当前行的类型、来源载荷匹配菜单项。
pub fn actions_for(plugins: &[PluginManifest], row: &FileRow) -> Vec<FilePluginAction> {
    let mut actions = Vec::new();
    for plugin in plugins {
        if !plugin.enabled || matches!(plugin.status.as_str(), "disabled" | "unavailable" | "error") {
            continue;
        }
        let Some(list) = plugin.contributes.get("source").and_then(|source| source.get("entryActions")).and_then(|value| value.as_array()) else {
            continue;
        };
        for action in list {
            if !matches_row(action, row) {
                continue;
            }
            let Some(prepared) = prepare(plugin, action, row) else {
                eprintln!("Nana 文件插件动作没有可执行入口：{}", plugin.plugin_id);
                continue;
            };
            actions.push(prepared);
        }
    }
    actions
}

fn prepare(plugin: &PluginManifest, action: &Value, row: &FileRow) -> Option<FilePluginAction> {
    let action_id = text(action, "actionId").unwrap_or_else(|| "action".into());
    let label = text(action, "label").unwrap_or_else(|| action_id.clone());
    let operation = text(action, "operation").unwrap_or_default();
    let value = format!("plugin/{}/{}", plugin.plugin_id, action_id);
    let method = action_method(plugin, action, &operation);
    let dispatch = match operation.as_str() {
        "clear-cache" | "refresh-playback" => {
            let method = method?;
            FilePluginDispatch::Call { plugin_id: plugin.plugin_id.clone(), method, payload: call_payload(action, row, &operation) }
        }
        "download-entry" | "download-directory" => {
            let method = method?;
            FilePluginDispatch::AskFolder {
                plugin_id: plugin.plugin_id.clone(),
                method,
                payload: call_payload(action, row, &operation),
            }
        }
        "playlist-from-directory" => {
            let method = method.unwrap_or_else(|| "source.playlistFromDirectory".into());
            let mut payload = call_payload(action, row, &operation);
            if let Some(player_type) = text(action, "playerTypeId") {
                if let Some(object) = payload.as_object_mut() {
                    object.insert("playerTypeId".into(), Value::String(player_type));
                }
            }
            FilePluginDispatch::AskName { plugin_id: plugin.plugin_id.clone(), method, payload }
        }
        _ if method.is_some() => FilePluginDispatch::Call {
            plugin_id: plugin.plugin_id.clone(),
            method: method.unwrap_or_default(),
            payload: call_payload(action, row, &operation),
        },
        _ => return None,
    };
    Some(FilePluginAction { value, label, dispatch })
}

fn action_method(plugin: &PluginManifest, action: &Value, operation: &str) -> Option<String> {
    if let Some(method) = text(action, "method") {
        return Some(method);
    }
    let key = match operation {
        "download-entry" => "downloadEntryMethod",
        "download-directory" => "downloadDirectoryMethod",
        "refresh-playback" => "preparePlaybackMethod",
        "clear-cache" => "clearCacheMethod",
        _ => return None,
    };
    plugin
        .contributes
        .get("source")
        .and_then(|source| source.get("media"))
        .and_then(|media| media.get(key))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn matches_row(action: &Value, row: &FileRow) -> bool {
    let scope = action.get("scope").and_then(|value| value.as_str()).unwrap_or("");
    if !scope.is_empty() && scope != row.kind {
        return false;
    }
    if let Some(provider) = text(action, "providerId") {
        if row.provider_id.as_deref() != Some(provider.as_str()) {
            return false;
        }
    }
    let Some(entry_kind) = text(action, "entryKind") else {
        return true;
    };
    let actual = row.source_payload.as_ref().and_then(|payload| payload.get("entryKind")).and_then(|value| value.as_str());
    if actual == Some(entry_kind.as_str()) {
        return true;
    }
    match entry_kind.as_str() {
        "track" => row.kind == "file" && source_number(row, "songId").is_some(),
        "playlist-folder" => row.kind == "directory" && source_number(row, "playlistId").is_some(),
        _ => false,
    }
}

fn call_payload(action: &Value, row: &FileRow, operation: &str) -> Value {
    let mut payload = Map::new();
    if let Some(song_id) = source_number(row, "songId") {
        payload.insert("songId".into(), Value::from(song_id));
    }
    if let Some(playlist_id) = source_number(row, "playlistId") {
        payload.insert("playlistId".into(), Value::from(playlist_id));
    }
    payload.insert("level".into(), Value::String(source_string(row, "level").unwrap_or_else(|| "standard".into())));
    if operation == "refresh-playback" {
        payload.insert("forceRefresh".into(), Value::Bool(true));
    }
    payload.insert("sourcePayload".into(), public_payload(row.source_payload.as_ref()));
    let _ = action;
    Value::Object(payload)
}

fn public_payload(payload: Option<&Value>) -> Value {
    let Some(object) = payload.and_then(|value| value.as_object()) else {
        return Value::Object(Map::new());
    };
    let mut public = Map::new();
    for (key, value) in object {
        let normalized = key.to_ascii_lowercase();
        if normalized.contains("cookie") || normalized.contains("password") || normalized.contains("secret") || normalized == "token" {
            continue;
        }
        public.insert(key.clone(), value.clone());
    }
    Value::Object(public)
}

fn source_number(row: &FileRow, key: &str) -> Option<i64> {
    row.source_payload
        .as_ref()
        .and_then(|payload| payload.get(key))
        .and_then(number_of)
        .or_else(|| row.metadata.get(key).and_then(number_of))
}

fn number_of(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

fn source_string(row: &FileRow, key: &str) -> Option<String> {
    let value = row.source_payload.as_ref().and_then(|payload| payload.get(key)).or_else(|| row.metadata.get(key))?;
    value.as_str().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string)
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(|item| item.as_str()).map(str::trim).filter(|item| !item.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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
                        { "actionId": "download-track", "label": "下载单曲", "scope": "file", "entryKind": "track", "operation": "download-entry", "method": "media.downloadTrackPackage" },
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

    #[test]
    fn clear_cache_calls_plugin_and_download_asks_for_a_folder() {
        let actions = actions_for(&[manifest()], &row());
        assert_eq!(actions.len(), 2);
        match &actions[0].dispatch {
            FilePluginDispatch::Call { method, payload, .. } => {
                assert_eq!(method, "media.clearTrackCache");
                assert_eq!(payload["songId"], 9);
                assert!(payload["sourcePayload"]["cookie"].is_null());
            }
            other => panic!("unexpected {other:?}"),
        }
        match &actions[1].dispatch {
            FilePluginDispatch::AskFolder { method, payload, .. } => {
                assert_eq!(method, "media.downloadTrackPackage");
                assert_eq!(payload["songId"], 9);
                assert!(payload.get("destination").is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn playlist_from_directory_asks_for_a_name() {
        let mut directory = row();
        directory.kind = "directory".into();
        directory.source_payload = Some(serde_json::json!({"entryKind": "playlist-folder", "playlistId": 3}));
        let actions = actions_for(&[manifest()], &directory);
        assert_eq!(actions.len(), 1);
        match &actions[0].dispatch {
            FilePluginDispatch::AskName { method, payload, .. } => {
                assert_eq!(method, "source.playlistFromDirectory");
                assert_eq!(payload["playlistId"], 3);
                assert!(payload.get("name").is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
