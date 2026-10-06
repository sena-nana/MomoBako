//! Eagle Smart Folder 转换器。
//!
//! 该模块把 Eagle 的 smartFolders 与 saved-filters 条件映射为 MomoBako
//! 当前可落盘的 smart folder filter 结构。

use super::super::*;
use super::planner::{
    dedupe_preserve_order, normalize_non_empty_string, normalize_text_values, warning,
    SmartFolderPlan,
};
use super::smart_folder_fields::{
    add_direct_range_filter, condition_field, condition_operator, condition_values,
    date_filter_key, dedupe_metadata_filters, dedupe_range_filters, direct_limit,
    direct_match_mode, direct_min_rating, direct_sort, normalize_format_values,
    normalize_path_values, numeric_filter_key, range_filter_from_condition, rating_from_condition,
};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub(super) fn build_smart_folder_plans(
    repo_id: &str,
    library_smart_folders: Option<&Value>,
    saved_filters: Option<&Value>,
    folder_index: &BTreeMap<String, String>,
    parent_path: &str,
    warnings: &mut Vec<EagleLibraryImportWarning>,
) -> Result<Vec<SmartFolderPlan>, String> {
    let mut plans = Vec::new();
    let mut used_ids = BTreeSet::new();
    for (source, entries_value) in [
        ("smartFolders", library_smart_folders),
        ("saved-filters", saved_filters),
    ] {
        let Some(entries_value) = entries_value else {
            continue;
        };
        let Some(entries) = entries_value.as_array() else {
            warnings.push(
                warning("skippedSmartFolder")
                    .with_reason("source is not a list")
                    .with_details(serde_json::json!({ "source": source }))
                    .into(),
            );
            continue;
        };
        for entry in entries {
            let Some(entry) = entry.as_object() else {
                warnings.push(
                    warning("skippedSmartFolder")
                        .with_reason("entry is not an object")
                        .with_details(serde_json::json!({ "source": source }))
                        .into(),
                );
                continue;
            };
            let source_id = smart_folder_source_id(entry, source, plans.len());
            let name = smart_folder_name(entry, &source_id);
            match convert_eagle_smart_folder_filter(entry, folder_index, parent_path) {
                Ok(filter) if !filter.is_null() => {
                    plans.push(SmartFolderPlan {
                        smart_folder_id: unique_smart_folder_id(
                            repo_id,
                            source,
                            &source_id,
                            &name,
                            &mut used_ids,
                        ),
                        source_id,
                        name,
                        filter,
                        sort_order: plans.len() as i64,
                    });
                }
                Ok(_) => {
                    warnings.push(
                        warning("skippedSmartFolder")
                            .with_reason("no equivalent MomoBako smart folder filter fields")
                            .with_details(serde_json::json!({
                                "source": source,
                                "sourceId": source_id,
                                "name": name,
                            }))
                            .into(),
                    );
                }
                Err(reason) => {
                    warnings.push(
                        warning("skippedSmartFolder")
                            .with_reason(&reason)
                            .with_details(serde_json::json!({
                                "source": source,
                                "sourceId": source_id,
                                "name": name,
                                "conditions": entry,
                            }))
                            .into(),
                    );
                }
            }
        }
    }
    Ok(plans)
}

fn smart_folder_source_id(entry: &Map<String, Value>, source: &str, index: usize) -> String {
    normalize_non_empty_string(
        entry
            .get("id")
            .or_else(|| entry.get("uuid"))
            .or_else(|| entry.get("smartFolderId"))
            .unwrap_or(&Value::Null),
    )
    .unwrap_or_else(|| format!("{source}-{index}"))
}

fn smart_folder_name(entry: &Map<String, Value>, source_id: &str) -> String {
    normalize_non_empty_string(
        entry
            .get("name")
            .or_else(|| entry.get("title"))
            .or_else(|| entry.get("label"))
            .unwrap_or(&Value::Null),
    )
    .unwrap_or_else(|| {
        format!(
            "Smart Folder {}",
            &source_id.chars().take(8).collect::<String>()
        )
    })
}

fn convert_eagle_smart_folder_filter(
    entry: &Map<String, Value>,
    folder_index: &BTreeMap<String, String>,
    parent_path: &str,
) -> Result<Value, String> {
    let filter_root = entry
        .get("filter")
        .and_then(Value::as_object)
        .unwrap_or(entry);
    if filter_root.keys().any(|key| {
        matches!(
            key.as_str(),
            "query"
                | "pathPrefix"
                | "excludeQuery"
                | "excludePathPrefixes"
                | "tags"
                | "formats"
                | "colors"
                | "shapes"
                | "metadataFilters"
                | "excludeTags"
                | "excludeFormats"
                | "excludeMetadataFilters"
                | "excludeNumberFilters"
                | "excludeDateFilters"
                | "numberFilters"
                | "dateFilters"
                | "minRating"
                | "matchMode"
                | "sort"
                | "limit"
        )
    }) {
        return Ok(Value::Object(filter_root.clone()));
    }
    let conditions = extract_smart_folder_conditions(filter_root)?;
    let mut query_values = Vec::new();
    let mut tag_values = Vec::new();
    let mut format_values = Vec::new();
    let mut path_values = Vec::new();
    let mut exclude_query_values = Vec::new();
    let mut exclude_path_values = Vec::new();
    let mut metadata_filters = Vec::<Value>::new();
    let mut exclude_tag_values = Vec::new();
    let mut exclude_format_values = Vec::new();
    let mut exclude_metadata_filters = Vec::<Value>::new();
    let mut exclude_number_filters = Vec::<Value>::new();
    let mut exclude_date_filters = Vec::<Value>::new();
    let mut number_filters = Vec::<Value>::new();
    let mut date_filters = Vec::<Value>::new();
    let mut min_rating = None::<f64>;

    apply_direct_smart_folder_fields(
        filter_root,
        folder_index,
        parent_path,
        &mut query_values,
        &mut tag_values,
        &mut format_values,
        &mut path_values,
        &mut exclude_query_values,
        &mut exclude_path_values,
        &mut metadata_filters,
        &mut exclude_tag_values,
        &mut exclude_format_values,
        &mut exclude_metadata_filters,
        &mut exclude_number_filters,
        &mut exclude_date_filters,
        &mut number_filters,
        &mut date_filters,
    )?;

    for condition in conditions {
        let Some(condition) = condition.as_object() else {
            return Err("filter condition is not an object".to_string());
        };
        apply_smart_folder_condition(
            condition,
            folder_index,
            parent_path,
            &mut query_values,
            &mut tag_values,
            &mut format_values,
            &mut path_values,
            &mut exclude_query_values,
            &mut exclude_path_values,
            &mut metadata_filters,
            &mut exclude_tag_values,
            &mut exclude_format_values,
            &mut exclude_metadata_filters,
            &mut exclude_number_filters,
            &mut exclude_date_filters,
            &mut number_filters,
            &mut date_filters,
        )?;
        if let Some(rating) = rating_from_condition(condition)? {
            min_rating = Some(min_rating.unwrap_or(0.0).max(rating));
        }
    }
    if let Some(rating) = direct_min_rating(filter_root)? {
        min_rating = Some(min_rating.unwrap_or(0.0).max(rating));
    }

    let mut filter_value = Map::new();
    if !query_values.is_empty() {
        filter_value.insert(
            "query".to_string(),
            Value::String(dedupe_preserve_order(query_values).join("\n")),
        );
    }
    if !path_values.is_empty() {
        filter_value.insert(
            "pathPrefix".to_string(),
            Value::String(dedupe_preserve_order(path_values).join("\n")),
        );
    }
    if !exclude_query_values.is_empty() {
        filter_value.insert(
            "excludeQuery".to_string(),
            Value::String(dedupe_preserve_order(exclude_query_values).join("\n")),
        );
    }
    if !exclude_path_values.is_empty() {
        filter_value.insert(
            "excludePathPrefixes".to_string(),
            Value::Array(
                dedupe_preserve_order(exclude_path_values)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if !tag_values.is_empty() {
        filter_value.insert(
            "tags".to_string(),
            Value::Array(
                dedupe_preserve_order(tag_values)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if !format_values.is_empty() {
        filter_value.insert(
            "formats".to_string(),
            Value::Array(
                dedupe_preserve_order(format_values)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if !metadata_filters.is_empty() {
        filter_value.insert(
            "metadataFilters".to_string(),
            Value::Array(dedupe_metadata_filters(metadata_filters)),
        );
    }
    if !exclude_tag_values.is_empty() {
        filter_value.insert(
            "excludeTags".to_string(),
            Value::Array(
                dedupe_preserve_order(exclude_tag_values)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if !exclude_format_values.is_empty() {
        filter_value.insert(
            "excludeFormats".to_string(),
            Value::Array(
                dedupe_preserve_order(exclude_format_values)
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if !exclude_metadata_filters.is_empty() {
        filter_value.insert(
            "excludeMetadataFilters".to_string(),
            Value::Array(dedupe_metadata_filters(exclude_metadata_filters)),
        );
    }
    if !exclude_number_filters.is_empty() {
        filter_value.insert(
            "excludeNumberFilters".to_string(),
            Value::Array(dedupe_range_filters(exclude_number_filters)),
        );
    }
    if !exclude_date_filters.is_empty() {
        filter_value.insert(
            "excludeDateFilters".to_string(),
            Value::Array(dedupe_range_filters(exclude_date_filters)),
        );
    }
    if !number_filters.is_empty() {
        filter_value.insert(
            "numberFilters".to_string(),
            Value::Array(dedupe_range_filters(number_filters)),
        );
    }
    if !date_filters.is_empty() {
        filter_value.insert(
            "dateFilters".to_string(),
            Value::Array(dedupe_range_filters(date_filters)),
        );
    }
    if let Some(min_rating) = min_rating {
        filter_value.insert("minRating".to_string(), serde_json::json!(min_rating));
    }
    if let Some(match_mode) = direct_match_mode(filter_root) {
        filter_value.insert("matchMode".to_string(), Value::String(match_mode));
    }
    if let Some(sort) = direct_sort(filter_root) {
        filter_value.insert("sort".to_string(), sort);
    }
    if let Some(limit) = direct_limit(filter_root) {
        filter_value.insert("limit".to_string(), Value::Number(limit.into()));
    }
    if filter_value.is_empty() {
        return Ok(Value::Null);
    }
    Ok(Value::Object(filter_value))
}

fn extract_smart_folder_conditions(filter_root: &Map<String, Value>) -> Result<Vec<Value>, String> {
    for key in ["conditions", "rules", "filters"] {
        if let Some(value) = filter_root.get(key) {
            return value
                .as_array()
                .cloned()
                .ok_or_else(|| "filter conditions are not a supported shape".to_string());
        }
    }
    Ok(Vec::new())
}

#[allow(clippy::too_many_arguments)]
fn apply_direct_smart_folder_fields(
    filter_root: &Map<String, Value>,
    folder_index: &BTreeMap<String, String>,
    parent_path: &str,
    query_values: &mut Vec<String>,
    tag_values: &mut Vec<String>,
    format_values: &mut Vec<String>,
    path_values: &mut Vec<String>,
    exclude_query_values: &mut Vec<String>,
    exclude_path_values: &mut Vec<String>,
    metadata_filters: &mut Vec<Value>,
    exclude_tag_values: &mut Vec<String>,
    exclude_format_values: &mut Vec<String>,
    exclude_metadata_filters: &mut Vec<Value>,
    exclude_number_filters: &mut Vec<Value>,
    exclude_date_filters: &mut Vec<Value>,
    number_filters: &mut Vec<Value>,
    date_filters: &mut Vec<Value>,
) -> Result<(), String> {
    for key in ["query", "keyword", "keywords", "text"] {
        query_values.extend(normalize_text_values(filter_root.get(key)));
    }
    for key in ["tags", "tag"] {
        tag_values.extend(normalize_text_values(filter_root.get(key)));
    }
    for key in ["formats", "format", "ext", "extensions"] {
        format_values.extend(normalize_format_values(filter_root.get(key)));
    }
    for key in ["folderId", "folderIds", "folders"] {
        for folder_id in normalize_text_values(filter_root.get(key)) {
            let Some(path) = folder_index.get(&folder_id) else {
                return Err(format!("unknown folder id: {folder_id}"));
            };
            path_values.push(path.clone());
        }
    }
    for key in ["pathPrefix", "path", "folderPath"] {
        path_values.extend(normalize_path_values(filter_root.get(key), parent_path));
    }
    let color_values = normalize_text_values(filter_root.get("colors"))
        .into_iter()
        .chain(normalize_text_values(filter_root.get("color")))
        .collect::<Vec<_>>();
    let shape_values = normalize_text_values(filter_root.get("shapes"))
        .into_iter()
        .chain(normalize_text_values(filter_root.get("shape")))
        .collect::<Vec<_>>();
    metadata_filters.extend(
        color_values
            .into_iter()
            .map(|value| serde_json::json!({ "key": "color", "value": value })),
    );
    metadata_filters.extend(
        shape_values
            .into_iter()
            .map(|value| serde_json::json!({ "key": "shape", "value": value })),
    );
    exclude_tag_values.extend(normalize_text_values(
        filter_root
            .get("excludeTags")
            .or_else(|| filter_root.get("excludedTags")),
    ));
    exclude_format_values.extend(normalize_format_values(
        filter_root
            .get("excludeFormats")
            .or_else(|| filter_root.get("excludedFormats")),
    ));
    exclude_query_values.extend(normalize_text_values(
        filter_root
            .get("excludeQuery")
            .or_else(|| filter_root.get("excludedQuery"))
            .or_else(|| filter_root.get("excludeKeyword"))
            .or_else(|| filter_root.get("excludedKeyword"))
            .or_else(|| filter_root.get("excludeText"))
            .or_else(|| filter_root.get("excludedText")),
    ));
    exclude_path_values.extend(normalize_path_values(
        filter_root
            .get("excludePathPrefixes")
            .or_else(|| filter_root.get("excludedPathPrefixes"))
            .or_else(|| filter_root.get("excludePath"))
            .or_else(|| filter_root.get("excludedPath"))
            .or_else(|| filter_root.get("excludeFolderPath"))
            .or_else(|| filter_root.get("excludedFolderPath")),
        parent_path,
    ));
    for key in [
        "excludeFolderIds",
        "excludedFolderIds",
        "excludeFolders",
        "excludedFolders",
    ] {
        for folder_id in normalize_text_values(filter_root.get(key)) {
            let Some(path) = folder_index.get(&folder_id) else {
                return Err(format!("unknown folder id: {folder_id}"));
            };
            exclude_path_values.push(path.clone());
        }
    }
    for key in ["excludeColors", "excludedColors"] {
        exclude_metadata_filters.extend(
            normalize_text_values(filter_root.get(key))
                .into_iter()
                .map(|value| serde_json::json!({ "key": "color", "value": value })),
        );
    }
    for key in ["excludeShapes", "excludedShapes"] {
        exclude_metadata_filters.extend(
            normalize_text_values(filter_root.get(key))
                .into_iter()
                .map(|value| serde_json::json!({ "key": "shape", "value": value })),
        );
    }
    add_direct_range_filter(filter_root, number_filters, "width", "width");
    add_direct_range_filter(filter_root, number_filters, "height", "height");
    add_direct_range_filter(filter_root, number_filters, "size", "originalSizeBytes");
    add_direct_range_filter(filter_root, date_filters, "createdAt", "fileCreatedAt");
    add_direct_range_filter(filter_root, date_filters, "modifiedAt", "fileModifiedAt");
    add_direct_range_filter(filter_root, date_filters, "importedAt", "addedToLibraryAt");
    add_direct_range_filter(filter_root, exclude_number_filters, "excludeWidth", "width");
    add_direct_range_filter(
        filter_root,
        exclude_number_filters,
        "excludedWidth",
        "width",
    );
    add_direct_range_filter(
        filter_root,
        exclude_number_filters,
        "excludeHeight",
        "height",
    );
    add_direct_range_filter(
        filter_root,
        exclude_number_filters,
        "excludedHeight",
        "height",
    );
    add_direct_range_filter(
        filter_root,
        exclude_number_filters,
        "excludeSize",
        "originalSizeBytes",
    );
    add_direct_range_filter(
        filter_root,
        exclude_number_filters,
        "excludedSize",
        "originalSizeBytes",
    );
    add_direct_range_filter(
        filter_root,
        exclude_date_filters,
        "excludeCreatedAt",
        "fileCreatedAt",
    );
    add_direct_range_filter(
        filter_root,
        exclude_date_filters,
        "excludedCreatedAt",
        "fileCreatedAt",
    );
    add_direct_range_filter(
        filter_root,
        exclude_date_filters,
        "excludeModifiedAt",
        "fileModifiedAt",
    );
    add_direct_range_filter(
        filter_root,
        exclude_date_filters,
        "excludedModifiedAt",
        "fileModifiedAt",
    );
    add_direct_range_filter(
        filter_root,
        exclude_date_filters,
        "excludeImportedAt",
        "addedToLibraryAt",
    );
    add_direct_range_filter(
        filter_root,
        exclude_date_filters,
        "excludedImportedAt",
        "addedToLibraryAt",
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_smart_folder_condition(
    condition: &Map<String, Value>,
    folder_index: &BTreeMap<String, String>,
    parent_path: &str,
    query_values: &mut Vec<String>,
    tag_values: &mut Vec<String>,
    format_values: &mut Vec<String>,
    path_values: &mut Vec<String>,
    exclude_query_values: &mut Vec<String>,
    exclude_path_values: &mut Vec<String>,
    metadata_filters: &mut Vec<Value>,
    exclude_tag_values: &mut Vec<String>,
    exclude_format_values: &mut Vec<String>,
    exclude_metadata_filters: &mut Vec<Value>,
    exclude_number_filters: &mut Vec<Value>,
    exclude_date_filters: &mut Vec<Value>,
    number_filters: &mut Vec<Value>,
    date_filters: &mut Vec<Value>,
) -> Result<(), String> {
    let operator = condition_operator(condition);
    let field_name = condition_field(condition);
    let values = condition_values(condition);
    if field_name.is_empty() || values.is_null() {
        return Ok(());
    }
    let negative = matches!(
        operator.as_str(),
        "not" | "notcontains" | "doesnotcontain" | "ne" | "neq" | "notequals" | "exclude"
    );
    match field_name.as_str() {
        "keyword" | "keywords" | "query" | "text" | "name" | "filename" => {
            (if negative {
                exclude_query_values
            } else {
                query_values
            })
            .extend(normalize_text_values(Some(&values)));
            Ok(())
        }
        "tag" | "tags" => {
            (if negative {
                exclude_tag_values
            } else {
                tag_values
            })
            .extend(normalize_text_values(Some(&values)));
            Ok(())
        }
        "format" | "formats" | "ext" | "extension" | "extensions" | "filetype" | "filetypes" => {
            (if negative {
                exclude_format_values
            } else {
                format_values
            })
            .extend(normalize_format_values(Some(&values)));
            Ok(())
        }
        "folder" | "folders" | "folderid" | "folderids" => {
            for folder_id in normalize_text_values(Some(&values)) {
                let Some(path) = folder_index.get(&folder_id) else {
                    return Err(format!("unknown folder id: {folder_id}"));
                };
                if negative {
                    exclude_path_values.push(path.clone());
                } else {
                    path_values.push(path.clone());
                }
            }
            Ok(())
        }
        "path" | "pathprefix" | "folderpath" => {
            (if negative {
                exclude_path_values
            } else {
                path_values
            })
            .extend(normalize_path_values(Some(&values), parent_path));
            Ok(())
        }
        "color" | "colors" => {
            let target = if negative {
                exclude_metadata_filters
            } else {
                metadata_filters
            };
            target.extend(
                normalize_text_values(Some(&values))
                    .into_iter()
                    .map(|value| serde_json::json!({ "key": "color", "value": value })),
            );
            Ok(())
        }
        "shape" | "shapes" => {
            let target = if negative {
                exclude_metadata_filters
            } else {
                metadata_filters
            };
            target.extend(
                normalize_text_values(Some(&values))
                    .into_iter()
                    .map(|value| serde_json::json!({ "key": "shape", "value": value })),
            );
            Ok(())
        }
        "rating" | "score" => Ok(()),
        "width" | "height" | "size" | "filesize" | "originalsizebytes" => {
            if let Some(number_filter) =
                range_filter_from_condition(condition, numeric_filter_key(&field_name))?
            {
                (if negative {
                    exclude_number_filters
                } else {
                    number_filters
                })
                .push(number_filter);
            }
            Ok(())
        }
        "createdat" | "created" | "btime" | "modifiedat" | "mtime" | "updatedat" | "importedat"
        | "addedat" => {
            if let Some(date_filter) =
                range_filter_from_condition(condition, date_filter_key(&field_name))?
            {
                (if negative {
                    exclude_date_filters
                } else {
                    date_filters
                })
                .push(date_filter);
            }
            Ok(())
        }
        _ => Err(format!("unsupported field: {field_name}")),
    }
}

fn unique_smart_folder_id(
    repo_id: &str,
    source: &str,
    source_id: &str,
    name: &str,
    used_ids: &mut BTreeSet<String>,
) -> String {
    let base = slugify_ascii_component(&format!("smart-{repo_id}-{source}-{source_id}-{name}"));
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
