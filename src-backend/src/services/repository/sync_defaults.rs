//! Parser 默认元数据批处理与来源字段安全投影。

use super::*;
use crate::services::logging::write_log;

/// 为全量或增量同步发现的文件构造安全批量输入并调用 Parser。
pub(super) fn metadata_defaults_for_files(
    service_root: &Path,
    files: &[DiscoveredFile],
    existing_metadata_by_path: &BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    source_metadata_keys: &[String],
) -> Result<BTreeMap<String, BTreeMap<String, serde_json::Value>>, String> {
    let entries = files
        .iter()
        .map(|file| MetadataDefaultsBatchEntry {
            path: file.relative_path.clone(),
            name: file.filename.clone(),
            extension: file.extension.clone(),
            kind: "file".to_string(),
            provider_id: file.provider_id.clone(),
            provider_item_id: file.provider_item_id.clone(),
            source_metadata: project_source_metadata(
                file.source_payload.as_ref(),
                source_metadata_keys,
            ),
            metadata: existing_metadata_by_path.get(&file.relative_path).cloned(),
        })
        .collect::<Vec<_>>();
    metadata_defaults_for_entries(service_root, entries)
}

/// 为分页来源条目计算默认元数据，输入只包含 Source 清单允许公开的来源字段。
pub(super) fn metadata_defaults_for_file_system_entries(
    service_root: &Path,
    entries: &[FileSystemEntry],
    existing_metadata_by_path: &BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    source_metadata_keys: &[String],
) -> Result<BTreeMap<String, BTreeMap<String, serde_json::Value>>, String> {
    let entries = entries
        .iter()
        .map(|entry| MetadataDefaultsBatchEntry {
            path: entry.path.clone(),
            name: entry.name.clone(),
            extension: entry.extension.clone().unwrap_or_default(),
            kind: match entry.kind {
                FileSystemEntryKind::Directory => "directory",
                FileSystemEntryKind::File => "file",
            }
            .to_string(),
            provider_id: entry.provider_id.clone(),
            provider_item_id: entry.provider_item_id.clone(),
            source_metadata: project_source_metadata(
                entry.source_payload.as_ref(),
                source_metadata_keys,
            ),
            metadata: existing_metadata_by_path.get(&entry.path).cloned(),
        })
        .collect::<Vec<_>>();
    metadata_defaults_for_entries(service_root, entries)
}

/// 依次合并可用 Parser 的返回值，并丢弃不属于本批次的路径。
fn metadata_defaults_for_entries(
    service_root: &Path,
    entries: Vec<MetadataDefaultsBatchEntry>,
) -> Result<BTreeMap<String, BTreeMap<String, serde_json::Value>>, String> {
    if entries.is_empty() {
        return Ok(BTreeMap::new());
    }

    let registry = plugin_catalog(service_root);
    let providers = registry.metadata_default_providers();
    if providers.is_empty() {
        return Ok(BTreeMap::new());
    }

    let known_paths = entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let payload = serde_json::json!({ "entries": entries });
    let mut defaults_by_path = BTreeMap::<String, BTreeMap<String, serde_json::Value>>::new();

    for (plugin_id, action) in providers {
        let response = registry.call(&plugin_id, &action, payload.clone())?;
        let parsed =
            serde_json::from_value::<MetadataDefaultsBatchResponse>(response).map_err(|error| {
                log_metadata_defaults_error(&plugin_id, &action, &error.to_string());
                json_error(error)
            })?;
        for (path, defaults) in parsed.defaults_by_path {
            if !known_paths.contains(&path) {
                continue;
            }
            defaults_by_path.entry(path).or_default().extend(defaults);
        }
    }

    Ok(defaults_by_path)
}

/// 按 Source 清单声明的键投影来源元数据，避免认证信息进入 Parser 调用。
pub(super) fn project_source_metadata(
    source_payload: Option<&serde_json::Value>,
    source_metadata_keys: &[String],
) -> Option<BTreeMap<String, serde_json::Value>> {
    let source_payload = source_payload?.as_object()?;
    let projected = source_metadata_keys
        .iter()
        .filter_map(|key| {
            source_payload
                .get(key)
                .cloned()
                .map(|value| (key.clone(), value))
        })
        .collect::<BTreeMap<_, _>>();
    (!projected.is_empty()).then_some(projected)
}

fn log_metadata_defaults_error(plugin_id: &str, action: &str, error: &str) {
    let _ = write_log(SystemLogWriteRequest {
        level: "error".to_string(),
        category: "plugin.metadata-defaults".to_string(),
        action: "invalidResponse".to_string(),
        message: "Parser 返回的默认元数据格式无效。".to_string(),
        context: Some(serde_json::json!({
            "pluginId": plugin_id,
            "method": action,
            "error": error,
        })),
        repo_id: None,
        plugin_id: Some(plugin_id.to_string()),
        source_kind: Some("backend-plugin".to_string()),
        source_label: Some(plugin_id.to_string()),
        location: Some(SystemLogLocationInput {
            module_path: Some(module_path!().to_string()),
            file: Some(file!().to_string()),
            line: Some(line!()),
        }),
    });
}
