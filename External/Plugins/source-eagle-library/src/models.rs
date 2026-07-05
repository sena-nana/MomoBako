//! Eagle Library source 插件的载荷与序列化模型。

use std::{collections::BTreeMap, path::PathBuf};

use momobako_lib::EagleSourceEntry;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginPayload {
    pub(crate) repo_root: PathBuf,
    #[serde(default)]
    pub(crate) directory_path: Option<String>,
    #[serde(default)]
    pub(crate) offset: Option<usize>,
    #[serde(default)]
    pub(crate) limit: Option<usize>,
    #[serde(default)]
    pub(crate) parent_path: Option<String>,
    #[serde(default)]
    pub(crate) target_parent_path: Option<String>,
    #[serde(default)]
    pub(crate) entry_path: Option<String>,
    #[serde(default)]
    pub(crate) source_path: Option<String>,
    #[serde(default)]
    pub(crate) path: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) new_name: Option<String>,
    #[serde(default)]
    pub(crate) recursive: Option<bool>,
    #[serde(default)]
    pub(crate) shared_asset_id: Option<String>,
    #[serde(default)]
    pub(crate) metadata: Option<BTreeMap<String, Value>>,
    #[serde(default)]
    pub(crate) previous_metadata: Option<BTreeMap<String, Value>>,
    #[serde(default)]
    pub(crate) operation: Option<String>,
    #[serde(default)]
    pub(crate) directory_metadata_by_path: Option<BTreeMap<String, FolderMetadataPayload>>,
    #[serde(default)]
    pub(crate) quick_access: Option<Vec<RepositoryShortcutPayload>>,
    #[serde(default)]
    pub(crate) tag_groups: Option<Vec<RepositoryTagGroupPayload>>,
    #[serde(default)]
    pub(crate) smart_folders: Option<Vec<SmartFolderPayload>>,
    #[serde(default)]
    pub(crate) repository_actions: Option<Vec<RepositoryActionPayload>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DirectoryPageResult {
    pub(crate) entries: Vec<EagleSourceEntry>,
    pub(crate) total_entries: usize,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FolderMetadataPayload {
    pub(crate) protected: bool,
    pub(crate) password_tip: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepositoryShortcutPayload {
    pub(crate) shortcut_id: String,
    pub(crate) label: String,
    pub(crate) target_kind: String,
    pub(crate) target_path: Option<String>,
    pub(crate) target_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepositoryTagGroupPayload {
    pub(crate) tag_group_id: String,
    pub(crate) name: String,
    pub(crate) tags: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SmartFolderPayload {
    pub(crate) smart_folder_id: String,
    pub(crate) name: String,
    pub(crate) filter: Value,
    pub(crate) sort_order: i64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepositoryActionPayload {
    pub(crate) raw: Value,
    pub(crate) enabled: bool,
}

pub(crate) fn serialize_shortcut(shortcut: RepositoryShortcutPayload) -> Value {
    match shortcut.target_kind.as_str() {
        "smartFolder" => serde_json::json!({
            "id": shortcut.shortcut_id,
            "name": shortcut.label,
            "type": "smartFolder",
            "smartFolderId": shortcut.target_id,
        }),
        "file" => serde_json::json!({
            "id": shortcut.shortcut_id,
            "name": shortcut.label,
            "type": "file",
            "path": shortcut.target_path,
        }),
        _ => serde_json::json!({
            "id": shortcut.shortcut_id,
            "name": shortcut.label,
            "type": "folder",
            "path": shortcut.target_path,
        }),
    }
}

pub(crate) fn serialize_tag_group(group: RepositoryTagGroupPayload) -> Value {
    serde_json::json!({
        "id": group.tag_group_id,
        "name": group.name,
        "tags": group.tags,
    })
}

pub(crate) fn serialize_smart_folder(folder: SmartFolderPayload) -> Value {
    serde_json::json!({
        "id": folder.smart_folder_id,
        "name": folder.name,
        "sortOrder": folder.sort_order,
        "filter": folder.filter,
    })
}

pub(crate) fn set_raw_action_enabled(raw: Value, enabled: bool) -> Value {
    let mut object = raw.as_object().cloned().unwrap_or_default();
    object.insert("enabled".to_string(), Value::Bool(enabled));
    Value::Object(object)
}
