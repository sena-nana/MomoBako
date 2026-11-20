//! 显示名与相对路径处理、标签收集、元数据时间戳与 Eagle ID 生成等工具函数。

use super::*;

pub(super) fn split_display_name(name: &str) -> (String, String) {
    let path = Path::new(name);
    let display_name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(name)
        .to_string();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    (display_name, extension)
}

pub(super) fn split_relative_path(path: &str) -> Vec<String> {
    normalize_relative_path(path)
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

pub(super) fn normalize_relative_path(path: &str) -> String {
    path.trim().replace('\\', "/").trim_matches('/').to_string()
}

pub(super) fn parent_relative_path(path: &str) -> String {
    let normalized = normalize_relative_path(path);
    normalized
        .rfind('/')
        .map(|index| normalized[..index].to_string())
        .unwrap_or_default()
}

pub(super) fn join_relative_path(parent: &str, name: &str) -> String {
    let parent = normalize_relative_path(parent);
    let name = normalize_relative_path(name);
    if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
    }
}

pub(super) fn flatten_tags(value: &Value) -> Vec<String> {
    let mut tags = Vec::new();
    collect_tag_values(value, &mut tags);
    tags.sort();
    tags.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    tags
}

pub(super) fn collect_tag_values(value: &Value, tags: &mut Vec<String>) {
    match value {
        Value::String(tag) => {
            let tag = tag.trim();
            if !tag.is_empty() {
                tags.push(tag.to_string());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_tag_values(item, tags);
            }
        }
        Value::Object(map) => {
            for key in ["tags", "items", "children", "value"] {
                if let Some(value) = map.get(key) {
                    collect_tag_values(value, tags);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn ensure_array_mut<'a>(
    map: &'a mut Map<String, Value>,
    key: &str,
) -> &'a mut Vec<Value> {
    if !matches!(map.get(key), Some(Value::Array(_))) {
        map.insert(key.to_string(), Value::Array(Vec::new()));
    }
    map.get_mut(key)
        .and_then(Value::as_array_mut)
        .expect("json array should exist")
}

pub(super) fn info_dir_for_asset(repo_root: &Path, shared_asset_id: &str) -> PathBuf {
    repo_root
        .join("images")
        .join(format!("{shared_asset_id}.info"))
}

pub(super) fn touch_library_metadata(metadata: &mut Map<String, Value>) {
    metadata.insert(
        "modificationTime".to_string(),
        serde_json::json!(now_unix_millis()),
    );
}

pub(super) fn touch_asset_metadata(
    metadata: &mut Map<String, Value>,
    file_path: &Path,
) -> Result<(), String> {
    let stats = fs::metadata(file_path).map_err(io_error)?;
    metadata.insert(
        "size".to_string(),
        serde_json::json!(i64::try_from(stats.len()).unwrap_or(0)),
    );
    metadata.insert(
        "mtime".to_string(),
        serde_json::json!(stats
            .modified()
            .ok()
            .map(system_time_to_millis)
            .unwrap_or_else(now_unix_millis)),
    );
    metadata.insert(
        "modificationTime".to_string(),
        serde_json::json!(now_unix_millis()),
    );
    metadata.insert(
        "lastModified".to_string(),
        serde_json::json!(now_unix_millis()),
    );
    Ok(())
}

pub(super) fn generate_eagle_id(prefix: &str) -> String {
    format!(
        "{}{:X}",
        prefix
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .flat_map(|ch| ch
                .to_ascii_uppercase()
                .to_string()
                .chars()
                .collect::<Vec<_>>())
            .collect::<String>(),
        now_unix_millis()
    )
}

pub(super) fn now_unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

pub(super) fn system_time_to_millis(value: SystemTime) -> i64 {
    value
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_else(|_| now_unix_millis())
}

pub(super) fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

#[allow(dead_code)]
pub(super) fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}
