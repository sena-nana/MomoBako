//! EagleLibrary 导入计划构建器。
//!
//! 该模块负责把 EagleLibrary 的 JSON 与素材目录解析为可执行的仓库导入计划，
//! 让宿主与原生插件共用同一套映射规则。

use super::super::*;
use super::repository_objects::{
    build_quick_access_plans, build_repository_action_plans, build_tag_group_plans,
};
use super::smart_folder::build_smart_folder_plans;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use time::PrimitiveDateTime;

const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

const EAGLE_CREATED_TIME_METADATA_KEYS: &[(&str, &str)] = &[
    ("btime", "fileCreatedAt"),
    ("createdAt", "fileCreatedAt"),
    ("birthtime", "fileCreatedAt"),
];

const EAGLE_NUMERIC_METADATA_KEYS: &[(&str, &str)] = &[
    ("width", "width"),
    ("height", "height"),
    ("size", "originalSizeBytes"),
];

#[derive(Debug, Clone)]
pub(super) struct FolderNode {
    pub(super) path: String,
    pub(super) protected: bool,
    pub(super) password_tip: Option<String>,
    pub(super) children: Vec<FolderNode>,
}

#[derive(Debug, Clone)]
pub(super) struct AssetMembership {
    pub(super) target_relative_path: String,
    pub(super) target_relative_dir: String,
    pub(super) target_filename: String,
    pub(super) role: String,
    pub(super) link_state: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct AssetPlan {
    pub(super) eagle_asset_id: String,
    pub(super) source_info_dir: PathBuf,
    pub(super) source_file: PathBuf,
    pub(super) source_thumbnail: Option<PathBuf>,
    pub(super) memberships: Vec<AssetMembership>,
    pub(super) display_title: String,
    pub(super) extension: String,
    pub(super) tags: Vec<String>,
    pub(super) note: Option<String>,
    pub(super) palette: Vec<String>,
    pub(super) is_deleted: bool,
    pub(super) preserved_metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
pub(super) struct RepositoryShortcutPlan {
    pub(super) shortcut_id: String,
    pub(super) label: String,
    pub(super) target_kind: String,
    pub(super) target_path: Option<String>,
    pub(super) target_id: Option<String>,
    pub(super) sort_order: i64,
}

#[derive(Debug, Clone)]
pub(super) struct RepositoryActionStepPlan {
    pub(super) step_id: String,
    pub(super) step_kind: String,
    pub(super) label: String,
    pub(super) status: String,
    pub(super) config: Value,
    pub(super) raw: Value,
    pub(super) unsupported_reason: Option<String>,
    pub(super) sort_order: i64,
}

#[derive(Debug, Clone)]
pub(super) struct RepositoryActionPlan {
    pub(super) action_id: String,
    pub(super) source_action_id: Option<String>,
    pub(super) name: String,
    pub(super) status: String,
    pub(super) enabled: bool,
    pub(super) raw: Value,
    pub(super) unsupported_reason: Option<String>,
    pub(super) sort_order: i64,
    pub(super) steps: Vec<RepositoryActionStepPlan>,
}

#[derive(Debug, Clone)]
pub(super) struct TagGroupPlan {
    pub(super) tag_group_id: String,
    pub(super) name: String,
    pub(super) tags: Vec<String>,
    pub(super) sort_order: i64,
}

#[derive(Debug, Clone)]
pub(super) struct SmartFolderPlan {
    pub(super) smart_folder_id: String,
    pub(super) source_id: String,
    pub(super) name: String,
    pub(super) filter: Value,
    pub(super) sort_order: i64,
}

#[derive(Debug, Clone)]
pub(super) struct ConversionPlan {
    pub(super) assets: Vec<AssetPlan>,
    pub(super) folder_nodes: Vec<FolderNode>,
    pub(super) quick_access: Vec<RepositoryShortcutPlan>,
    pub(super) repository_actions: Vec<RepositoryActionPlan>,
    pub(super) tag_groups: Vec<TagGroupPlan>,
    pub(super) smart_folders: Vec<SmartFolderPlan>,
    pub(super) warnings: Vec<EagleLibraryImportWarning>,
}

pub(super) fn build_conversion_plan(
    library_root: &Path,
    repo_id: &str,
    parent_path: &str,
) -> Result<ConversionPlan, String> {
    let library_metadata = load_json_value(&library_root.join("metadata.json"))?;
    let tags_json = load_json_value_or(
        &library_root.join("tags.json"),
        serde_json::json!({
            "historyTags": [],
            "starredTags": [],
        }),
    )?;
    let actions_json =
        load_json_value_or(&library_root.join("actions.json"), Value::Array(Vec::new()))?;
    let saved_filters_json = load_json_value_or(
        &library_root.join("saved-filters.json"),
        Value::Array(Vec::new()),
    )?;
    let mtime_json = load_json_value_or(&library_root.join("mtime.json"), serde_json::json!({}))?;

    let mut folder_index = BTreeMap::new();
    let mut warnings = Vec::new();
    let folder_nodes = build_folder_index(
        library_metadata.get("folders").and_then(Value::as_array),
        parent_path,
        &mut folder_index,
    )?;

    let mut assets = Vec::new();
    let mut output_name_usage = BTreeMap::<String, BTreeSet<String>>::new();
    let images_root = library_root.join("images");
    let mut info_dirs = fs::read_dir(&images_root)
        .map_err(io_error)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.ends_with(".info"))
        })
        .collect::<Vec<_>>();
    info_dirs.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));

    for info_dir in info_dirs {
        let metadata_path = info_dir.join("metadata.json");
        if !metadata_path.is_file() {
            continue;
        }
        let metadata = load_json_value(&metadata_path)?;
        if !metadata.is_object() {
            continue;
        }
        assets.push(build_asset_plan(
            &info_dir,
            &metadata,
            &mtime_json,
            &folder_index,
            parent_path,
            &mut output_name_usage,
            &mut warnings,
        )?);
    }

    let smart_folders = build_smart_folder_plans(
        repo_id,
        library_metadata.get("smartFolders"),
        Some(&saved_filters_json),
        &folder_index,
        parent_path,
        &mut warnings,
    )?;
    let quick_access = build_quick_access_plans(
        repo_id,
        library_metadata.get("quickAccess"),
        &folder_index,
        &smart_folders,
        &assets,
        parent_path,
        &mut warnings,
    );
    let repository_actions =
        build_repository_action_plans(repo_id, Some(&actions_json), &mut warnings);
    let tag_groups = build_tag_group_plans(
        repo_id,
        library_metadata.get("tagsGroups"),
        Some(&tags_json),
        &mut warnings,
    );

    Ok(ConversionPlan {
        assets,
        folder_nodes,
        quick_access,
        repository_actions,
        tag_groups,
        smart_folders,
        warnings,
    })
}

fn load_json_value(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(io_error)?;
    serde_json::from_str(&raw).map_err(json_error)
}

fn load_json_value_or(path: &Path, default: Value) -> Result<Value, String> {
    if path.exists() {
        load_json_value(path)
    } else {
        Ok(default)
    }
}

fn build_folder_index(
    folders: Option<&Vec<Value>>,
    parent_path: &str,
    folder_index: &mut BTreeMap<String, String>,
) -> Result<Vec<FolderNode>, String> {
    let Some(folders) = folders else {
        return Ok(Vec::new());
    };
    let mut nodes = Vec::new();
    let mut sibling_paths = BTreeSet::new();
    for folder in folders {
        let Some(folder) = folder.as_object() else {
            continue;
        };
        let folder_id = folder
            .get("id")
            .and_then(normalize_non_empty_string)
            .ok_or_else(|| "发现缺少 id 的 Eagle 文件夹。".to_string())?;
        let folder_name = folder
            .get("name")
            .and_then(normalize_non_empty_string)
            .unwrap_or_else(|| format!("folder-{}", safe_prefix(&folder_id, 8)));
        let safe_name = sanitize_segment(
            &folder_name,
            &format!("folder-{}", safe_prefix(&folder_id, 8)),
        );
        let unique_name = ensure_unique_segment(&safe_name, &mut sibling_paths, &folder_id);
        let path = join_relative_path(parent_path, &unique_name);
        folder_index.insert(folder_id.clone(), path.clone());
        let password_tip = first_non_empty_string(
            folder,
            &[
                "passwordTips",
                "passwordTip",
                "password_hint",
                "passwordHint",
            ],
        );
        let protected = folder
            .get("password")
            .and_then(normalize_non_empty_string)
            .is_some()
            || password_tip.is_some();
        let children = build_folder_index(
            folder.get("children").and_then(Value::as_array),
            &path,
            folder_index,
        )?;
        nodes.push(FolderNode {
            path,
            protected,
            password_tip,
            children,
        });
    }
    Ok(nodes)
}

fn build_asset_plan(
    info_dir: &Path,
    asset_metadata: &Value,
    mtime_json: &Value,
    folder_index: &BTreeMap<String, String>,
    parent_path: &str,
    output_name_usage: &mut BTreeMap<String, BTreeSet<String>>,
    warnings: &mut Vec<EagleLibraryImportWarning>,
) -> Result<AssetPlan, String> {
    let asset = asset_metadata
        .as_object()
        .ok_or_else(|| format!("invalid asset metadata: {}", info_dir.to_string_lossy()))?;
    let eagle_asset_id = asset
        .get("id")
        .and_then(normalize_non_empty_string)
        .ok_or_else(|| format!("{} 缺少素材 id。", info_dir.to_string_lossy()))?;
    let source_file = select_source_file(info_dir, asset)?;
    let source_thumbnail = select_thumbnail_file(info_dir)?;
    let extension = source_file
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let folder_ids = dedupe_preserve_order(
        asset
            .get("folders")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(normalize_non_empty_string)
            .collect(),
    );
    let mut target_dirs = Vec::<(String, String)>::new();
    if folder_ids.is_empty() {
        target_dirs.push((String::new(), parent_path.to_string()));
    } else {
        for folder_id in &folder_ids {
            if let Some(target_dir) = folder_index.get(folder_id) {
                target_dirs.push((folder_id.clone(), target_dir.clone()));
            } else {
                warnings.push(
                    warning("missingFolder")
                        .with_asset_id(&eagle_asset_id)
                        .with_folder_id(folder_id)
                        .into(),
                );
            }
        }
        if target_dirs.is_empty() {
            target_dirs.push((String::new(), parent_path.to_string()));
        }
    }

    let fallback_name = format!(
        "asset-{}.{}",
        safe_prefix(&eagle_asset_id, 8),
        if extension.is_empty() {
            "bin"
        } else {
            &extension
        }
    );
    let filename = sanitize_filename(
        &source_file
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("asset.bin"),
        &fallback_name,
    );
    let mut memberships = Vec::new();
    for (index, (folder_id, target_dir)) in target_dirs.iter().enumerate() {
        let directory_usage = output_name_usage.entry(target_dir.clone()).or_default();
        let member_filename = ensure_unique_filename(
            &filename,
            directory_usage,
            &format!(
                "{}-{}",
                eagle_asset_id,
                if folder_id.is_empty() {
                    index.to_string()
                } else {
                    folder_id.clone()
                }
            ),
        );
        memberships.push(AssetMembership {
            target_relative_path: join_relative_path(target_dir, &member_filename),
            target_relative_dir: target_dir.clone(),
            target_filename: member_filename,
            role: if index == 0 { "primary" } else { "alias" }.to_string(),
            link_state: None,
        });
    }
    if source_thumbnail.is_none() {
        warnings.push(
            warning("missingThumbnail")
                .with_asset_id(&eagle_asset_id)
                .with_details(serde_json::json!({ "sourceInfoDir": info_dir.to_string_lossy() }))
                .into(),
        );
    }
    let fallback_title = source_file
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("asset")
        .to_string();
    Ok(AssetPlan {
        eagle_asset_id: eagle_asset_id.clone(),
        source_info_dir: info_dir.to_path_buf(),
        source_file,
        source_thumbnail,
        memberships,
        display_title: asset
            .get("name")
            .and_then(normalize_non_empty_string)
            .unwrap_or(fallback_title),
        extension,
        tags: dedupe_preserve_order(
            asset
                .get("tags")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(normalize_non_empty_string)
                .collect(),
        ),
        note: asset.get("annotation").and_then(normalize_non_empty_string),
        palette: normalize_eagle_palette(asset.get("palettes")),
        is_deleted: asset
            .get("isDeleted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        preserved_metadata: build_preserved_metadata(
            &eagle_asset_id,
            info_dir,
            asset,
            mtime_json,
            warnings,
        ),
    })
}

fn build_preserved_metadata(
    asset_id: &str,
    info_dir: &Path,
    asset_metadata: &serde_json::Map<String, Value>,
    mtime_json: &Value,
    warnings: &mut Vec<EagleLibraryImportWarning>,
) -> BTreeMap<String, Value> {
    let mut preserved = BTreeMap::new();
    if let Some(link) = asset_metadata
        .get("url")
        .and_then(normalize_non_empty_string)
    {
        preserved.insert("link".to_string(), Value::String(link));
    }
    for (source_key, target_key) in [("importedAt", "addedToLibraryAt")]
        .into_iter()
        .chain(EAGLE_CREATED_TIME_METADATA_KEYS.iter().copied())
    {
        if preserved.contains_key(target_key) || !asset_metadata.contains_key(source_key) {
            continue;
        }
        if let Some(timestamp) = normalize_eagle_datetime(
            asset_metadata.get(source_key),
            asset_id,
            source_key,
            warnings,
        ) {
            preserved.insert(target_key.to_string(), Value::String(timestamp));
        }
    }
    let mtime_value = lookup_mtime_value(mtime_json, asset_id, info_dir);
    for (source_key, value) in [
        ("modifiedAt", asset_metadata.get("modifiedAt").cloned()),
        ("mtime", mtime_value),
    ] {
        let Some(value) = value else {
            continue;
        };
        if let Some(timestamp) =
            normalize_eagle_datetime(Some(&value), asset_id, source_key, warnings)
        {
            preserved.insert("fileModifiedAt".to_string(), Value::String(timestamp));
            break;
        }
    }
    for (source_key, target_key) in EAGLE_NUMERIC_METADATA_KEYS {
        if let Some(number) = asset_metadata
            .get(*source_key)
            .and_then(normalize_non_negative_number)
        {
            preserved.insert(target_key.to_string(), number);
        } else if asset_metadata.contains_key(*source_key) {
            warnings.push(
                warning("invalidEagleMetadataField")
                    .with_asset_id(asset_id)
                    .with_field(source_key)
                    .with_reason("expected a non-negative number")
                    .into(),
            );
        }
    }
    preserved
}

pub(super) fn flatten_folder_nodes(nodes: &[FolderNode]) -> Vec<FolderNode> {
    let mut flattened = Vec::new();
    for node in nodes {
        flattened.push(node.clone());
        flattened.extend(flatten_folder_nodes(&node.children));
    }
    flattened
}

pub(super) fn dedupe_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            result.push(value);
        }
    }
    result
}

pub(super) fn normalize_non_empty_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn normalize_text_values(value: Option<&Value>) -> Vec<String> {
    flatten_scalar_values(value)
        .into_iter()
        .filter_map(|item| match item {
            Value::String(text) => Some(text.trim().to_string()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
        .filter(|value| !value.is_empty())
        .collect()
}

fn flatten_scalar_values(value: Option<&Value>) -> Vec<Value> {
    let Some(value) = value else {
        return Vec::new();
    };
    match value {
        Value::Array(items) => items
            .iter()
            .flat_map(|item| flatten_scalar_values(Some(item)))
            .collect(),
        Value::Object(map) => {
            for key in ["id", "name", "value", "text"] {
                if let Some(value) = map.get(key) {
                    return flatten_scalar_values(Some(value));
                }
            }
            Vec::new()
        }
        _ => vec![value.clone()],
    }
}

pub(super) fn normalize_non_negative_number(value: &Value) -> Option<Value> {
    if let Some(number) = value.as_i64() {
        return (number >= 0).then(|| Value::Number(number.into()));
    }
    if let Some(number) = value.as_u64() {
        return Some(Value::Number(number.into()));
    }
    if let Some(number) = value.as_f64() {
        return (number >= 0.0).then(|| serde_json::json!(number));
    }
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Ok(number) = trimmed.parse::<i64>() {
            return (number >= 0).then(|| Value::Number(number.into()));
        }
        if let Ok(number) = trimmed.parse::<f64>() {
            return (number >= 0.0).then(|| serde_json::json!(number));
        }
    }
    None
}

fn normalize_eagle_palette(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    let mut colors = Vec::new();
    for item in items {
        let color_value = item
            .as_object()
            .and_then(|entry| entry.get("color"))
            .unwrap_or(item);
        if let Some(color) = normalize_hex_color(color_value) {
            if !colors.contains(&color) {
                colors.push(color);
            }
            if colors.len() == 5 {
                break;
            }
        }
    }
    colors
}

fn normalize_hex_color(value: &Value) -> Option<String> {
    let mut color = value.as_str()?.trim().trim_start_matches('#').to_string();
    if color.len() == 3 && color.chars().all(|ch| ch.is_ascii_hexdigit()) {
        color = color.chars().flat_map(|ch| [ch, ch]).collect();
    }
    if color.len() != 6 || !color.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", color.to_ascii_uppercase()))
}

fn select_source_file(
    info_dir: &Path,
    asset_metadata: &serde_json::Map<String, Value>,
) -> Result<PathBuf, String> {
    let logical_name = first_non_empty_string(asset_metadata, &["name"]).unwrap_or_default();
    let logical_ext = first_non_empty_string(asset_metadata, &["ext"]).unwrap_or_default();
    let expected_name = if logical_name.is_empty() || logical_ext.is_empty() {
        String::new()
    } else {
        format!("{logical_name}.{logical_ext}")
    };
    let candidates = fs::read_dir(info_dir)
        .map_err(io_error)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| path.file_name().and_then(|value| value.to_str()) != Some("metadata.json"))
        .filter(|path| !is_thumbnail_candidate(path))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(format!(
            "{} 未找到可转换的原始文件。",
            info_dir.to_string_lossy()
        ));
    }
    let exact_matches = candidates
        .iter()
        .filter(|path| {
            path.file_name().and_then(|value| value.to_str()) == Some(expected_name.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    if exact_matches.len() == 1 {
        return Ok(exact_matches[0].clone());
    }
    if candidates.len() == 1 {
        return Ok(candidates[0].clone());
    }
    let mut sized = candidates
        .into_iter()
        .map(|path| {
            let size = fs::metadata(&path).map_err(io_error)?.len();
            Ok((path, size))
        })
        .collect::<Result<Vec<_>, String>>()?;
    sized.sort_by(|left, right| right.1.cmp(&left.1));
    if sized.len() >= 2 && sized[0].1 > sized[1].1 {
        return Ok(sized[0].0.clone());
    }
    Err(format!("{} 原始文件存在歧义。", info_dir.to_string_lossy()))
}

fn select_thumbnail_file(info_dir: &Path) -> Result<Option<PathBuf>, String> {
    let mut thumbnails = fs::read_dir(info_dir)
        .map_err(io_error)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_thumbnail_candidate(path))
        .collect::<Vec<_>>();
    if thumbnails.is_empty() {
        return Ok(None);
    }
    thumbnails.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
    Ok(thumbnails.into_iter().next())
}

fn is_thumbnail_candidate(path: &Path) -> bool {
    path.file_stem()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.ends_with("_thumbnail"))
}

fn lookup_mtime_value(mtime_json: &Value, asset_id: &str, info_dir: &Path) -> Option<Value> {
    let source = mtime_json.as_object()?;
    for key in [
        asset_id,
        info_dir
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(""),
        info_dir
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(""),
    ] {
        let value = source.get(key)?;
        if let Some(map) = value.as_object() {
            for nested_key in ["modifiedAt", "mtime", "time", "value"] {
                if let Some(nested) = map.get(nested_key) {
                    return Some(nested.clone());
                }
            }
            return None;
        }
        return Some(value.clone());
    }
    None
}

fn normalize_eagle_datetime(
    value: Option<&Value>,
    asset_id: &str,
    source_key: &str,
    warnings: &mut Vec<EagleLibraryImportWarning>,
) -> Option<String> {
    let normalized = value.and_then(parse_eagle_datetime);
    if normalized.is_some() {
        return normalized;
    }
    warnings.push(
        warning("invalidEagleMetadataField")
            .with_asset_id(asset_id)
            .with_field(source_key)
            .with_reason("expected RFC3339 or Unix timestamp seconds/milliseconds")
            .into(),
    );
    None
}

fn parse_eagle_datetime(value: &Value) -> Option<String> {
    if value.is_null() || value.is_boolean() {
        return None;
    }
    if let Some(number) = value.as_f64() {
        return rfc3339_from_eagle_timestamp(number);
    }
    let text = value.as_str()?.trim().to_string();
    if text.is_empty() {
        return None;
    }
    if let Ok(number) = text.parse::<f64>() {
        return rfc3339_from_eagle_timestamp(number);
    }
    OffsetDateTime::parse(
        &text.replace('Z', "+00:00"),
        &time::format_description::well_known::Iso8601::DEFAULT,
    )
    .ok()
    .or_else(|| {
        PrimitiveDateTime::parse(
            &text,
            &time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]"),
        )
        .ok()
        .map(|value: PrimitiveDateTime| value.assume_utc())
    })
    .map(|value| value.format(&Rfc3339).unwrap_or_else(|_| now_rfc3339()))
}

fn rfc3339_from_eagle_timestamp(value: f64) -> Option<String> {
    if value <= 0.0 {
        return None;
    }
    let seconds = if value.abs() > 10_000_000_000.0 {
        value / 1000.0
    } else {
        value
    };
    OffsetDateTime::from_unix_timestamp(seconds as i64)
        .ok()
        .and_then(|value| value.format(&Rfc3339).ok())
}

pub(super) fn first_non_empty_string(
    map: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter()
        .filter_map(|key| map.get(*key))
        .find_map(normalize_non_empty_string)
}

pub(super) fn prefix_relative_path(parent_path: &str, path: &str) -> String {
    if parent_path.is_empty() {
        path.to_string()
    } else if path.is_empty() {
        parent_path.to_string()
    } else {
        format!("{parent_path}/{path}")
    }
}

fn sanitize_segment(value: &str, fallback: &str) -> String {
    let mut sanitized = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            _ => ch,
        })
        .collect::<String>()
        .trim()
        .trim_end_matches('.')
        .to_string();
    sanitized = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    if sanitized.is_empty() {
        sanitized = fallback.to_string();
    }
    if sanitized == REPO_META_DIR
        || sanitized == LEGACY_REPO_META_DIR
        || WINDOWS_RESERVED_NAMES.contains(&sanitized.to_ascii_uppercase().as_str())
    {
        sanitized = format!("_{sanitized}");
    }
    sanitized
}

fn sanitize_filename(filename: &str, fallback: &str) -> String {
    let source = Path::new(filename);
    let stem = sanitize_segment(
        source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(fallback),
        Path::new(fallback)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("asset"),
    );
    let suffix = source
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| {
            format!(
                ".{}",
                value
                    .chars()
                    .map(|ch| {
                        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                            || ch.is_control()
                        {
                            '_'
                        } else {
                            ch
                        }
                    })
                    .collect::<String>()
            )
        })
        .filter(|value| value != ".")
        .or_else(|| {
            Path::new(fallback)
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| format!(".{value}"))
        })
        .unwrap_or_default();
    format!("{stem}{suffix}")
}

fn ensure_unique_segment(
    candidate: &str,
    siblings: &mut BTreeSet<String>,
    stable_key: &str,
) -> String {
    if siblings.insert(candidate.to_string()) {
        return candidate.to_string();
    }
    let unique = format!("{candidate} ({})", safe_prefix(stable_key, 8));
    siblings.insert(unique.clone());
    unique
}

fn ensure_unique_filename(
    candidate: &str,
    siblings: &mut BTreeSet<String>,
    stable_key: &str,
) -> String {
    if siblings.insert(candidate.to_string()) {
        return candidate.to_string();
    }
    let path = Path::new(candidate);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(candidate);
    let suffix = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    let unique = format!("{stem} [{}]{suffix}", safe_prefix(stable_key, 8));
    siblings.insert(unique.clone());
    unique
}

fn join_relative_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

pub(super) fn unique_id(seed: &str, used_ids: &mut BTreeSet<String>) -> String {
    let base = slugify_ascii_component(seed);
    if used_ids.insert(base.clone()) {
        return base;
    }
    let mut index = 2;
    loop {
        let candidate = format!("{base}-{index}");
        if used_ids.insert(candidate.clone()) {
            return candidate;
        }
        index += 1;
    }
}

pub(super) fn warning(kind: &str) -> EagleLibraryImportWarningBuilder {
    EagleLibraryImportWarningBuilder {
        warning: EagleLibraryImportWarning {
            warning_type: kind.to_string(),
            ..EagleLibraryImportWarning::default()
        },
    }
}

pub(super) struct EagleLibraryImportWarningBuilder {
    warning: EagleLibraryImportWarning,
}

impl EagleLibraryImportWarningBuilder {
    pub(super) fn with_asset_id(mut self, value: &str) -> Self {
        self.warning.asset_id = Some(value.to_string());
        self
    }

    pub(super) fn with_field(mut self, value: &str) -> Self {
        self.warning.field = Some(value.to_string());
        self
    }

    pub(super) fn with_folder_id(mut self, value: &str) -> Self {
        self.warning.folder_id = Some(value.to_string());
        self
    }

    pub(super) fn with_index(mut self, value: usize) -> Self {
        self.warning.index = Some(value);
        self
    }

    pub(super) fn with_reason(mut self, value: &str) -> Self {
        self.warning.reason = Some(value.to_string());
        self
    }

    pub(super) fn with_details(mut self, value: Value) -> Self {
        self.warning.details = Some(value);
        self
    }
}

impl From<EagleLibraryImportWarningBuilder> for EagleLibraryImportWarning {
    fn from(value: EagleLibraryImportWarningBuilder) -> Self {
        value.warning
    }
}
