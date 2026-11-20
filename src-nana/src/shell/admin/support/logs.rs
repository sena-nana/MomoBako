//! 日志级别、来源与位置标签，日志筛选、筛选计数与筛选值切换。

use super::*;

pub fn level_label(level: &str) -> String {
    match level {
        "debug" => "调试".into(),
        "info" => "信息".into(),
        "warn" => "警告".into(),
        "error" => "错误".into(),
        other => other.to_uppercase(),
    }
}

/// 来源种类的显示名。不认识的种类原样显示。
pub fn source_kind_label(kind: &str) -> String {
    match kind {
        "host" => "宿主".into(),
        "frontend-host" => "前端宿主".into(),
        "frontend-plugin" => "前端插件".into(),
        "backend-plugin" => "后端插件".into(),
        "helper" => "辅助进程".into(),
        other => other.to_string(),
    }
}

/// 位置标签：模块路径、文件和行号用冒号连起来，空的省掉。
pub fn location_label(record: &SystemLogRecord) -> String {
    [
        record.location.module_path.clone(),
        record.location.file.clone(),
        record.location.line.map(|line| line.to_string()),
    ]
    .into_iter()
    .flatten()
    .filter(|item| !item.is_empty())
    .collect::<Vec<_>>()
    .join(":")
}

pub fn log_matches(
    record: &SystemLogRecord,
    levels: &[String],
    kinds: &[String],
    plugin_id: &str,
    repo_id: &str,
    search: &str,
) -> bool {
    if !levels.is_empty() && !levels.iter().any(|level| level == &record.level) {
        return false;
    }
    if !kinds.is_empty() && !kinds.iter().any(|kind| kind == &record.source.kind) {
        return false;
    }
    if !plugin_id.is_empty() && record.source.plugin_id.as_deref() != Some(plugin_id) {
        return false;
    }
    if !repo_id.is_empty() && record.source.repo_id.as_deref() != Some(repo_id) {
        return false;
    }
    let keyword = search.trim().to_lowercase();
    if keyword.is_empty() {
        return true;
    }
    let location = [
        record.location.module_path.clone().unwrap_or_default(),
        record.location.file.clone().unwrap_or_default(),
        record
            .location
            .line
            .map(|line| line.to_string())
            .unwrap_or_default(),
    ]
    .join(":");
    let source = [
        record.source.kind.clone(),
        record.source.label.clone().unwrap_or_default(),
        record.source.plugin_id.clone().unwrap_or_default(),
        record.source.repo_id.clone().unwrap_or_default(),
    ]
    .join(" ");
    let haystack = [
        record.level.clone(),
        record.category.clone(),
        record.action.clone(),
        record.message.clone(),
        source,
        location,
        serde_json::to_string(&record.context).unwrap_or_else(|_| "{}".into()),
    ]
    .join("\n")
    .to_lowercase();
    haystack.contains(&keyword)
}

pub fn filtered_logs(
    records: &[SystemLogRecord],
    levels: &[String],
    kinds: &[String],
    plugin_id: &str,
    repo_id: &str,
    search: &str,
) -> Vec<SystemLogRecord> {
    let mut records = records
        .iter()
        .filter(|record| log_matches(record, levels, kinds, plugin_id, repo_id, search))
        .cloned()
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then(left.id.cmp(&right.id))
    });
    records
}

pub fn active_filter_count(
    levels: &[String],
    kinds: &[String],
    plugin_id: &str,
    repo_id: &str,
    search: &str,
) -> usize {
    levels.len()
        + kinds.len()
        + usize::from(!plugin_id.is_empty())
        + usize::from(!repo_id.is_empty())
        + usize::from(!search.trim().is_empty())
}

pub fn unique_sorted(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

pub fn toggle_value(items: &[String], value: &str) -> Vec<String> {
    if items.iter().any(|item| item == value) {
        items
            .iter()
            .filter(|item| item.as_str() != value)
            .cloned()
            .collect()
    } else {
        let mut next = items.to_vec();
        next.push(value.to_string());
        next
    }
}
