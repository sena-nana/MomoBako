//! Eagle 仓库级对象转换器。
//!
//! 该模块负责把 Eagle 的快捷入口、标签组和仓库动作转换为 MomoBako 仓库对象。

use super::super::*;
use super::planner::{
    dedupe_preserve_order, first_non_empty_string, normalize_non_empty_string,
    normalize_non_negative_number, normalize_text_values, prefix_relative_path, unique_id, warning,
    AssetPlan, RepositoryActionPlan, RepositoryActionStepPlan, RepositoryShortcutPlan,
    SmartFolderPlan, TagGroupPlan,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// 构建 Eagle quickAccess 对应的仓库快捷入口计划。
pub(super) fn build_quick_access_plans(
    repo_id: &str,
    quick_access: Option<&Value>,
    folder_index: &BTreeMap<String, String>,
    smart_folders: &[SmartFolderPlan],
    assets: &[AssetPlan],
    parent_path: &str,
    warnings: &mut Vec<EagleLibraryImportWarning>,
) -> Vec<RepositoryShortcutPlan> {
    let Some(quick_access) = quick_access else {
        return Vec::new();
    };
    let Some(entries) = quick_access.as_array() else {
        warnings.push(
            warning("invalidQuickAccess")
                .with_reason("quickAccess is not a list")
                .into(),
        );
        return Vec::new();
    };
    let smart_by_source_id = smart_folders
        .iter()
        .map(|item| (item.source_id.clone(), item.smart_folder_id.clone()))
        .collect::<BTreeMap<_, _>>();
    let asset_by_source_id = assets
        .iter()
        .filter_map(|asset| {
            asset.memberships.first().map(|item| {
                (
                    asset.eagle_asset_id.clone(),
                    item.target_relative_path.clone(),
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    let mut plans = Vec::new();
    let mut used_ids = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let entry_value = if entry.is_string() {
            serde_json::json!({ "target": entry })
        } else {
            entry.clone()
        };
        let Some(entry) = entry_value.as_object() else {
            warnings.push(
                warning("invalidQuickAccess")
                    .with_index(index)
                    .with_reason("entry is not an object")
                    .into(),
            );
            continue;
        };
        let mut label = first_non_empty_string(entry, &["name", "label", "title"]);
        let raw_kind = first_non_empty_string(entry, &["type", "kind", "targetKind"]);
        let raw_target = first_non_empty_string(
            entry,
            &[
                "target",
                "targetId",
                "id",
                "path",
                "folderId",
                "smartFolderId",
                "assetId",
            ],
        );
        let Some(raw_target) = raw_target else {
            warnings.push(
                warning("invalidQuickAccess")
                    .with_index(index)
                    .with_reason("missing target")
                    .into(),
            );
            continue;
        };
        let mut target_kind = normalize_quick_access_kind(raw_kind.as_deref(), entry);
        let mut target_path = None;
        let mut target_id = None;
        if target_kind == "folder" {
            target_path = folder_index.get(&raw_target).cloned().or_else(|| {
                Some(prefix_relative_path(
                    parent_path,
                    &normalize_quick_access_path(&raw_target),
                ))
            });
        } else if target_kind == "smartFolder" {
            target_id = smart_by_source_id
                .get(&raw_target)
                .cloned()
                .or(Some(raw_target.clone()));
        } else if target_kind == "file" {
            target_path = asset_by_source_id.get(&raw_target).cloned().or_else(|| {
                Some(prefix_relative_path(
                    parent_path,
                    &normalize_quick_access_path(&raw_target),
                ))
            });
        } else if let Some(path) = folder_index.get(&raw_target) {
            target_kind = "folder".to_string();
            target_path = Some(path.clone());
        } else if let Some(id) = smart_by_source_id.get(&raw_target) {
            target_kind = "smartFolder".to_string();
            target_id = Some(id.clone());
        } else if let Some(path) = asset_by_source_id.get(&raw_target) {
            target_kind = "file".to_string();
            target_path = Some(path.clone());
        } else {
            let normalized =
                prefix_relative_path(parent_path, &normalize_quick_access_path(&raw_target));
            target_kind = if Path::new(&normalized).extension().is_some() {
                "file".to_string()
            } else {
                "folder".to_string()
            };
            target_path = Some(normalized);
        }
        if label.is_none() {
            label = Some(
                Path::new(
                    target_path
                        .as_deref()
                        .unwrap_or(target_id.as_deref().unwrap_or(&raw_target)),
                )
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(&raw_target)
                .to_string(),
            );
        }
        let label = label.unwrap_or_else(|| raw_target.clone());
        plans.push(RepositoryShortcutPlan {
            shortcut_id: unique_id(
                &format!("shortcut-{repo_id}-{raw_target}-{label}"),
                &mut used_ids,
            ),
            label,
            target_kind,
            target_path,
            target_id,
            sort_order: index as i64,
        });
    }
    plans
}

/// 构建 Eagle tagsGroups 与 starredTags 对应的标签组计划。
pub(super) fn build_tag_group_plans(
    repo_id: &str,
    tags_groups: Option<&Value>,
    tags_json: Option<&Value>,
    warnings: &mut Vec<EagleLibraryImportWarning>,
) -> Vec<TagGroupPlan> {
    let mut plans = Vec::new();
    let mut used_ids = BTreeSet::new();
    if let Some(tags_groups) = tags_groups {
        if let Some(groups) = tags_groups.as_array() {
            for (index, group) in groups.iter().enumerate() {
                if let Some(name) = normalize_non_empty_string(group) {
                    plans.push(TagGroupPlan {
                        tag_group_id: unique_id(
                            &format!("tag-group-{repo_id}-{name}"),
                            &mut used_ids,
                        ),
                        name,
                        tags: Vec::new(),
                        sort_order: index as i64,
                    });
                    continue;
                }
                let Some(group) = group.as_object() else {
                    warnings.push(
                        warning("invalidTagGroup")
                            .with_index(index)
                            .with_reason("entry is not an object")
                            .into(),
                    );
                    continue;
                };
                let name = first_non_empty_string(group, &["name", "label", "title"])
                    .unwrap_or_else(|| format!("Tag Group {}", index + 1));
                let tags = dedupe_preserve_order(normalize_text_values(
                    group
                        .get("tags")
                        .or_else(|| group.get("children"))
                        .or_else(|| group.get("items")),
                ));
                let source_id = group
                    .get("id")
                    .and_then(normalize_non_empty_string)
                    .unwrap_or_else(|| name.clone());
                plans.push(TagGroupPlan {
                    tag_group_id: unique_id(
                        &format!("tag-group-{repo_id}-{source_id}"),
                        &mut used_ids,
                    ),
                    name,
                    tags,
                    sort_order: index as i64,
                });
            }
        } else {
            warnings.push(
                warning("invalidTagGroups")
                    .with_reason("tagsGroups is not a list")
                    .into(),
            );
        }
    }
    let starred: Vec<String> = tags_json
        .and_then(Value::as_object)
        .and_then(|value| value.get("starredTags"))
        .map(|value| normalize_text_values(Some(value)))
        .unwrap_or_default();
    if !starred.is_empty() {
        plans.push(TagGroupPlan {
            tag_group_id: unique_id(&format!("tag-group-{repo_id}-starredTags"), &mut used_ids),
            name: "Starred Tags".to_string(),
            tags: dedupe_preserve_order(starred),
            sort_order: plans.len() as i64,
        });
    }
    plans
}

/// 构建 Eagle actions.json 对应的仓库动作计划。
pub(super) fn build_repository_action_plans(
    repo_id: &str,
    actions_json: Option<&Value>,
    warnings: &mut Vec<EagleLibraryImportWarning>,
) -> Vec<RepositoryActionPlan> {
    let Some(actions_json) = actions_json else {
        return Vec::new();
    };
    if actions_json.is_null() {
        return Vec::new();
    }
    let Some(actions) = actions_json.as_array() else {
        warnings.push(
            warning("invalidActions")
                .with_reason("actions.json is not a list")
                .into(),
        );
        return Vec::new();
    };
    let mut plans = Vec::new();
    let mut used_ids = BTreeSet::new();
    for (index, action) in actions.iter().enumerate() {
        let Some(action) = action.as_object() else {
            warnings.push(
                warning("invalidAction")
                    .with_index(index)
                    .with_reason("entry is not an object")
                    .into(),
            );
            continue;
        };
        let source_action_id = first_non_empty_string(action, &["id", "uuid", "actionId"]);
        let name = first_non_empty_string(action, &["name", "title", "label"])
            .unwrap_or_else(|| format!("Eagle Action {}", index + 1));
        let action_id = unique_id(
            &format!(
                "action-{repo_id}-{}-{name}",
                source_action_id
                    .clone()
                    .unwrap_or_else(|| index.to_string())
            ),
            &mut used_ids,
        );
        let steps_source = action
            .get("steps")
            .or_else(|| action.get("tasks"))
            .or_else(|| action.get("actions"))
            .or_else(|| action.get("items"))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        let mut steps =
            build_repository_action_step_plans(repo_id, &action_id, &steps_source, warnings);
        if steps.is_empty() {
            steps.push(unsupported_action_step_plan(
                repo_id,
                &action_id,
                0,
                Value::Object(action.clone()),
                "action has no recognizable steps",
            ));
        }
        let unsupported_reasons = steps
            .iter()
            .filter(|step| step.status != "ready")
            .filter_map(|step| step.unsupported_reason.clone())
            .collect::<Vec<_>>();
        let status = if unsupported_reasons.is_empty() {
            "ready"
        } else {
            "unsupported"
        };
        plans.push(RepositoryActionPlan {
            action_id,
            source_action_id,
            name,
            status: status.to_string(),
            enabled: status == "ready",
            raw: Value::Object(action.clone()),
            unsupported_reason: (!unsupported_reasons.is_empty())
                .then(|| unsupported_reasons.join("; ")),
            sort_order: index as i64,
            steps,
        });
    }
    plans
}

fn build_repository_action_step_plans(
    repo_id: &str,
    action_id: &str,
    steps_source: &Value,
    warnings: &mut Vec<EagleLibraryImportWarning>,
) -> Vec<RepositoryActionStepPlan> {
    let steps = if let Some(items) = steps_source.as_array() {
        items.clone()
    } else if steps_source.is_object() {
        vec![steps_source.clone()]
    } else {
        warnings.push(
            warning("invalidActionSteps")
                .with_reason("steps are not a list")
                .into(),
        );
        return Vec::new();
    };
    steps
        .into_iter()
        .enumerate()
        .map(|(index, step)| {
            if let Some(step_obj) = step.as_object() {
                convert_repository_action_step(repo_id, action_id, index, step_obj)
            } else {
                unsupported_action_step_plan(
                    repo_id,
                    action_id,
                    index,
                    step,
                    "step is not an object",
                )
            }
        })
        .collect()
}

fn convert_repository_action_step(
    repo_id: &str,
    action_id: &str,
    index: usize,
    step: &serde_json::Map<String, Value>,
) -> RepositoryActionStepPlan {
    let step_type = first_non_empty_string(step, &["type", "kind", "action", "command", "name"])
        .unwrap_or_default()
        .to_lowercase();
    let mut metadata = serde_json::Map::new();
    if matches!(step_type.as_str(), "rating" | "score" | "setrating") || step.contains_key("rating")
    {
        if let Some(value) = step
            .get("rating")
            .or_else(|| step.get("value"))
            .and_then(normalize_non_negative_number)
        {
            metadata.insert("rating".to_string(), value);
        }
    }
    if matches!(
        step_type.as_str(),
        "favorite" | "favourite" | "star" | "setfavorite"
    ) || step.contains_key("favorite")
        || step.contains_key("favourite")
    {
        let value = step
            .get("favorite")
            .or_else(|| step.get("favourite"))
            .or_else(|| step.get("value"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        metadata.insert("favorite".to_string(), Value::Bool(value));
    }
    if matches!(step_type.as_str(), "tag" | "tags" | "settags" | "addtags")
        || step.contains_key("tags")
    {
        let tags = dedupe_preserve_order(normalize_text_values(
            step.get("tags")
                .or_else(|| step.get("value"))
                .or_else(|| step.get("values")),
        ));
        return RepositoryActionStepPlan {
            step_id: action_step_id(
                repo_id,
                action_id,
                index,
                if step_type.is_empty() {
                    "tags"
                } else {
                    &step_type
                },
            ),
            step_kind: "tagGroups.set".to_string(),
            label: first_non_empty_string(step, &["label", "title"])
                .unwrap_or_else(|| "设置标签".to_string()),
            status: "ready".to_string(),
            config: serde_json::json!({ "tags": tags }),
            raw: Value::Object(step.clone()),
            unsupported_reason: None,
            sort_order: index as i64,
        };
    }
    if matches!(
        step_type.as_str(),
        "annotation" | "comment" | "note" | "setcomment"
    ) || step.contains_key("annotation")
        || step.contains_key("comment")
    {
        let value = first_non_empty_string(step, &["comment", "annotation", "note", "value"])
            .unwrap_or_default();
        metadata.insert("comment".to_string(), Value::String(value));
    }
    if matches!(step_type.as_str(), "url" | "link" | "setlink")
        || step.contains_key("url")
        || step.contains_key("link")
    {
        let value = first_non_empty_string(step, &["link", "url", "value"]).unwrap_or_default();
        metadata.insert("link".to_string(), Value::String(value));
    }
    if matches!(
        step_type.as_str(),
        "metadata" | "setmetadata" | "update_metadata"
    ) {
        if let Some(extra) = step.get("metadata").and_then(Value::as_object) {
            metadata.extend(extra.clone());
        }
    }
    if !metadata.is_empty() {
        return RepositoryActionStepPlan {
            step_id: action_step_id(
                repo_id,
                action_id,
                index,
                if step_type.is_empty() {
                    "metadata"
                } else {
                    &step_type
                },
            ),
            step_kind: "metadata.update".to_string(),
            label: first_non_empty_string(step, &["label", "title"])
                .unwrap_or_else(|| "更新元数据".to_string()),
            status: "ready".to_string(),
            config: Value::Object(serde_json::Map::from_iter([(
                "metadata".to_string(),
                Value::Object(metadata),
            )])),
            raw: Value::Object(step.clone()),
            unsupported_reason: None,
            sort_order: index as i64,
        };
    }
    let reason = if matches!(
        step_type.as_str(),
        "move" | "copy" | "rename" | "delete" | "trash" | "open" | "export" | "download"
    ) {
        format!("dangerous or external step requires native executor: {step_type}")
    } else {
        format!(
            "unsupported action step: {}",
            if step_type.is_empty() {
                "unknown"
            } else {
                &step_type
            }
        )
    };
    unsupported_action_step_plan(
        repo_id,
        action_id,
        index,
        Value::Object(step.clone()),
        &reason,
    )
}

fn unsupported_action_step_plan(
    repo_id: &str,
    action_id: &str,
    index: usize,
    raw: Value,
    reason: &str,
) -> RepositoryActionStepPlan {
    RepositoryActionStepPlan {
        step_id: action_step_id(repo_id, action_id, index, reason),
        step_kind: "unsupported".to_string(),
        label: format!("未支持步骤 {}", index + 1),
        status: "unsupported".to_string(),
        config: serde_json::json!({}),
        raw,
        unsupported_reason: Some(reason.to_string()),
        sort_order: index as i64,
    }
}

fn normalize_quick_access_kind(
    raw_kind: Option<&str>,
    entry: &serde_json::Map<String, Value>,
) -> String {
    let text = raw_kind
        .unwrap_or_default()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect::<String>();
    if matches!(text.as_str(), "folder" | "directory" | "dir") || entry.contains_key("folderId") {
        return "folder".to_string();
    }
    if matches!(text.as_str(), "smartfolder" | "smart" | "savedfilter")
        || entry.contains_key("smartFolderId")
    {
        return "smartFolder".to_string();
    }
    if matches!(text.as_str(), "file" | "asset" | "item") || entry.contains_key("assetId") {
        return "file".to_string();
    }
    String::new()
}

fn normalize_quick_access_path(value: &str) -> String {
    value.replace('\\', "/").trim_matches('/').to_string()
}

fn action_step_id(repo_id: &str, action_id: &str, index: usize, key: &str) -> String {
    slugify_ascii_component(&format!("action-step-{repo_id}-{action_id}-{index}-{key}"))
}
