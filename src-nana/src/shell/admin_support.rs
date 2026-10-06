//! 设置、插件、日志和任务的纯函数。
//!
//! 这里只做分组、筛选、圆角钳制和文案。服务调用留在调度器，失败不会被写成已保存。

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::backend::services::repository::{
    PluginHookExecutionRecord, PluginManifest, RepositorySummary, SystemLogRecord, TaskProgressSnapshot,
};
use crate::settings;

use super::super::player::{resolution_notice, resolve_player, PlayerCandidate, AUDIO_CAPABILITY, AUDIO_SEQUENCE_TYPE};

pub const CORNER_RADIUS_MIN: f64 = 0.0;
pub const CORNER_RADIUS_MAX: f64 = 20.0;
pub const DEFAULT_CORNER_RADIUS: f64 = 8.0;
pub const CORNER_STYLE_KEY: &str = "momobako.corners";
pub const CORNER_RADIUS_KEY: &str = "momobako.cornerRadius";
const PLUGIN_GROUP_ORDER: [&str; 6] = ["source", "library-kind", "parser", "preview", "service", "unclassified"];

/// 桌面剪贴板走 Nana `OsClipboard`。保存和打开对话框走 Nana `OpenFileDialog`。
pub fn clipboard_available() -> bool {
    true
}

pub fn save_dialog_available() -> bool {
    true
}

pub fn open_dialog_available() -> bool {
    true
}

pub fn reveal_directory_available() -> bool {
    true
}

/// Windows 和 Linux 默认平滑，macOS 默认普通。未写入半径时用 MomoBako 的 8。
pub fn default_corner_style() -> &'static str {
    if cfg!(target_os = "macos") { "round" } else { "smooth" }
}

pub fn corners_path() -> PathBuf {
    settings::default_path().with_file_name("corners.json")
}

pub fn clamp_radius(value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    Some(value.clamp(CORNER_RADIUS_MIN, CORNER_RADIUS_MAX))
}

pub fn read_corners(path: &Path) -> (String, f64) {
    let fallback = (default_corner_style().to_string(), DEFAULT_CORNER_RADIUS);
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return fallback,
        Err(error) => {
            eprintln!("Nana 读取圆角偏好失败（{CORNER_STYLE_KEY}）：{error}");
            return fallback;
        }
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Nana 解析圆角偏好失败（{CORNER_STYLE_KEY}）：{error}");
            return fallback;
        }
    };
    let style = value
        .get(CORNER_STYLE_KEY)
        .and_then(|item| item.as_str())
        .filter(|item| *item == "smooth" || *item == "round")
        .map(str::to_owned)
        .unwrap_or_else(|| default_corner_style().to_string());
    let radius = value
        .get(CORNER_RADIUS_KEY)
        .and_then(|item| item.as_f64())
        .and_then(clamp_radius)
        .unwrap_or(DEFAULT_CORNER_RADIUS);
    (style, radius)
}

/// 原子写入圆角偏好。失败时删掉临时文件并记日志。
pub fn write_corners(path: &Path, style: &str, radius: f64) {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if let Err(error) = fs::create_dir_all(parent) {
        eprintln!("Nana 创建圆角偏好目录失败：{error}");
        return;
    }
    let value = serde_json::json!({
        CORNER_STYLE_KEY: style,
        CORNER_RADIUS_KEY: radius,
    });
    let raw = match serde_json::to_vec_pretty(&value) {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!("Nana 序列化圆角偏好失败：{error}");
            return;
        }
    };
    let temporary = path.with_extension("json.new");
    if let Err(error) = fs::write(&temporary, &raw) {
        eprintln!("Nana 写入圆角偏好失败：{error}");
        return;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        eprintln!("Nana 提交圆角偏好失败：{error}");
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub options: Vec<serde_json::Value>,
    pub default_value: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldChange {
    Set(serde_json::Value),
    Reset,
    Invalid(String),
}

pub fn plugin_category(plugin: &PluginManifest) -> String {
    let category = plugin.category.trim();
    if !category.is_empty() {
        return category.to_string();
    }
    match plugin.kind.as_str() {
        "filesystem" | "webdav" | "cloud" => "source".into(),
        "preview" => "preview".into(),
        "library-kind" => "library-kind".into(),
        "parser" => "parser".into(),
        _ => "service".into(),
    }
}

pub fn plugin_group(plugin: &PluginManifest) -> String {
    let category = plugin_category(plugin);
    if PLUGIN_GROUP_ORDER[..5].contains(&category.as_str()) { category } else { "unclassified".into() }
}

pub fn category_label(category: &str) -> &'static str {
    match category {
        "source" => "库来源",
        "library-kind" => "库类型",
        "parser" => "文件解析",
        "preview" => "预览渲染",
        "service" => "基础服务",
        _ => "未分类",
    }
}

pub fn plugin_source_label(source: &str) -> &'static str {
    match source {
        "user" => "用户插件",
        "system" => "系统插件",
        _ => "内置插件",
    }
}

pub fn plugin_status_label(plugin: &PluginManifest) -> &'static str {
    if plugin.status == "error" {
        "错误"
    } else if plugin.status == "unavailable" {
        "不可用"
    } else if plugin.degraded && plugin.enabled {
        "降级运行"
    } else if !plugin.enabled || plugin.status == "disabled" {
        "未启用"
    } else {
        "已启用"
    }
}

pub fn dependency_label(plugin: &PluginManifest) -> String {
    let required = if plugin.dependency_status.required.is_empty() {
        plugin.requires.len()
    } else {
        plugin.dependency_status.required.len()
    };
    let optional = if plugin.dependency_status.optional.is_empty() {
        plugin.optional.len()
    } else {
        plugin.dependency_status.optional.len()
    };
    if required == 0 && optional == 0 {
        "无依赖".into()
    } else {
        format!("必需 {required} / 可选 {optional}")
    }
}

pub fn dependency_status_label(status: &str) -> String {
    match status {
        "ready" => "可用".into(),
        "missing" => "缺失".into(),
        "disabled" => "未启用".into(),
        "unavailable" => "不可用".into(),
        "error" => "错误".into(),
        other => other.to_string(),
    }
}

pub fn can_delete_plugin(plugin: &PluginManifest) -> bool {
    plugin.source == "user"
}

fn hook_executions<'a>(records: &'a [PluginHookExecutionRecord], plugin_id: &str) -> Vec<&'a PluginHookExecutionRecord> {
    records.iter().filter(|record| record.plugin_id == plugin_id).take(3).collect()
}

pub fn plugin_search_text(plugin: &PluginManifest, records: &[PluginHookExecutionRecord]) -> String {
    let mut parts = vec![
        plugin.name.clone(),
        plugin.description.clone(),
        plugin.plugin_id.clone(),
        plugin.disable_reason.clone().unwrap_or_default(),
        plugin.degradation_reason.clone().unwrap_or_default(),
        plugin.r#type.as_ref().map(|item| item.layer.clone()).unwrap_or_default(),
        plugin.kind.clone(),
        plugin_category(plugin),
    ];
    parts.extend(plugin.capabilities.clone());
    parts.extend(plugin.permissions.clone());
    parts.extend(plugin.requires.clone());
    parts.extend(plugin.optional.clone());
    for hook in &plugin.hooks {
        parts.push(hook.slot.clone());
        parts.push(hook.action.clone());
        parts.push(hook.label.clone().unwrap_or_default());
    }
    for record in hook_executions(records, &plugin.plugin_id) {
        parts.push(record.status.clone());
        parts.push(record.message.clone());
        parts.push(record.hook_action.clone());
        parts.push(record.hook_slot.clone());
        parts.push(record.hook_label.clone().unwrap_or_default());
    }
    parts.into_iter().filter(|item| !item.is_empty()).collect::<Vec<_>>().join("\n").to_lowercase()
}

pub fn filtered_plugins<'a>(plugins: &'a [PluginManifest], keyword: &str, records: &[PluginHookExecutionRecord]) -> Vec<&'a PluginManifest> {
    let keyword = keyword.trim().to_lowercase();
    if keyword.is_empty() {
        return plugins.iter().collect();
    }
    plugins.iter().filter(|plugin| plugin_search_text(plugin, records).contains(&keyword)).collect()
}

pub fn grouped_plugin_ids(plugins: &[PluginManifest], keyword: &str, records: &[PluginHookExecutionRecord]) -> Vec<(String, Vec<String>)> {
    let filtered = filtered_plugins(plugins, keyword, records);
    PLUGIN_GROUP_ORDER
        .into_iter()
        .filter_map(|category| {
            let ids: Vec<String> = filtered
                .iter()
                .filter(|plugin| plugin_group(plugin) == category)
                .map(|plugin| plugin.plugin_id.clone())
                .collect();
            (!ids.is_empty()).then(|| (category.to_string(), ids))
        })
        .collect()
}

pub fn settings_fields(plugin: &PluginManifest) -> Vec<ConfigField> {
    let Some(fields) = plugin.contributes.get("settings").and_then(|item| item.get("fields")).and_then(|item| item.as_array()) else {
        return Vec::new();
    };
    fields
        .iter()
        .filter_map(|field| {
            let key = field.get("key").and_then(|item| item.as_str()).unwrap_or("").trim();
            let label = field.get("label").and_then(|item| item.as_str()).unwrap_or("").trim();
            if key.is_empty() || label.is_empty() {
                return None;
            }
            let options = field
                .get("options")
                .and_then(|item| item.as_array())
                .map(|items| {
                    items.iter().filter_map(|option| option.get("value").cloned()).collect()
                })
                .unwrap_or_default();
            Some(ConfigField {
                key: key.to_string(),
                label: label.to_string(),
                field_type: field.get("type").and_then(|item| item.as_str()).unwrap_or("string").to_string(),
                options,
                default_value: field.get("default").cloned(),
            })
        })
        .collect()
}

pub fn has_vue_settings_page(plugin: &PluginManifest) -> bool {
    let Some(settings) = plugin.contributes.get("settings") else {
        return false;
    };
    let has_page = settings.get("settingsPage").is_some_and(|item| !item.is_null());
    let has_fields = settings.get("fields").and_then(|item| item.as_array()).is_some_and(|items| !items.is_empty());
    has_page && !has_fields
}

pub fn has_source_authentication(plugin: &PluginManifest) -> bool {
    plugin.contributes.get("source").and_then(|item| item.get("authentication")).is_some_and(|item| !item.is_null())
}

/// 已有字段的设置页不提示升级。来源账号能读出 kind 和方法名时只给摘要，否则仍提示升级。
pub fn settings_upgrade_lines(plugin: &PluginManifest, marked_vue: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if marked_vue || has_vue_settings_page(plugin) {
        lines.push("插件设置页仍是 Vue 页面，需要升级为 Nana 原生设置字段".into());
    }
    if let Some(summary) = source_account_summary(plugin) {
        lines.push(summary);
    } else if has_source_authentication(plugin) {
        lines.push("账号与来源仍是 Vue 页面，需要升级为 Nana 原生设置".into());
    }
    lines
}

/// 认证声明同时有 kind，以及创建会话或状态方法时，拼一行摘要。
pub fn source_account_summary(plugin: &PluginManifest) -> Option<String> {
    let auth = plugin.contributes.get("source").and_then(|item| item.get("authentication"))?;
    if auth.is_null() {
        return None;
    }
    let kind = json_name(auth, "kind")?;
    let create = json_name(auth, "createSessionMethod");
    let status = json_name(auth, "statusMethod");
    if create.is_none() && status.is_none() {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(method) = create {
        parts.push(format!("创建会话 {method}"));
    }
    if let Some(method) = status {
        parts.push(format!("查询状态 {method}"));
    }
    Some(format!("来源账号 {kind}：{}。", parts.join("，")))
}

/// 读取认证对象上的方法名。空白不算有方法名。
pub fn named_auth_method(plugin: &PluginManifest, key: &str) -> Option<String> {
    let auth = plugin.contributes.get("source").and_then(|item| item.get("authentication"))?;
    if auth.is_null() {
        return None;
    }
    json_name(auth, key).map(str::to_string)
}

/// 登录调用成功后的提示。只附加字符串 message 和 status，不展开二维码。
pub fn source_auth_called_message(method: &str, payload: &serde_json::Value) -> String {
    let extras = ["message", "status"]
        .into_iter()
        .filter_map(|key| payload.get(key).and_then(|item| item.as_str()).map(str::trim).filter(|text| !text.is_empty()))
        .collect::<Vec<_>>();
    if extras.is_empty() {
        format!("已调用 {method}。")
    } else {
        format!("已调用 {method}。{}", extras.join(" "))
    }
}

fn json_name<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|item| item.as_str()).map(str::trim).filter(|item| !item.is_empty())
}

pub fn normalize_field_input(field: &ConfigField, text: &str, checked: Option<bool>) -> FieldChange {
    if field.field_type == "boolean" {
        return FieldChange::Set(serde_json::Value::Bool(checked.unwrap_or(false)));
    }
    if field.field_type == "number" || field.field_type == "select" {
        if text.trim().is_empty() {
            return FieldChange::Reset;
        }
    }
    if field.field_type == "number" {
        let value: f64 = match text.trim().parse::<f64>() {
            Ok(value) if value.is_finite() => value,
            _ => return FieldChange::Invalid(format!("{} 不是有效数字。", field.label)),
        };
        let number = serde_json::Number::from_f64(value).map(serde_json::Value::Number);
        return match number {
            Some(value) => FieldChange::Set(value),
            None => FieldChange::Invalid(format!("{} 不是有效数字。", field.label)),
        };
    }
    if field.field_type == "select" {
        let matched = field.options.iter().find(|option| serde_json::to_string(option).ok().as_deref() == Some(text));
        return FieldChange::Set(matched.cloned().unwrap_or_else(|| serde_json::Value::String(text.to_string())));
    }
    FieldChange::Set(serde_json::Value::String(text.to_string()))
}

pub fn parse_json_draft(label: &str, draft: &str) -> Result<serde_json::Value, String> {
    let draft = draft.trim();
    if draft.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(draft).map_err(|_| format!("{label} 不是有效 JSON。"))
}

pub fn json_draft_text(value: &serde_json::Value) -> String {
    if value.is_null() {
        return String::new();
    }
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackendCount {
    pub plugin_id: String,
    pub name: String,
    pub kind: String,
    pub count: usize,
}

pub fn backend_counts(repositories: &[RepositorySummary]) -> Vec<BackendCount> {
    let mut counts = Vec::<BackendCount>::new();
    for repository in repositories {
        if let Some(current) = counts.iter_mut().find(|item| item.plugin_id == repository.backend.plugin_id) {
            current.count += 1;
            continue;
        }
        counts.push(BackendCount {
            plugin_id: repository.backend.plugin_id.clone(),
            name: repository.backend.name.clone(),
            kind: repository.backend.kind.clone(),
            count: 1,
        });
    }
    counts
}

pub fn backend_summary(counts: &[BackendCount]) -> String {
    if counts.is_empty() {
        return "无".into();
    }
    counts.iter().map(|item| format!("{} ({})", item.name, item.count)).collect::<Vec<_>>().join(" / ")
}

pub fn external_status_label(ready: Option<bool>) -> &'static str {
    match ready {
        None => "未加载",
        Some(true) => "运行中",
        Some(false) => "未就绪",
    }
}

pub fn mask_token(token: Option<&str>) -> String {
    let Some(token) = token.filter(|value| !value.is_empty()) else {
        return "未加载".into();
    };
    let head = token.chars().take(10).collect::<String>();
    let tail_start = token.chars().count().saturating_sub(6);
    let tail = token.chars().skip(tail_start).collect::<String>();
    format!("{head}...{tail}")
}

pub fn connection_json(base_url: &str, token: &str, version: &str, started_at: &str) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "baseUrl": base_url,
        "token": token,
        "version": version,
        "startedAt": started_at,
    }))
    .unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioChoice {
    pub plugin_id: String,
    pub label: String,
    pub unavailable: bool,
}

/// 设置页的音频列表来自播放器候选，解析规则与播放条相同。
pub fn audio_choices(candidates: &[PlayerCandidate], preferences: &BTreeMap<String, String>) -> (Vec<AudioChoice>, String, String) {
    let mut implementations: Vec<&PlayerCandidate> = candidates.iter().filter(|candidate| candidate_capability(candidate) == AUDIO_CAPABILITY).collect();
    implementations.sort_by(|left, right| left.label.cmp(&right.label).then(left.plugin_id.cmp(&right.plugin_id)));
    let audio_candidates: Vec<PlayerCandidate> = implementations.iter().map(|item| (*item).clone()).collect();
    let resolution_candidates: Vec<PlayerCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.player_type_id == AUDIO_SEQUENCE_TYPE)
        .cloned()
        .collect();
    let resolution = resolve_player(AUDIO_SEQUENCE_TYPE, &resolution_candidates, preferences);
    let preferred = preferences.get(AUDIO_CAPABILITY).cloned();
    let mut choices = Vec::new();
    if let Some(plugin_id) = preferred.clone() {
        if !audio_candidates.iter().any(|candidate| candidate.plugin_id == plugin_id) {
            choices.push(AudioChoice { plugin_id: plugin_id.clone(), label: format!("{plugin_id}（不可用）"), unavailable: true });
        }
    }
    choices.extend(audio_candidates.iter().map(|candidate| AudioChoice {
        plugin_id: candidate.plugin_id.clone(),
        label: format!("{} · {}", candidate.label, candidate.plugin_id),
        unavailable: false,
    }));
    let selected = preferred.unwrap_or_else(|| resolution.player.as_ref().map(|player| player.plugin_id.clone()).unwrap_or_default());
    (choices, selected, resolution_notice(&resolution))
}

pub fn level_label(level: &str) -> String {
    match level {
        "debug" => "调试".into(),
        "info" => "信息".into(),
        "warn" => "警告".into(),
        "error" => "错误".into(),
        other => other.to_uppercase(),
    }
}

pub fn source_kind_label(kind: &str) -> &'static str {
    match kind {
        "host" => "宿主",
        "frontend-host" => "前端宿主",
        "frontend-plugin" => "前端插件",
        "backend-plugin" => "后端插件",
        "helper" => "辅助进程",
        _ => "",
    }
}

pub fn log_matches(record: &SystemLogRecord, levels: &[String], kinds: &[String], plugin_id: &str, repo_id: &str, search: &str) -> bool {
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
        record.location.line.map(|line| line.to_string()).unwrap_or_default(),
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

pub fn filtered_logs(records: &[SystemLogRecord], levels: &[String], kinds: &[String], plugin_id: &str, repo_id: &str, search: &str) -> Vec<SystemLogRecord> {
    let mut records = records
        .iter()
        .filter(|record| log_matches(record, levels, kinds, plugin_id, repo_id, search))
        .cloned()
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.timestamp.cmp(&right.timestamp).then(left.id.cmp(&right.id)));
    records
}

pub fn active_filter_count(levels: &[String], kinds: &[String], plugin_id: &str, repo_id: &str, search: &str) -> usize {
    levels.len() + kinds.len() + usize::from(!plugin_id.is_empty()) + usize::from(!repo_id.is_empty()) + usize::from(!search.trim().is_empty())
}

pub fn unique_sorted(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut values = values.into_iter().filter(|item| !item.is_empty()).collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

pub fn toggle_value(items: &[String], value: &str) -> Vec<String> {
    if items.iter().any(|item| item == value) {
        items.iter().filter(|item| item.as_str() != value).cloned().collect()
    } else {
        let mut next = items.to_vec();
        next.push(value.to_string());
        next
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PopoverRow {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub value: f64,
    pub indeterminate: bool,
    pub source: String,
    pub updated_at_ms: i64,
}

pub fn popover_rows(tasks: &[TaskProgressSnapshot], operation: Option<&super::OperationProgress>) -> Vec<PopoverRow> {
    let mut rows = Vec::new();
    if let Some(operation) = operation {
        rows.push(PopoverRow {
            id: "workspace-operation".into(),
            label: operation.label.clone(),
            detail: operation.detail.clone(),
            value: operation.value,
            indeterminate: operation.indeterminate,
            source: "资源库".into(),
            updated_at_ms: operation.updated_at_ms,
        });
    }
    rows.extend(tasks.iter().map(|task| PopoverRow {
        id: task.task_id.clone(),
        label: task.label.clone().unwrap_or_else(|| task.protocol_id.clone()),
        detail: task.error.clone().or(task.phase.clone()).unwrap_or_default(),
        value: f64::from(task.percent.unwrap_or(0.0)),
        indeterminate: task.percent.is_none(),
        source: "任务".into(),
        updated_at_ms: task.updated_at.parse().unwrap_or(0),
    }));
    rows.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
    rows
}

pub fn action_status_label(status: &str, enabled: bool) -> &'static str {
    if status != "ready" {
        "不支持"
    } else if !enabled {
        "已停用"
    } else {
        "可执行"
    }
}

pub fn action_can_run(status: &str, enabled: bool, selected_count: usize, running: bool) -> bool {
    status == "ready" && enabled && selected_count > 0 && !running
}

pub fn apply_tool_page_selection(pages: &[super::ToolPageEntry], active: Option<&str>) -> Option<String> {
    if pages.is_empty() {
        return None;
    }
    if active.is_some_and(|id| pages.iter().any(|page| page.id == id)) {
        return active.map(str::to_string);
    }
    Some(pages[0].id.clone())
}

pub const TOOL_FILE_MANAGER: &str = "momobako.tool.file-manager";
pub const TOOL_EAGLE_IMPORTER: &str = "momobako.tool.eagle-importer";
pub const TOOL_API_PLAYGROUND: &str = "momobako.tool.api-playground";

pub fn is_builtin_tool_page(id: &str) -> bool {
    matches!(id, TOOL_FILE_MANAGER | TOOL_EAGLE_IMPORTER | TOOL_API_PLAYGROUND)
}

/// 三个内置工具页已经是原生页。其它非原生页仍提示升级。
pub fn tool_page_upgrade(page: &super::ToolPageEntry) -> Option<&'static str> {
    if page.native || is_builtin_tool_page(&page.id) {
        return None;
    }
    Some("工具页仍是 Vue 插件，需要升级为 Nana 原生工具页")
}

pub fn audio_picker_enabled(choices: &[AudioChoice]) -> bool {
    choices.iter().any(|choice| !choice.unavailable)
}

fn candidate_capability(candidate: &PlayerCandidate) -> String {
    candidate
        .capability_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if candidate.player_type_id == AUDIO_SEQUENCE_TYPE {
                AUDIO_CAPABILITY.into()
            } else {
                format!("playlist-player:{}", candidate.player_type_id)
            }
        })
}
