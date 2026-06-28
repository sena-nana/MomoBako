//! Runtime API design snapshot builders.

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginApiTestContribution {
    pub(super) method: String,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) payload: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) request_template: Option<serde_json::Value>,
}

pub(super) fn default_api_definitions(service_root: &Path) -> Vec<ApiDefinition> {
    let mut definitions = Vec::new();
    definitions.extend(external_api_definitions());
    definitions.extend(core_tauri_api_definitions());
    definitions.extend(plugin_api_definitions(service_root));
    definitions
}

pub(super) fn external_api_definitions() -> Vec<ApiDefinition> {
    vec![
        external_api_definition(
            "GET",
            "/external/v1/health",
            "检查外部 API 服务状态。",
            false,
            None,
        ),
        external_api_definition(
            "GET",
            "/external/v1/repositories",
            "列出可接收外部素材的本地仓库。",
            true,
            None,
        ),
        external_api_definition(
            "POST",
            "/external/v1/assets:add",
            "从远程 URL 添加素材到仓库。",
            true,
            Some(serde_json::json!({
                "repoId": "",
                "parentPath": "",
                "client": {
                    "id": "momobako.api-playground",
                    "name": "API Playground",
                    "version": "0.1.0"
                },
                "items": [
                    {
                        "kind": "remoteUrl",
                        "url": "https://example.com/image.png",
                        "filename": "image.png",
                        "metadata": {
                            "sourceUrl": "https://example.com/image.png"
                        }
                    }
                ]
            })),
        ),
    ]
}

pub(super) fn core_tauri_api_definitions() -> Vec<ApiDefinition> {
    vec![
        tauri_api_definition(
            "Runtime API",
            "ping",
            "检测 Tauri 命令桥。",
            serde_json::json!({}),
        ),
        tauri_api_definition(
            "Repository API",
            "list_repositories",
            "列出所有仓库。",
            serde_json::json!({}),
        ),
        tauri_api_definition(
            "Repository API",
            "get_repository_snapshot",
            "读取仓库总览、文件树和基础状态。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Asset API",
            "get_asset_detail",
            "读取单个素材详情与元数据。",
            serde_json::json!({ "repoId": "<repoId>", "assetId": "<assetId>" }),
        ),
        tauri_api_definition(
            "Search API",
            "search_assets",
            "执行跨仓库结构化搜索。",
            serde_json::json!({
                "request": {
                    "query": "",
                    "repoId": null,
                    "limit": 20
                }
            }),
        ),
        tauri_api_definition(
            "Metadata API",
            "update_asset_metadata",
            "带乐观锁更新素材元数据。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "assetId": "<assetId>",
                    "expectedVersion": 1,
                    "metadata": {},
                    "source": "api-playground"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "get_file_browser",
            "读取仓库文件浏览快照。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "directoryPath": "",
                    "includeTree": true,
                    "specialLocation": null
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "list_playlists",
            "列出仓库播放列表。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Playlist API",
            "list_playlist_memberships",
            "列出素材到播放列表的轻量成员关系索引。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Playlist API",
            "create_playlist",
            "创建播放列表。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": null,
                    "name": "New Playlist",
                    "playerTypeId": "builtin.sequence"
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "update_playlist",
            "更新播放列表名称或播放器。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": "<playlistId>",
                    "name": "Updated Playlist",
                    "playerTypeId": null
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "delete_playlist",
            "删除播放列表。",
            serde_json::json!({ "repoId": "<repoId>", "playlistId": "<playlistId>" }),
        ),
        tauri_api_definition(
            "Playlist API",
            "get_playlist_detail",
            "读取播放列表详情。",
            serde_json::json!({ "repoId": "<repoId>", "playlistId": "<playlistId>" }),
        ),
        tauri_api_definition(
            "Playlist API",
            "add_playlist_items",
            "向播放列表添加素材。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": "<playlistId>",
                    "assetIds": ["<assetId>"]
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "add_playlist_items_by_paths",
            "按文件或目录路径向播放列表添加条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": "<playlistId>",
                    "paths": ["<path>"]
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "reorder_playlist_items",
            "重排播放列表条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": "<playlistId>",
                    "itemIds": ["<playlistItemId>"]
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "remove_playlist_item",
            "移除播放列表条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "playlistId": "<playlistId>",
                    "playlistItemId": "<playlistItemId>"
                }
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "set_playlist_membership",
            "设置素材所属播放列表。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "assetId": "<assetId>",
                    "playlistIds": ["<playlistId>"]
                }
            }),
        ),
        tauri_api_definition(
            "Smart Folder API",
            "list_smart_folders",
            "列出智能文件夹树。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Smart Folder API",
            "create_smart_folder",
            "创建智能文件夹。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "smartFolderId": null,
                    "parentId": null,
                    "name": "New Smart Folder",
                    "filter": { "query": "", "limit": 20 }
                }
            }),
        ),
        tauri_api_definition(
            "Smart Folder API",
            "update_smart_folder",
            "更新智能文件夹。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "smartFolderId": "<smartFolderId>",
                    "parentId": null,
                    "name": "Updated Smart Folder",
                    "filter": { "query": "", "limit": 20 }
                }
            }),
        ),
        tauri_api_definition(
            "Smart Folder API",
            "delete_smart_folder",
            "删除智能文件夹。",
            serde_json::json!({ "repoId": "<repoId>", "smartFolderId": "<smartFolderId>" }),
        ),
        tauri_api_definition(
            "Smart Folder API",
            "query_smart_folder",
            "按智能文件夹条件查询虚拟文件列表。",
            serde_json::json!({ "repoId": "<repoId>", "smartFolderId": "<smartFolderId>" }),
        ),
        tauri_api_definition(
            "Repository Action API",
            "list_repository_actions",
            "列出仓库动作。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Repository Action API",
            "get_repository_action",
            "读取单个仓库动作。",
            serde_json::json!({ "repoId": "<repoId>", "actionId": "<actionId>" }),
        ),
        tauri_api_definition(
            "Repository Action API",
            "set_repository_action_enabled",
            "启用或停用仓库动作。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "actionId": "<actionId>",
                    "enabled": true
                }
            }),
        ),
        tauri_api_definition(
            "Repository Action API",
            "run_repository_action",
            "运行仓库动作。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "actionId": "<actionId>",
                    "targetPaths": [],
                    "assetIds": []
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "read_file",
            "读取仓库文件字节。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "path": "<path>" } }),
        ),
        tauri_api_definition(
            "Preview API",
            "prepare_preview_file_source",
            "为本地文件预览准备流式读取源。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "path": "<path>" } }),
        ),
        tauri_api_definition(
            "Preview API",
            "prepare_entry_playback_source",
            "为本地或虚拟条目准备播放源。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "path": "<path>" } }),
        ),
        tauri_api_definition(
            "Preview API",
            "prepare_entry_playback_source_with_progress",
            "为本地或虚拟条目准备播放源，并通过进度通道回报准备与下载阶段。",
            serde_json::json!({
                "request": { "repoId": "<repoId>", "path": "<path>" },
                "progress": "<Channel<EntryPlaybackProgressEvent>>"
            }),
        ),
        tauri_api_definition(
            "Playlist API",
            "download_playlist_with_progress",
            "下载歌单并通过进度通道回报逐首处理状态。",
            serde_json::json!({
                "request": {
                    "playlistId": 9001,
                    "playlistName": "夜跑歌单",
                    "tracks": [
                        {
                            "songId": 2001,
                            "songName": "稻香",
                            "sourcePayload": {
                                "provider": "netease-cloud-music",
                                "songId": 2001
                            }
                        }
                    ],
                    "destination": {
                        "kind": "localFolder",
                        "path": "C:/Downloads/Playlist"
                    },
                    "sourcePayload": {
                        "provider": "netease-cloud-music",
                        "playlistId": 9001
                    },
                    "level": "standard"
                },
                "progress": "<Channel<DownloaderPlaylistProgressEvent>>"
            }),
        ),
        tauri_api_definition(
            "Plugin API",
            "call_plugin",
            "调用后端插件方法。",
            serde_json::json!({
                "request": {
                    "pluginId": "<pluginId>",
                    "method": "<method>",
                    "payload": {}
                }
            }),
        ),
        tauri_api_definition(
            "Plugin API",
            "read_plugin_archive_text",
            "读取插件包内文本文件。",
            serde_json::json!({ "request": { "pluginId": "<pluginId>", "path": "manifest.json" } }),
        ),
        tauri_api_definition(
            "Plugin API",
            "get_plugin_data_directory",
            "创建并读取插件自有数据目录。",
            serde_json::json!({ "pluginId": "<pluginId>" }),
        ),
        tauri_api_definition(
            "Plugin API",
            "prepare_plugin_data_file_preview_source",
            "将插件数据目录内文件注册为受控预览源。",
            serde_json::json!({
                "request": {
                    "pluginId": "<pluginId>",
                    "path": "<absolutePluginDataFilePath>",
                    "mediaType": "text/plain; charset=utf-8"
                }
            }),
        ),
        tauri_api_definition(
            "Plugin API",
            "get_plugin_config",
            "读取插件 key-value 配置快照。",
            serde_json::json!({ "pluginId": "<pluginId>" }),
        ),
        tauri_api_definition(
            "Plugin API",
            "set_plugin_config_value",
            "写入插件 key-value 配置项。",
            serde_json::json!({
                "request": {
                    "pluginId": "<pluginId>",
                    "key": "apiKey",
                    "value": "<value>"
                }
            }),
        ),
        tauri_api_definition(
            "Plugin API",
            "delete_plugin_config_value",
            "删除插件 key-value 配置项。",
            serde_json::json!({
                "request": {
                    "pluginId": "<pluginId>",
                    "key": "apiKey"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "write_binary_file",
            "写入二进制文件。",
            serde_json::json!({ "request": { "path": "<absolutePath>", "bytes": [] } }),
        ),
        tauri_api_definition(
            "File API",
            "create_directory",
            "创建目录。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "parentPath": "",
                    "name": "New Folder"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "create_file",
            "创建空文件。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "parentPath": "",
                    "name": "new-file.txt"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "import_entries",
            "导入外部文件或目录。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "parentPath": "",
                    "sourcePaths": ["<absolutePath>"]
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "copy_entries",
            "复制仓库内文件条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "sourcePaths": ["<path>"],
                    "parentPath": "",
                    "mode": "copy"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "move_entries",
            "移动仓库内文件条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "sourcePaths": ["<path>"],
                    "parentPath": ""
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "rename_entry",
            "重命名仓库内文件条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "path": "<path>",
                    "newName": "renamed.txt"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "delete_entry",
            "删除或移入回收站。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "path": "<path>",
                    "mode": "trash"
                }
            }),
        ),
        tauri_api_definition(
            "File API",
            "mutate_trash",
            "恢复或清理回收站条目。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "action": "restore",
                    "path": "<trashPath>"
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "create_repository",
            "创建仓库。",
            serde_json::json!({
                "request": {
                    "repoId": null,
                    "name": "New Repository",
                    "path": "<absolutePath>",
                    "backendPluginId": null,
                    "backendConfig": null
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "import_repository",
            "导入已有仓库。",
            serde_json::json!({
                "request": {
                    "repoId": null,
                    "name": "Imported Repository",
                    "path": "<absolutePath>",
                    "backendPluginId": null,
                    "backendConfig": null
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "attach_repository_folder",
            "挂载仓库文件夹。",
            serde_json::json!({ "request": { "path": "<absolutePath>" } }),
        ),
        tauri_api_definition(
            "Repository API",
            "delete_repository",
            "删除仓库记录。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Repository API",
            "relocate_repository",
            "重定位仓库路径。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "path": "<absolutePath>" } }),
        ),
        tauri_api_definition(
            "Repository API",
            "update_repository_backend_config",
            "更新仓库后端配置。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "backendConfig": {}
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "configure_netease_repository_cache",
            "配置网易云资源库本地缓存目录并迁移可识别旧缓存。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "path": "<absolutePath>",
                    "migrateLegacyCache": true
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "export_repository",
            "导出仓库元数据。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "target": "archive",
                    "archive": {
                        "format": "zip",
                        "outputPath": "<absolutePath>",
                        "compression": "default",
                        "encrypt": false,
                        "password": null
                    },
                    "git": null
                }
            }),
        ),
        tauri_api_definition(
            "Repository API",
            "sync_repository",
            "同步仓库文件状态。",
            serde_json::json!({ "request": { "repoId": "<repoId>" } }),
        ),
        tauri_api_definition(
            "Hardlink API",
            "list_hardlink_candidates",
            "列出硬链接候选。",
            serde_json::json!({ "repoId": "<repoId>" }),
        ),
        tauri_api_definition(
            "Hardlink API",
            "confirm_hardlink_candidate",
            "确认硬链接候选。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "candidateId": "<candidateId>" } }),
        ),
        tauri_api_definition(
            "Thumbnail API",
            "ensure_thumbnail",
            "按需复用或生成缩略图。",
            serde_json::json!({
                "request": {
                    "repoId": "<repoId>",
                    "path": "<path>",
                    "action": "ensure",
                    "sourcePath": null,
                    "sourceUrl": null,
                    "imageBytes": null,
                    "mediaType": null
                }
            }),
        ),
        tauri_api_definition(
            "Revision API",
            "undo_last_revision",
            "回滚到上一版 metadata 状态。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "assetId": "<assetId>" } }),
        ),
        tauri_api_definition(
            "Revision API",
            "redo_last_revision",
            "重做到下一版 metadata 状态。",
            serde_json::json!({ "request": { "repoId": "<repoId>", "assetId": "<assetId>" } }),
        ),
        tauri_api_definition(
            "Plugin API",
            "list_plugins",
            "列出插件与能力声明。",
            serde_json::json!({}),
        ),
        tauri_api_definition(
            "Plugin API",
            "list_plugin_hook_executions",
            "列出插件 Hook 执行记录。",
            serde_json::json!({ "request": { "pluginId": "<pluginId>", "limit": 50 } }),
        ),
        tauri_api_definition(
            "Plugin API",
            "set_plugin_enabled",
            "启用或停用插件。",
            serde_json::json!({ "request": { "pluginId": "<pluginId>", "enabled": true } }),
        ),
        tauri_api_definition(
            "Plugin API",
            "delete_plugin",
            "删除插件。",
            serde_json::json!({ "pluginId": "<pluginId>" }),
        ),
        tauri_api_definition(
            "Plugin API",
            "install_plugin_from_archive",
            "从插件包安装插件。",
            serde_json::json!({ "request": { "packagePath": "<absolutePackagePath>" } }),
        ),
        tauri_api_definition(
            "Cache API",
            "get_cache_snapshot",
            "读取缓存配置与状态。",
            serde_json::json!({}),
        ),
        tauri_api_definition(
            "Runtime API",
            "get_api_design_snapshot",
            "读取 API 调试契约快照。",
            serde_json::json!({}),
        ),
        tauri_api_definition(
            "Runtime API",
            "get_external_api_connection_status",
            "读取外部 API 连接信息。",
            serde_json::json!({}),
        ),
    ]
}

pub(super) fn plugin_api_definitions(service_root: &Path) -> Vec<ApiDefinition> {
    let registry = backend_plugin_registry(service_root);
    let mut definitions = Vec::new();
    let mut seen = HashSet::<(String, String)>::new();

    for manifest in registry.list_manifests() {
        if !plugin_manifest_can_be_called(&manifest) {
            continue;
        }
        let Some(contributes) = manifest.contributes.as_object() else {
            continue;
        };

        if let Some(raw_tests) = contributes.get("apiTests") {
            if let Ok(tests) =
                serde_json::from_value::<Vec<PluginApiTestContribution>>(raw_tests.clone())
            {
                for test in tests {
                    if test.method.trim().is_empty() {
                        continue;
                    }
                    let method = test.method.trim().to_string();
                    if !seen.insert((manifest.plugin_id.clone(), method.clone())) {
                        continue;
                    }
                    let summary = test
                        .summary
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| format!("调用插件 API {method}。"));
                    let payload = test
                        .payload
                        .or(test.request_template)
                        .unwrap_or_else(|| serde_json::json!({}));
                    definitions.push(plugin_api_definition(
                        &manifest.plugin_id,
                        &manifest.name,
                        &method,
                        &summary,
                        payload,
                    ));
                }
            }
        }

        if let Some(action) = contributes
            .get("provider")
            .and_then(|provider| provider.get("lookup"))
            .and_then(|lookup| lookup.get("action"))
            .and_then(|action| action.as_str())
            .map(str::trim)
            .filter(|action| !action.is_empty())
        {
            if seen.insert((manifest.plugin_id.clone(), action.to_string())) {
                definitions.push(plugin_api_definition(
                    &manifest.plugin_id,
                    &manifest.name,
                    action,
                    &format!("查询 {} 元数据候选。", manifest.name),
                    serde_json::json!({ "id": "<externalId>" }),
                ));
            }
        }

        if let Some(action) = contributes
            .get("metadataDefaults")
            .and_then(|defaults| defaults.get("action"))
            .and_then(|action| action.as_str())
            .map(str::trim)
            .filter(|action| !action.is_empty())
        {
            if seen.insert((manifest.plugin_id.clone(), action.to_string())) {
                definitions.push(plugin_api_definition(
                    &manifest.plugin_id,
                    &manifest.name,
                    action,
                    &format!("生成 {} 元数据默认值。", manifest.name),
                    serde_json::json!({
                        "entries": [
                            {
                                "path": "work/track01.mp3",
                                "name": "track01.mp3",
                                "extension": "mp3",
                                "kind": "file"
                            }
                        ]
                    }),
                ));
            }
        }
    }

    definitions.sort_by(|left, right| {
        left.group
            .cmp(&right.group)
            .then_with(|| left.path.cmp(&right.path))
    });
    definitions
}

pub(super) fn plugin_manifest_can_be_called(manifest: &PluginManifest) -> bool {
    manifest.enabled
        && manifest.sdk == "backend"
        && !matches!(
            manifest.status.as_str(),
            "disabled" | "error" | "unavailable"
        )
}

pub(super) fn external_api_definition(
    method: &str,
    path: &str,
    summary: &str,
    requires_auth: bool,
    request_template: Option<serde_json::Value>,
) -> ApiDefinition {
    ApiDefinition {
        group: "External Asset API".to_string(),
        transport: "external-http".to_string(),
        method: method.to_string(),
        path: path.to_string(),
        summary: summary.to_string(),
        command: None,
        plugin_id: None,
        plugin_method: None,
        requires_auth: Some(requires_auth),
        request_template,
    }
}

pub(super) fn tauri_api_definition(
    group: &str,
    command: &str,
    summary: &str,
    request_template: serde_json::Value,
) -> ApiDefinition {
    ApiDefinition {
        group: group.to_string(),
        transport: "tauri-command".to_string(),
        method: "INVOKE".to_string(),
        path: command.to_string(),
        summary: summary.to_string(),
        command: Some(command.to_string()),
        plugin_id: None,
        plugin_method: None,
        requires_auth: None,
        request_template: Some(request_template),
    }
}

pub(super) fn plugin_api_definition(
    plugin_id: &str,
    plugin_name: &str,
    method: &str,
    summary: &str,
    request_template: serde_json::Value,
) -> ApiDefinition {
    ApiDefinition {
        group: format!("Plugin API / {plugin_name}"),
        transport: "plugin-call".to_string(),
        method: "PLUGIN".to_string(),
        path: format!("{plugin_id}:{method}"),
        summary: summary.to_string(),
        command: None,
        plugin_id: Some(plugin_id.to_string()),
        plugin_method: Some(method.to_string()),
        requires_auth: None,
        request_template: Some(request_template),
    }
}
