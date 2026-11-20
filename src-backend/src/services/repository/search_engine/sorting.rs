//! 搜索文本拼接、文件浏览排序与 JSON 值比较转换。

use super::*;

pub(in super::super) fn build_search_haystack(
    repo: &RepositorySummary,
    asset: &AssetSummary,
    metadata: &BTreeMap<String, serde_json::Value>,
) -> String {
    let metadata_values = metadata
        .iter()
        .map(|(key, value)| format!("{key} {}", json_value_to_search_text(value)))
        .collect::<Vec<_>>()
        .join(" ");

    [
        repo.name.as_str(),
        asset.filename.as_str(),
        asset.path.as_str(),
        asset.status.as_str(),
        &asset.tags.join(" "),
        metadata_values.as_str(),
    ]
    .join(" ")
    .to_lowercase()
}

pub(in super::super) fn sort_file_browser_entries(
    entries: &mut [FileBrowserEntry],
    sort: Option<&SearchSort>,
) {
    let Some(sort) = sort else {
        entries.sort_by(|left, right| left.path.to_lowercase().cmp(&right.path.to_lowercase()));
        return;
    };
    let field = sort.field.trim();
    let normalized_field = field.to_lowercase();
    if normalized_field == "random" {
        sort_by_random_key(
            entries,
            |entry| &entry.path,
            sort.direction.trim().eq_ignore_ascii_case("desc"),
        );
        return;
    }
    let descending = sort.direction.trim().eq_ignore_ascii_case("desc");
    entries.sort_by(|left, right| {
        let ordering =
            compare_sort_field(
                field,
                &left.metadata,
                &right.metadata,
                || match normalized_field.as_str() {
                    "filename" | "name" => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
                    "size" | "sizebytes" => left.size_bytes.cmp(&right.size_bytes),
                    "modified" | "modifiedat" => left.modified_at.cmp(&right.modified_at),
                    "rating" => metadata_sort_number(&left.metadata, "rating")
                        .partial_cmp(&metadata_sort_number(&right.metadata, "rating"))
                        .unwrap_or(std::cmp::Ordering::Equal),
                    _ => left.path.to_lowercase().cmp(&right.path.to_lowercase()),
                },
            );
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

pub(in super::super) fn sort_by_random_key<T>(
    items: &mut [T],
    key: impl Fn(&T) -> &str,
    descending: bool,
) {
    use std::collections::hash_map::DefaultHasher;

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    items.sort_by(|left, right| {
        let mut left_hasher = DefaultHasher::new();
        seed.hash(&mut left_hasher);
        key(left).hash(&mut left_hasher);
        let mut right_hasher = DefaultHasher::new();
        seed.hash(&mut right_hasher);
        key(right).hash(&mut right_hasher);
        let ordering = left_hasher.finish().cmp(&right_hasher.finish());
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

pub(in super::super) fn metadata_sort_number(
    metadata: &BTreeMap<String, serde_json::Value>,
    key: &str,
) -> f64 {
    metadata
        .get(key)
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0)
}

pub(in super::super) fn metadata_sort_field_key(field: &str) -> Option<&str> {
    const PREFIX: &str = "metadata.";
    if field.len() > PREFIX.len() && field[..PREFIX.len()].eq_ignore_ascii_case(PREFIX) {
        Some(&field[PREFIX.len()..])
    } else {
        None
    }
}

pub(in super::super) fn compare_sort_field(
    field: &str,
    left_metadata: &BTreeMap<String, serde_json::Value>,
    right_metadata: &BTreeMap<String, serde_json::Value>,
    fallback: impl FnOnce() -> std::cmp::Ordering,
) -> std::cmp::Ordering {
    if let Some(metadata_key) = metadata_sort_field_key(field) {
        compare_metadata_values(left_metadata, right_metadata, metadata_key)
    } else {
        fallback()
    }
}

pub(in super::super) fn compare_metadata_values(
    left: &BTreeMap<String, serde_json::Value>,
    right: &BTreeMap<String, serde_json::Value>,
    key: &str,
) -> std::cmp::Ordering {
    compare_optional_json_values(left.get(key), right.get(key))
}

pub(in super::super) fn compare_optional_json_values(
    left: Option<&serde_json::Value>,
    right: Option<&serde_json::Value>,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => compare_json_values(left, right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

pub(in super::super) fn compare_json_values(
    left: &serde_json::Value,
    right: &serde_json::Value,
) -> std::cmp::Ordering {
    if let (Some(left), Some(right)) = (json_value_to_f64(left), json_value_to_f64(right)) {
        return left
            .partial_cmp(&right)
            .unwrap_or(std::cmp::Ordering::Equal);
    }
    if let (Some(left), Some(right)) = (
        json_value_to_timestamp(left),
        json_value_to_timestamp(right),
    ) {
        return left.cmp(&right);
    }
    json_value_to_search_text(left)
        .to_lowercase()
        .cmp(&json_value_to_search_text(right).to_lowercase())
}

pub(in super::super) fn json_value_to_f64(value: &serde_json::Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        value
            .as_str()
            .map(str::trim)
            .and_then(|value| value.parse::<f64>().ok())
    })
}

pub(in super::super) fn json_value_to_timestamp(
    value: &serde_json::Value,
) -> Option<OffsetDateTime> {
    value
        .as_str()
        .map(str::trim)
        .and_then(parse_rfc3339_timestamp)
}

pub(in super::super) fn json_value_to_search_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

pub(in super::super) fn infer_value_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::String(_) => "string",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::Bool(_) => "boolean",
        _ => "json",
    }
}

pub(in super::super) fn parse_json_column(
    value_json: &str,
) -> Result<serde_json::Value, rusqlite::Error> {
    serde_json::from_str(value_json)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error)))
}

pub(in super::super) fn parse_json_column_optional(
    value_json: Option<String>,
) -> Result<serde_json::Value, rusqlite::Error> {
    match value_json {
        Some(value) => parse_json_column(&value),
        None => Ok(serde_json::json!({})),
    }
}

pub(in super::super) fn parse_json_column_nullable(
    value_json: Option<String>,
) -> Result<Option<serde_json::Value>, rusqlite::Error> {
    match value_json {
        Some(value) => Ok(Some(parse_json_column(&value)?)),
        None => Ok(None),
    }
}
