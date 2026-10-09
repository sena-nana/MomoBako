//! 验收场景共用的插件和设置包夹具：照 `tmp/vue-mock/ipc.ts` 的 `realPlugins`、`get_cache_snapshot`、
//! `get_api_design_snapshot` 和 `get_external_api_connection_status` 应答。
//!
//! 产品启动结束时读一次设置包、插件列表换新后读一次播放器类型，底子（`acceptance_base.rs`）和
//! 管理面板的场景（`acceptance_admin.rs`）都用这里的应答回答这两个请求。

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::backend::services::repository::{
    ApiDesignSnapshot, CacheConfig, CacheSnapshot, PlaylistPlayerContribution, PluginManifest,
};
use crate::backend::services::runtime::ExternalApiConnectionStatus;

use super::super::admin::AdminMessage;
use super::base_scene::NOW;

/// 读仓库自带的插件清单，按目录名排序（Vite `import.meta.glob` 的顺序），改成已启用、
/// 依赖就绪的内置插件。读不到或解析失败的清单记日志后跳过。
pub(super) fn bundled_plugins(keep: impl Fn(&str) -> bool) -> Vec<PluginManifest> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../External/Plugins");
    let mut manifests = match fs::read_dir(&root) {
        Ok(entries) => entries.filter_map(Result::ok).map(|entry| entry.path().join("manifest.json")).filter(|path| path.is_file()).collect::<Vec<_>>(),
        Err(error) => {
            eprintln!("Nana 验收读不到插件目录 {}：{error}", root.display());
            return Vec::new();
        }
    };
    manifests.sort();
    manifests
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(&path).map_err(|error| eprintln!("Nana 验收读不到插件清单 {}：{error}", path.display())).ok()?;
            let mut manifest: Map<String, Value> =
                serde_json::from_str(&text).map_err(|error| eprintln!("Nana 验收插件清单不是 JSON {}：{error}", path.display())).ok()?;
            let plugin_id = manifest.get("pluginId").and_then(Value::as_str)?.to_string();
            if !keep(&plugin_id) {
                return None;
            }
            mock_install(&mut manifest);
            serde_json::from_value(Value::Object(manifest))
                .map_err(|error| eprintln!("Nana 验收插件清单字段不全 {plugin_id}：{error}"))
                .ok()
        })
        .collect()
}

/// 插件清单里声明的播放器，按清单顺序。领域服务的 `list_playlist_players` 也从已启用插件的
/// `contributes.playlistPlayers` 取。字段不全的声明记日志后跳过。
pub(super) fn players_of(plugins: &[PluginManifest]) -> Vec<PlaylistPlayerContribution> {
    plugins
        .iter()
        .filter(|plugin| plugin.enabled)
        .filter_map(|plugin| plugin.contributes.get("playlistPlayers").cloned())
        .flat_map(|players| match serde_json::from_value::<Vec<PlaylistPlayerContribution>>(players) {
            Ok(players) => players,
            Err(error) => {
                eprintln!("Nana 验收播放器声明解析失败：{error}");
                Vec::new()
            }
        })
        .collect()
}

/// 一次读完设置包五份数据的应答，插件之外都和 Vue 模拟 IPC 的默认应答一致。
pub(super) fn bundle_loaded(plugins: Vec<PluginManifest>) -> AdminMessage {
    AdminMessage::SettingsBundleLoaded {
        plugins: Ok(plugins),
        hooks: Ok(Vec::new()),
        cache: Ok(cache_snapshot()),
        api: Ok(api_design()),
        external: Ok(external_status()),
    }
}

/// 照 `tmp/vue-mock/ipc.ts` 的 `realPlugins`：已启用、就绪、内置来源、依赖全部可用。
fn mock_install(manifest: &mut Map<String, Value>) {
    let version = manifest.get("version").and_then(Value::as_str).unwrap_or("0").to_string();
    let dependencies = |key: &str| {
        manifest
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|id| json!({ "pluginId": id, "name": null, "status": "ready", "enabled": true, "available": true }))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let status = json!({
        "required": dependencies("requires"),
        "optional": dependencies("optional"),
        "missingRequired": [],
        "missingOptional": [],
        "disabledRequired": [],
        "disabledOptional": [],
    });
    manifest.insert("enabled".into(), Value::Bool(true));
    manifest.insert("status".into(), Value::String("ready".into()));
    manifest.entry("source").or_insert_with(|| Value::String("builtin".into()));
    manifest.insert("provenance".into(), Value::String("bundled".into()));
    manifest.insert("packageHash".into(), Value::String(format!("mock-{version}")));
    manifest.insert("dependencyStatus".into(), status);
}

pub(super) fn cache_snapshot() -> CacheSnapshot {
    CacheSnapshot { config: CacheConfig { metadata_capacity: 512, thumbnail_capacity: 1024, query_capacity: 256 }, entries: Vec::new() }
}

pub(super) fn api_design() -> ApiDesignSnapshot {
    ApiDesignSnapshot { transport: "tauri-ipc".into(), endpoints: Vec::new() }
}

pub(super) fn external_status() -> ExternalApiConnectionStatus {
    ExternalApiConnectionStatus {
        base_url: "http://127.0.0.1:41595".into(),
        token: "mock-token".into(),
        version: "0.1.0".into(),
        started_at: NOW.into(),
        ready: true,
        connection_file_path: "C:/Users/acceptance/AppData/Roaming/com.momobako.desktop/external-api.json".into(),
    }
}
