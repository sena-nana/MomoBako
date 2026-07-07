//! Eagle source 挂载快照构造器。
//!
//! 该模块把 Eagle `.library` 解析为 source 插件可直接消费的只读快照，
//! 复用现有 Eagle 导入阶段的字段映射与仓库对象转换逻辑。

use super::planner::{
    build_conversion_plan, flatten_folder_nodes, AssetPlan, FolderNode, RepositoryActionPlan,
    RepositoryShortcutPlan, SmartFolderPlan, TagGroupPlan,
};
use super::*;
use std::collections::BTreeMap;

const EAGLE_SOURCE_REPO_ID: &str = "eagle-library-source";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EagleSourceDiscoveredFile {
    pub absolute_path: Option<String>,
    pub relative_path: String,
    pub filename: String,
    pub extension: String,
    pub size_bytes: i64,
    pub created_at: Option<String>,
    pub modified_at: String,
    pub is_virtual: bool,
    pub provider_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub source_payload: Option<serde_json::Value>,
    pub local_absolute_path: Option<String>,
    pub status: Option<String>,
    pub shared_asset_id: Option<String>,
    pub tags: Option<Vec<String>>,
    pub thumbnail_local_absolute_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EagleSourceEntryKind {
    Directory,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EagleSourceEntry {
    pub path: String,
    pub name: String,
    pub kind: EagleSourceEntryKind,
    pub extension: Option<String>,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<String>,
    pub is_virtual: bool,
    pub provider_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub source_payload: Option<serde_json::Value>,
    pub local_absolute_path: Option<String>,
    pub status: Option<String>,
    pub shared_asset_id: Option<String>,
    pub tags: Option<Vec<String>>,
    pub thumbnail_local_absolute_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EagleSourceSnapshot {
    pub files: Vec<EagleSourceDiscoveredFile>,
    pub tree: Vec<FileTreeNode>,
    pub directories: BTreeMap<String, Vec<EagleSourceEntry>>,
    pub repository_state: SourceRepositoryStateSnapshot,
}

/// 构建 Eagle source 挂载快照。
pub fn build_eagle_source_snapshot(library_root: &Path) -> Result<EagleSourceSnapshot, String> {
    validate_eagle_library_root(library_root)?;
    let plan = build_conversion_plan(library_root, EAGLE_SOURCE_REPO_ID, "")?;
    let files = build_discovered_files(&plan)?;
    let tree = build_source_tree(&plan);
    let directories = build_directory_entries(&plan)?;
    let repository_state = build_repository_state(&plan);
    Ok(EagleSourceSnapshot {
        files,
        tree,
        directories,
        repository_state,
    })
}

fn build_discovered_files(plan: &ConversionPlan) -> Result<Vec<EagleSourceDiscoveredFile>, String> {
    let mut files = Vec::new();
    for asset in &plan.assets {
        let metadata = fs::metadata(&asset.source_file).map_err(io_error)?;
        let modified_at = asset_modified_at(asset, &metadata)?;
        let created_at = asset_created_at(asset);
        let size_bytes = i64::try_from(metadata.len())
            .map_err(|_| "file size exceeds supported range".to_string())?;
        let source_file_path = asset.source_file.to_string_lossy().to_string();
        let thumbnail_path = asset
            .source_thumbnail
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());
        for membership in &asset.memberships {
            files.push(EagleSourceDiscoveredFile {
                absolute_path: Some(source_file_path.clone()),
                relative_path: membership.target_relative_path.clone(),
                filename: membership.target_filename.clone(),
                extension: asset.extension.clone(),
                size_bytes,
                created_at: created_at.clone(),
                modified_at: modified_at.clone(),
                is_virtual: false,
                provider_id: None,
                provider_item_id: None,
                source_payload: Some(asset_source_payload(
                    asset,
                    membership,
                    thumbnail_path.as_deref(),
                )),
                local_absolute_path: Some(source_file_path.clone()),
                status: Some(if asset.is_deleted {
                    "deleted".to_string()
                } else {
                    "synced".to_string()
                }),
                shared_asset_id: Some(asset.eagle_asset_id.clone()),
                tags: Some(asset.tags.clone()),
                thumbnail_local_absolute_path: thumbnail_path.clone(),
            });
        }
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

fn build_source_tree(plan: &ConversionPlan) -> Vec<FileTreeNode> {
    let file_counts = plan
        .assets
        .iter()
        .filter(|asset| !asset.is_deleted)
        .flat_map(|asset| asset.memberships.iter())
        .fold(
            BTreeMap::<String, usize>::new(),
            |mut counts, membership| {
                *counts
                    .entry(membership.target_relative_dir.clone())
                    .or_default() += 1;
                counts
            },
        );
    plan.folder_nodes
        .iter()
        .map(|node| build_tree_node(node, &file_counts))
        .collect()
}

fn build_tree_node(node: &FolderNode, file_counts: &BTreeMap<String, usize>) -> FileTreeNode {
    FileTreeNode {
        path: node.path.clone(),
        label: Path::new(&node.path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(node.path.as_str())
            .to_string(),
        file_count: file_counts.get(&node.path).copied().unwrap_or(0),
        children: node
            .children
            .iter()
            .map(|child| build_tree_node(child, file_counts))
            .collect(),
    }
}

fn build_directory_entries(
    plan: &ConversionPlan,
) -> Result<BTreeMap<String, Vec<EagleSourceEntry>>, String> {
    let mut directories = BTreeMap::<String, Vec<EagleSourceEntry>>::new();
    directories.entry(String::new()).or_default();
    for folder in flatten_folder_nodes(&plan.folder_nodes) {
        let parent_path = parent_relative_path(&folder.path);
        directories.entry(folder.path.clone()).or_default();
        directories
            .entry(parent_path)
            .or_default()
            .push(EagleSourceEntry {
                path: folder.path.clone(),
                name: Path::new(&folder.path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(folder.path.as_str())
                    .to_string(),
                kind: EagleSourceEntryKind::Directory,
                extension: None,
                size_bytes: None,
                modified_at: None,
                is_virtual: false,
                provider_id: None,
                provider_item_id: None,
                source_payload: None,
                local_absolute_path: None,
                status: None,
                shared_asset_id: None,
                tags: None,
                thumbnail_local_absolute_path: None,
            });
    }
    for asset in &plan.assets {
        if asset.is_deleted {
            continue;
        }
        let metadata = fs::metadata(&asset.source_file).map_err(io_error)?;
        let modified_at = asset_modified_at(asset, &metadata)?;
        let size_bytes = i64::try_from(metadata.len())
            .map_err(|_| "file size exceeds supported range".to_string())?;
        let source_file_path = asset.source_file.to_string_lossy().to_string();
        let thumbnail_path = asset
            .source_thumbnail
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());
        for membership in &asset.memberships {
            directories
                .entry(membership.target_relative_dir.clone())
                .or_default()
                .push(EagleSourceEntry {
                    path: membership.target_relative_path.clone(),
                    name: membership.target_filename.clone(),
                    kind: EagleSourceEntryKind::File,
                    extension: Some(asset.extension.clone()),
                    size_bytes: Some(size_bytes),
                    modified_at: Some(modified_at.clone()),
                    is_virtual: false,
                    provider_id: None,
                    provider_item_id: None,
                    source_payload: Some(asset_source_payload(
                        asset,
                        membership,
                        thumbnail_path.as_deref(),
                    )),
                    local_absolute_path: Some(source_file_path.clone()),
                    status: Some("synced".to_string()),
                    shared_asset_id: Some(asset.eagle_asset_id.clone()),
                    tags: Some(asset.tags.clone()),
                    thumbnail_local_absolute_path: thumbnail_path.clone(),
                });
        }
    }
    for entries in directories.values_mut() {
        entries.sort_by(|left, right| match (&left.kind, &right.kind) {
            (EagleSourceEntryKind::Directory, EagleSourceEntryKind::File) => {
                std::cmp::Ordering::Less
            }
            (EagleSourceEntryKind::File, EagleSourceEntryKind::Directory) => {
                std::cmp::Ordering::Greater
            }
            _ => left.path.to_lowercase().cmp(&right.path.to_lowercase()),
        });
    }
    Ok(directories)
}

fn build_repository_state(plan: &ConversionPlan) -> SourceRepositoryStateSnapshot {
    let now = now_rfc3339();
    SourceRepositoryStateSnapshot {
        directory_metadata_by_path: flatten_folder_nodes(&plan.folder_nodes)
            .into_iter()
            .map(|folder| {
                (
                    folder.path,
                    FolderMetadata {
                        protected: folder.protected,
                        password_tip: folder.password_tip,
                    },
                )
            })
            .collect(),
        quick_access: build_shortcuts(&plan.quick_access),
        tag_groups: build_tag_groups(&plan.tag_groups),
        smart_folders: build_smart_folders(&plan.smart_folders, &now),
        repository_actions: build_repository_actions(&plan.repository_actions, &now),
        trash_entries: build_trash_entries(&plan.assets, &now),
    }
}

fn build_shortcuts(plans: &[RepositoryShortcutPlan]) -> Vec<RepositoryShortcut> {
    plans
        .iter()
        .map(|entry| RepositoryShortcut {
            shortcut_id: entry.shortcut_id.clone(),
            label: entry.label.clone(),
            target_kind: entry.target_kind.clone(),
            target_path: entry.target_path.clone(),
            target_id: entry.target_id.clone(),
        })
        .collect()
}

fn build_tag_groups(plans: &[TagGroupPlan]) -> Vec<RepositoryTagGroup> {
    plans
        .iter()
        .map(|entry| RepositoryTagGroup {
            tag_group_id: entry.tag_group_id.clone(),
            name: entry.name.clone(),
            tags: entry.tags.clone(),
        })
        .collect()
}

fn build_smart_folders(plans: &[SmartFolderPlan], now: &str) -> Vec<SmartFolder> {
    plans
        .iter()
        .map(|entry| SmartFolder {
            smart_folder_id: entry.smart_folder_id.clone(),
            repo_id: EAGLE_SOURCE_REPO_ID.to_string(),
            parent_id: None,
            name: entry.name.clone(),
            filter: deserialize_smart_folder_filter(&entry.filter),
            sort_order: entry.sort_order,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        })
        .collect()
}

/// 把 Eagle 智能文件夹过滤器转成宿主读模型。
fn deserialize_smart_folder_filter(value: &serde_json::Value) -> SmartFolderFilter {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

fn build_repository_actions(plans: &[RepositoryActionPlan], now: &str) -> Vec<RepositoryAction> {
    plans
        .iter()
        .map(|entry| RepositoryAction {
            action_id: entry.action_id.clone(),
            repo_id: EAGLE_SOURCE_REPO_ID.to_string(),
            source: "eagle-library".to_string(),
            source_action_id: entry.source_action_id.clone(),
            name: entry.name.clone(),
            status: entry.status.clone(),
            enabled: entry.enabled,
            raw: entry.raw.clone(),
            unsupported_reason: entry.unsupported_reason.clone(),
            sort_order: entry.sort_order,
            created_at: now.to_string(),
            updated_at: now.to_string(),
            steps: entry
                .steps
                .iter()
                .map(|step| RepositoryActionStep {
                    step_id: step.step_id.clone(),
                    action_id: entry.action_id.clone(),
                    repo_id: EAGLE_SOURCE_REPO_ID.to_string(),
                    step_kind: step.step_kind.clone(),
                    label: step.label.clone(),
                    status: step.status.clone(),
                    config: step.config.clone(),
                    raw: step.raw.clone(),
                    unsupported_reason: step.unsupported_reason.clone(),
                    sort_order: step.sort_order,
                })
                .collect(),
            last_run: None,
        })
        .collect()
}

fn build_trash_entries(assets: &[AssetPlan], now: &str) -> Vec<SourceTrashEntry> {
    let mut trash_entries = Vec::new();
    for asset in assets.iter().filter(|asset| asset.is_deleted) {
        let deleted_at = asset
            .preserved_metadata
            .get("fileModifiedAt")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| now.to_string());
        for membership in &asset.memberships {
            trash_entries.push(SourceTrashEntry {
                trash_path: membership.target_relative_path.clone(),
                original_path: membership.target_relative_path.clone(),
                kind: "file".to_string(),
                deleted_at: deleted_at.clone(),
                shared_asset_id: Some(asset.eagle_asset_id.clone()),
            });
        }
    }
    trash_entries.sort_by(|left, right| left.trash_path.cmp(&right.trash_path));
    trash_entries
}

fn asset_created_at(asset: &AssetPlan) -> Option<String> {
    asset
        .preserved_metadata
        .get("fileCreatedAt")
        .or_else(|| asset.preserved_metadata.get("addedToLibraryAt"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn asset_modified_at(asset: &AssetPlan, metadata: &fs::Metadata) -> Result<String, String> {
    asset
        .preserved_metadata
        .get("fileModifiedAt")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .map(Ok)
        .unwrap_or_else(|| {
            metadata
                .modified()
                .map_err(io_error)
                .and_then(|value| system_time_to_rfc3339(value).map_err(time_error))
        })
}

fn asset_source_payload(
    asset: &AssetPlan,
    membership: &super::planner::AssetMembership,
    thumbnail_path: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "sharedAssetId": asset.eagle_asset_id,
        "eagleAssetId": asset.eagle_asset_id,
        "aliasRole": membership.role,
        "thumbnailLocalAbsolutePath": thumbnail_path,
        "sourceFileAbsolutePath": asset.source_file.to_string_lossy().to_string(),
    })
}
