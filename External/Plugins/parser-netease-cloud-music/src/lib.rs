//! 网易云来源元数据解析插件，仅消费宿主白名单投影后的公开字段。

use std::collections::BTreeMap;

use momobako_mutsuki_plugin_sdk::{export_mutsuki_momobako_plugin, PluginCallEnvelope};
use serde::Deserialize;
use serde_json::Value;

const NETEASE_PROVIDER_ID: &str = "netease-cloud-music";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefaultsPayload {
    entries: Vec<MetadataDefaultEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetadataDefaultEntry {
    path: String,
    kind: String,
    #[serde(default)]
    provider_id: Option<String>,
    #[serde(default)]
    provider_item_id: Option<String>,
    #[serde(default)]
    source_metadata: BTreeMap<String, Value>,
}

export_mutsuki_momobako_plugin!(
    "momobako.netease.parser",
    "0.1.0",
    protocols = ["metadata.defaults.batch"],
    requires = ["momobako.netease.source"],
    permissions = ["readMetadata", "writeCandidates"],
    handle_call
);

/// 路由宿主批量默认元数据请求，错误交由宿主统一记录插件与方法上下文。
fn handle_call(request: PluginCallEnvelope) -> Result<Value, String> {
    match request.method.as_str() {
        "metadata.defaults.batch" => {
            let payload: DefaultsPayload = serde_json::from_value(request.payload)
                .map_err(|error| format!("invalid metadata.defaults.batch payload: {error}"))?;
            Ok(serde_json::json!({
                "defaultsByPath": metadata_defaults(payload.entries)
            }))
        }
        method => Err(format!("unsupported method: {method}")),
    }
}

/// 只处理网易云文件条目；目录和其他来源不会生成候选值。
fn metadata_defaults(
    entries: Vec<MetadataDefaultEntry>,
) -> BTreeMap<String, BTreeMap<String, Value>> {
    entries
        .into_iter()
        .filter(|entry| entry.kind == "file")
        .filter(|entry| entry.provider_id.as_deref() == Some(NETEASE_PROVIDER_ID))
        .filter_map(|entry| {
            let defaults = default_metadata(&entry);
            (!defaults.is_empty()).then_some((entry.path, defaults))
        })
        .collect()
}

/// 将网易云字段映射到通用音频字段，同时保留稳定的来源标识与歌单分类。
fn default_metadata(entry: &MetadataDefaultEntry) -> BTreeMap<String, Value> {
    let source = &entry.source_metadata;
    let mut defaults = BTreeMap::from([("libraryKind".to_string(), Value::String("audio".into()))]);

    copy_non_empty_string(source, "songName", &mut defaults, "title");
    copy_non_empty_string(source, "albumName", &mut defaults, "album");
    copy_non_empty_string(source, "coverUrl", &mut defaults, "coverArt");
    copy_non_empty_string(source, "playlistName", &mut defaults, "playlistName");
    copy_non_empty_string(
        source,
        "playlistCategory",
        &mut defaults,
        "playlistCategory",
    );
    copy_number_or_string(source, "songId", &mut defaults);
    copy_number_or_string(source, "playlistId", &mut defaults);

    if let Some(duration_ms) = source.get("durationMs").and_then(Value::as_u64) {
        defaults.insert("durationMs".to_string(), serde_json::json!(duration_ms));
    }
    if let Some(artist) = artist_label(source.get("artists")) {
        defaults.insert("artist".to_string(), Value::String(artist));
    }
    if !defaults.contains_key("songId") {
        if let Some(provider_item_id) = entry
            .provider_item_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            defaults.insert(
                "songId".to_string(),
                provider_item_id
                    .parse::<u64>()
                    .map(Value::from)
                    .unwrap_or_else(|_| Value::String(provider_item_id.to_string())),
            );
        }
    }
    defaults
}

fn copy_non_empty_string(
    source: &BTreeMap<String, Value>,
    source_key: &str,
    target: &mut BTreeMap<String, Value>,
    target_key: &str,
) {
    if let Some(value) = source
        .get(source_key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        target.insert(target_key.to_string(), Value::String(value.to_string()));
    }
}

fn copy_number_or_string(
    source: &BTreeMap<String, Value>,
    key: &str,
    target: &mut BTreeMap<String, Value>,
) {
    if let Some(value) = source
        .get(key)
        .filter(|value| value.is_number() || value.is_string())
    {
        target.insert(key.to_string(), value.clone());
    }
}

fn artist_label(value: Option<&Value>) -> Option<String> {
    let artists = value?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    (!artists.is_empty()).then(|| artists.join("，"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn netease_entry(source_metadata: Value) -> MetadataDefaultEntry {
        MetadataDefaultEntry {
            path: "创建的歌单/示例/歌手 - 歌曲.mp3".to_string(),
            kind: "file".to_string(),
            provider_id: Some(NETEASE_PROVIDER_ID.to_string()),
            provider_item_id: Some("1001".to_string()),
            source_metadata: serde_json::from_value(source_metadata)
                .expect("test source metadata should decode"),
        }
    }

    #[test]
    fn maps_public_source_fields_to_audio_metadata() {
        let defaults = default_metadata(&netease_entry(serde_json::json!({
            "songId": 1001,
            "songName": "歌曲",
            "artists": ["歌手甲", "歌手乙"],
            "albumName": "专辑",
            "coverUrl": "https://example.invalid/cover.jpg",
            "durationMs": 180000,
            "playlistId": 2001,
            "playlistName": "示例歌单",
            "playlistCategory": "created"
        })));

        assert_eq!(
            defaults.get("libraryKind"),
            Some(&serde_json::json!("audio"))
        );
        assert_eq!(defaults.get("title"), Some(&serde_json::json!("歌曲")));
        assert_eq!(
            defaults.get("artist"),
            Some(&serde_json::json!("歌手甲，歌手乙"))
        );
        assert_eq!(defaults.get("album"), Some(&serde_json::json!("专辑")));
        assert_eq!(defaults.get("durationMs"), Some(&serde_json::json!(180000)));
        assert_eq!(defaults.get("songId"), Some(&serde_json::json!(1001)));
        assert_eq!(defaults.get("playlistId"), Some(&serde_json::json!(2001)));
        assert_eq!(
            defaults.get("playlistCategory"),
            Some(&serde_json::json!("created"))
        );
    }

    #[test]
    fn ignores_other_providers_and_directories() {
        let other = MetadataDefaultEntry {
            provider_id: Some("other-source".to_string()),
            ..netease_entry(serde_json::json!({ "songName": "不应处理" }))
        };
        let directory = MetadataDefaultEntry {
            kind: "directory".to_string(),
            ..netease_entry(serde_json::json!({ "songName": "不应处理" }))
        };

        assert!(metadata_defaults(vec![other, directory]).is_empty());
    }

    #[test]
    fn falls_back_to_public_provider_item_id() {
        let defaults = default_metadata(&netease_entry(serde_json::json!({
            "songName": "歌曲"
        })));
        assert_eq!(defaults.get("songId"), Some(&serde_json::json!(1001)));
    }
}
