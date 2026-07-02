//! Eagle Smart Folder 字段与条件归一化工具。
//!
//! 该模块把 Eagle 条件字段、操作符和值转换为 MomoBako smart folder filter 片段。

use super::super::*;
use super::planner::{normalize_non_empty_string, normalize_text_values, prefix_relative_path};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub(super) fn condition_field(condition: &Map<String, Value>) -> String {
    for key in ["field", "key", "type", "property", "name"] {
        if let Some(value) = condition.get(key).and_then(normalize_non_empty_string) {
            return value
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .flat_map(|ch| ch.to_lowercase())
                .collect();
        }
    }
    String::new()
}

pub(super) fn condition_values(condition: &Map<String, Value>) -> Value {
    for key in [
        "value",
        "values",
        "keyword",
        "keywords",
        "text",
        "folderId",
        "folderIds",
    ] {
        if let Some(value) = condition.get(key) {
            return value.clone();
        }
    }
    Value::Null
}

pub(super) fn condition_operator(condition: &Map<String, Value>) -> String {
    for key in ["operator", "op", "match", "matcher"] {
        if let Some(value) = condition.get(key).and_then(normalize_non_empty_string) {
            return value
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .flat_map(|ch| ch.to_lowercase())
                .collect();
        }
    }
    String::new()
}

pub(super) fn rating_from_condition(condition: &Map<String, Value>) -> Result<Option<f64>, String> {
    if !matches!(condition_field(condition).as_str(), "rating" | "score") {
        return Ok(None);
    }
    let operator = condition_operator(condition);
    if !operator.is_empty() && !matches!(operator.as_str(), "gte" | "min" | "atleast") {
        return Err(format!("unsupported rating operator: {operator}"));
    }
    let value = first_scalar_value(&condition_values(condition)).and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
    });
    Ok(value.filter(|value| *value > 0.0))
}

pub(super) fn direct_min_rating(filter_root: &Map<String, Value>) -> Result<Option<f64>, String> {
    for key in ["minRating", "rating"] {
        let Some(value) = filter_root.get(key) else {
            continue;
        };
        let value = first_scalar_value(value)
            .and_then(|value| {
                value
                    .as_f64()
                    .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
            })
            .ok_or_else(|| "rating value is not numeric".to_string())?;
        return Ok((value > 0.0).then_some(value));
    }
    Ok(None)
}

pub(super) fn direct_match_mode(filter_root: &Map<String, Value>) -> Option<String> {
    for key in ["match", "logic", "join", "combinator"] {
        let value = filter_root
            .get(key)
            .and_then(normalize_non_empty_string)?
            .to_lowercase();
        if matches!(value.as_str(), "or" | "any" | "some") {
            return Some("or".to_string());
        }
        if matches!(value.as_str(), "and" | "all") {
            return Some("and".to_string());
        }
    }
    None
}

pub(super) fn direct_sort(filter_root: &Map<String, Value>) -> Option<Value> {
    let sort_value = filter_root
        .get("sort")
        .or_else(|| filter_root.get("sortBy"))
        .or_else(|| filter_root.get("orderBy"))
        .or_else(|| filter_root.get("order"))?;
    let direction_value = filter_root
        .get("sortDirection")
        .or_else(|| filter_root.get("direction"))
        .or_else(|| filter_root.get("orderDirection"));
    let field = if let Some(field) = sort_value.as_str() {
        normalize_sort_field(field)
    } else if let Some(sort) = sort_value.as_object() {
        sort.get("field")
            .or_else(|| sort.get("key"))
            .and_then(normalize_non_empty_string)
            .and_then(|value| normalize_sort_field(&value))
    } else {
        None
    }?;
    let direction = direction_value
        .and_then(normalize_non_empty_string)
        .map(|value| value.to_lowercase())
        .filter(|value| matches!(value.as_str(), "asc" | "desc"))
        .or_else(|| {
            sort_value
                .as_object()
                .and_then(|sort| sort.get("direction"))
                .and_then(normalize_non_empty_string)
                .map(|value| value.to_lowercase())
                .filter(|value| matches!(value.as_str(), "asc" | "desc"))
        })
        .unwrap_or_else(|| "asc".to_string());
    Some(serde_json::json!({ "field": field, "direction": direction }))
}

pub(super) fn direct_limit(filter_root: &Map<String, Value>) -> Option<i64> {
    for key in ["limit", "maxResults", "maxCount"] {
        let value = filter_root.get(key)?;
        if let Some(number) = value.as_i64() {
            return (number > 0).then_some(number);
        }
        if let Some(text) = value
            .as_str()
            .and_then(|text| text.trim().parse::<i64>().ok())
        {
            return (text > 0).then_some(text);
        }
    }
    None
}

pub(super) fn add_direct_range_filter(
    filter_root: &Map<String, Value>,
    target: &mut Vec<Value>,
    key: &str,
    mapped_key: &str,
) {
    let min_key = format!("min{}", capitalize_ascii(key));
    let max_key = format!("max{}", capitalize_ascii(key));
    let value = range_filter_from_bounds(
        filter_root.get(&min_key),
        filter_root.get(&max_key),
        mapped_key,
    );
    if let Some(value) = value {
        target.push(value);
    }
}

pub(super) fn range_filter_from_condition(
    condition: &Map<String, Value>,
    key: &str,
) -> Result<Option<Value>, String> {
    let operator = condition_operator(condition);
    let value = condition_values(condition);
    let scalars = flatten_scalar_values(&value);
    let result = match operator.as_str() {
        "gte" | "min" | "atleast" | "gt" => range_filter_from_bounds(scalars.first(), None, key),
        "lte" | "max" | "lessthan" | "lt" => range_filter_from_bounds(None, scalars.first(), key),
        "between" | "range" => range_filter_from_bounds(scalars.first(), scalars.get(1), key),
        "" | "is" | "eq" | "equals" => {
            range_filter_from_bounds(scalars.first(), scalars.get(1).or(scalars.first()), key)
        }
        "not" | "notcontains" | "doesnotcontain" | "ne" | "neq" | "notequals" | "exclude" => {
            range_filter_from_bounds(scalars.first(), scalars.get(1), key)
        }
        other => return Err(format!("unsupported operator: {other}")),
    };
    Ok(result)
}

pub(super) fn numeric_filter_key(field_name: &str) -> &str {
    match field_name {
        "size" | "filesize" | "originalsizebytes" => "originalSizeBytes",
        _ => field_name,
    }
}

pub(super) fn date_filter_key(field_name: &str) -> &str {
    match field_name {
        "createdat" | "created" | "btime" => "fileCreatedAt",
        "modifiedat" | "mtime" | "updatedat" => "fileModifiedAt",
        _ => "addedToLibraryAt",
    }
}

pub(super) fn dedupe_range_filters(filters: Vec<Value>) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for item in filters {
        let marker = serde_json::to_string(&item).unwrap_or_default();
        if seen.insert(marker) {
            result.push(item);
        }
    }
    result
}

pub(super) fn dedupe_metadata_filters(filters: Vec<Value>) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for item in filters {
        let Some(map) = item.as_object() else {
            continue;
        };
        let Some(key) = map.get("key").and_then(normalize_non_empty_string) else {
            continue;
        };
        let Some(value) = map.get("value").and_then(normalize_non_empty_string) else {
            continue;
        };
        if seen.insert((key.clone(), value.clone())) {
            result.push(serde_json::json!({ "key": key, "value": value }));
        }
    }
    result
}

pub(super) fn normalize_format_values(value: Option<&Value>) -> Vec<String> {
    normalize_text_values(value)
        .into_iter()
        .map(|value| value.trim_start_matches('.').to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

pub(super) fn normalize_path_values(value: Option<&Value>, parent_path: &str) -> Vec<String> {
    normalize_text_values(value)
        .into_iter()
        .map(|value| {
            prefix_relative_path(
                parent_path,
                &value.replace('\\', "/").trim_matches('/').to_string(),
            )
        })
        .filter(|value| !value.is_empty())
        .collect()
}

pub(super) fn first_scalar_value(value: &Value) -> Option<Value> {
    flatten_scalar_values(value).into_iter().next()
}

fn normalize_sort_field(value: &str) -> Option<String> {
    let normalized = value
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '.')
        .flat_map(|ch| ch.to_lowercase())
        .collect::<String>();
    match normalized.as_str() {
        "name" | "filename" => Some("filename".to_string()),
        "path" | "folderpath" => Some("path".to_string()),
        "rating" => Some("rating".to_string()),
        "size" | "originalsizebytes" => Some("sizeBytes".to_string()),
        "modifiedat" | "updatedat" => Some("modifiedAt".to_string()),
        "width" => Some("metadata.width".to_string()),
        "height" => Some("metadata.height".to_string()),
        "createdat" | "filecreatedat" | "btime" => Some("metadata.fileCreatedAt".to_string()),
        "importedat" | "addedtolibraryat" => Some("metadata.addedToLibraryAt".to_string()),
        "random" => Some("random".to_string()),
        _ => None,
    }
}

fn range_filter_from_bounds(
    min_value: Option<&Value>,
    max_value: Option<&Value>,
    key: &str,
) -> Option<Value> {
    let mut map = Map::new();
    map.insert("key".to_string(), Value::String(key.to_string()));
    if let Some(min_value) = normalize_range_bound(min_value?) {
        map.insert(
            if key.starts_with("file") || key == "addedToLibraryAt" {
                "from"
            } else {
                "min"
            }
            .to_string(),
            min_value,
        );
    }
    if let Some(max_value) = max_value.and_then(normalize_range_bound) {
        map.insert(
            if key.starts_with("file") || key == "addedToLibraryAt" {
                "to"
            } else {
                "max"
            }
            .to_string(),
            max_value,
        );
    }
    (map.len() > 1).then_some(Value::Object(map))
}

fn normalize_range_bound(value: &Value) -> Option<Value> {
    if value.is_number() {
        return Some(value.clone());
    }
    if let Some(text) = value.as_str() {
        if let Ok(number) = text.trim().parse::<i64>() {
            return Some(Value::Number(number.into()));
        }
        if let Ok(number) = text.trim().parse::<f64>() {
            return Some(serde_json::json!(number));
        }
        if let Some(timestamp) = parse_date_bound(text) {
            return Some(Value::String(timestamp));
        }
    }
    None
}

fn parse_date_bound(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(number) = trimmed.parse::<f64>() {
        return if number > 0.0 {
            let seconds = if number.abs() > 10_000_000_000.0 {
                number / 1000.0
            } else {
                number
            };
            OffsetDateTime::from_unix_timestamp(seconds as i64)
                .ok()
                .and_then(|value| value.format(&Rfc3339).ok())
        } else {
            None
        };
    }
    OffsetDateTime::parse(
        &trimmed.replace('Z', "+00:00"),
        &time::format_description::well_known::Iso8601::DEFAULT,
    )
    .ok()
    .and_then(|value| value.format(&Rfc3339).ok())
}

fn flatten_scalar_values(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items.iter().flat_map(flatten_scalar_values).collect(),
        Value::Object(map) => {
            for key in ["id", "name", "value", "text"] {
                if let Some(value) = map.get(key) {
                    return flatten_scalar_values(value);
                }
            }
            Vec::new()
        }
        _ => vec![value.clone()],
    }
}

fn capitalize_ascii(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(ch) => ch.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}
