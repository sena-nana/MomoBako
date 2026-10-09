//! 设置、插件、日志和任务的纯函数。
//!
//! 这里只做分组、筛选、圆角钳制、字段规范化和文案，规则照 Vue 对应组件。
//! 服务调用留在调度器，失败不会被写成已保存。

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::backend::services::repository::{
    PlaylistPlayerContribution, PluginHookExecutionRecord, PluginManifest, RepositorySummary, SystemLogRecord,
    TaskProgressSnapshot,
};
use crate::settings;

use super::super::player::{
    builtin_audio_formats, capability_id, resolve_player, PlayerCandidate, AUDIO_CAPABILITY, AUDIO_SEQUENCE_TYPE,
};

pub(crate) use super::time::{hook_time_label, log_time_label, now_iso8601};

pub const CORNER_RADIUS_MIN: f64 = 0.0;
pub const CORNER_RADIUS_MAX: f64 = 20.0;
pub const DEFAULT_CORNER_RADIUS: f64 = 8.0;
pub const CORNER_STYLE_KEY: &str = "momobako.corners";
pub const CORNER_RADIUS_KEY: &str = "momobako.cornerRadius";
const PLUGIN_GROUP_ORDER: [&str; 6] = ["source", "library-kind", "parser", "preview", "service", "unclassified"];

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
    let value: Value = match serde_json::from_str(&raw) {
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

/// 下拉字段的一个选项。
#[derive(Clone, Debug, PartialEq)]
pub struct FieldOption {
    pub label: String,
    pub value: Value,
}

/// 插件声明的一个设置字段，只收 `contributes.settings.fields` 里有键和名称的项。
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub description: Option<String>,
    pub field_type: String,
    pub options: Vec<FieldOption>,
    pub default_value: Option<Value>,
    pub placeholder: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldChange {
    Set(Value),
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

/// 运行时芯片：原生动态库、Vue 模块、仅清单，其余是「未知运行时」。
pub fn plugin_runtime_label(runtime: &str) -> &'static str {
    match runtime {
        "native-dylib" => "原生动态库",
        "vue-module" => "Vue 模块",
        "manifest-only" => "仅清单",
        _ => "未知运行时",
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

/// 执行记录状态：成功、失败、已拦截，其余原样。
pub fn hook_status_label(status: &str) -> String {
    match status {
        "success" => "成功".into(),
        "failed" => "失败".into(),
        "blocked" => "已拦截".into(),
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

/// 插件声明的设置字段。和 Vue 一样只认 `contributes.settings.fields`，缺键或名称的跳过。
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
                    items
                        .iter()
                        .filter_map(|option| {
                            let value = option.get("value")?.clone();
                            let label = option.get("label").and_then(|item| item.as_str()).map(str::to_string).unwrap_or_else(|| option_wire(&value));
                            Some(FieldOption { label, value })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(ConfigField {
                key: key.to_string(),
                label: label.to_string(),
                description: field.get("description").and_then(|item| item.as_str()).map(str::to_string),
                field_type: field.get("type").and_then(|item| item.as_str()).unwrap_or("string").to_string(),
                options,
                default_value: field.get("default").cloned(),
                placeholder: field.get("placeholder").and_then(|item| item.as_str()).map(str::to_string),
            })
        })
        .collect()
}

/// 选项在下拉框里的值：`JSON.stringify(value)`。
pub fn option_wire(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// 插件设置区标题：`settingsPage.label`，来源插件是「账号与来源」，都没有是「插件设置」。
pub fn plugin_settings_label(plugin: &PluginManifest) -> String {
    settings_page_text(plugin, "label")
        .or_else(|| has_source_authentication(plugin).then(|| "账号与来源".to_string()))
        .unwrap_or_else(|| "插件设置".into())
}

/// 插件设置区说明：`settingsPage.description`，来源插件有默认说明。
pub fn plugin_settings_description(plugin: &PluginManifest) -> String {
    settings_page_text(plugin, "description")
        .or_else(|| has_source_authentication(plugin).then(|| "管理来源账号、登录状态与关联仓库。".to_string()))
        .unwrap_or_default()
}

fn settings_page_text(plugin: &PluginManifest, key: &str) -> Option<String> {
    plugin
        .contributes
        .get("settings")
        .and_then(|item| item.get("settingsPage"))
        .and_then(|item| item.get(key))
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

pub fn has_source_authentication(plugin: &PluginManifest) -> bool {
    plugin.contributes.get("source").and_then(|item| item.get("authentication")).is_some_and(|item| !item.is_null())
}

/// Vue `normalizeConfigFieldInput`。数字和选项的空值是重置。
pub fn normalize_field_input(field: &ConfigField, text: &str, checked: Option<bool>) -> FieldChange {
    if field.field_type == "boolean" {
        return FieldChange::Set(Value::Bool(checked.unwrap_or(false)));
    }
    if (field.field_type == "number" || field.field_type == "select") && text.trim().is_empty() {
        return FieldChange::Reset;
    }
    if field.field_type == "number" {
        let value: f64 = match text.trim().parse::<f64>() {
            Ok(value) if value.is_finite() => value,
            _ => return FieldChange::Invalid(format!("{} 不是有效数字。", field.label)),
        };
        return match serde_json::Number::from_f64(value).map(Value::Number) {
            Some(value) => FieldChange::Set(value),
            None => FieldChange::Invalid(format!("{} 不是有效数字。", field.label)),
        };
    }
    if field.field_type == "select" {
        let matched = field.options.iter().find(|option| option_wire(&option.value) == text);
        return FieldChange::Set(matched.map(|option| option.value.clone()).unwrap_or_else(|| Value::String(text.to_string())));
    }
    FieldChange::Set(Value::String(text.to_string()))
}

pub fn parse_json_draft(label: &str, draft: &str) -> Result<Value, String> {
    let draft = draft.trim();
    if draft.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(draft).map_err(|_| format!("{label} 不是有效 JSON。"))
}

pub fn json_draft_text(value: &Value) -> String {
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

/// 设置页里 Nana 内置解码器的名字。没有选中或官方的音频实现时，音频由它播放。
const BUILTIN_AUDIO_NAME: &str = "内置解码器";

#[derive(Clone, Debug, PartialEq)]
pub struct AudioChoice {
    /// 选项的值。内置解码器不是插件，值为空串，选它等于清掉偏好。
    pub plugin_id: String,
    pub label: String,
}

/// 设置页音频播放器下拉框的数据。
#[derive(Clone, Debug, PartialEq)]
pub struct AudioView {
    pub choices: Vec<AudioChoice>,
    /// 下拉框当前值：偏好，其次解析出的播放器；由内置解码器播放时为空串。
    pub selected: String,
    /// 有插件或原生候选实现了音频能力才能选，和 Vue 一样。
    pub selectable: bool,
    /// 回退提示（弱色）或缺失提示（危险色）。
    pub notice: Option<(String, bool)>,
}

/// 照 `Settings.vue`：候选是音频能力的实现，按名称排序；偏好的实现不在候选里时先放一项「（不可用）」。
/// 标签是「插件名 · 插件标识」。实现取自已启用插件清单的 `playlistPlayers`（Vue 注册表的来源，
/// Nana 用自己的音视频会话播放这些类型），以及登记了音频能力的原生候选。
///
/// 和 Vue 不同的一点：Nana 自带内存解码器（WAV、MP3、FLAC、Ogg，Windows 上还有媒体基础解的 M4A、AAC、Opus），
/// 没有选中也没有官方实现时音频由它播放。这时下拉框写内置解码器、回退提示指向它，不报「音频播放暂不可用」；
/// 连内置解码器都没有才照 Vue 报缺失。
pub fn audio_view(
    candidates: &[PlayerCandidate],
    plugins: &[PluginManifest],
    preferences: &BTreeMap<String, String>,
) -> AudioView {
    let implementations = audio_implementations(candidates, plugins);
    let resolution_candidates: Vec<PlayerCandidate> = implementations
        .iter()
        .map(|(candidate, _)| candidate)
        .filter(|candidate| candidate.player_type_id == AUDIO_SEQUENCE_TYPE)
        .cloned()
        .collect();
    let resolution = resolve_player(AUDIO_SEQUENCE_TYPE, &resolution_candidates, preferences);
    let builtin = builtin_audio_formats(candidates).map(|formats| format!("{BUILTIN_AUDIO_NAME} · {formats}"));
    let preferred = preferences.get(AUDIO_CAPABILITY).cloned();
    let mut choices = Vec::new();
    if let Some(plugin_id) = preferred.clone()
        && !implementations.iter().any(|(candidate, _)| candidate.plugin_id == plugin_id)
    {
        choices.push(AudioChoice { plugin_id: plugin_id.clone(), label: format!("{plugin_id}（不可用）") });
    }
    if resolution.player.is_none()
        && preferred.is_none()
        && let Some(label) = builtin.clone()
    {
        choices.push(AudioChoice { plugin_id: String::new(), label });
    }
    choices.extend(implementations.iter().map(|(candidate, name)| AudioChoice {
        plugin_id: candidate.plugin_id.clone(),
        label: format!("{name} · {}", candidate.plugin_id),
    }));
    let selected = preferred.unwrap_or_else(|| resolution.player.as_ref().map(|player| player.plugin_id.clone()).unwrap_or_default());
    let resolved_name = resolution
        .player
        .as_ref()
        .and_then(|player| implementations.iter().find(|(candidate, _)| candidate == player).map(|(_, name)| name.clone()));
    let notice = if resolution.fallback_used {
        let name = resolved_name
            .or_else(|| builtin.as_ref().map(|_| BUILTIN_AUDIO_NAME.to_string()))
            .unwrap_or_else(|| "官方默认实现".into());
        Some((format!("所选播放器当前不可用，已回退到 {name}。"), false))
    } else if resolution.player.is_none() && builtin.is_none() {
        Some(("官方音频播放器未启用或缺失，音频播放暂不可用。".into(), true))
    } else {
        None
    };
    AudioView { choices, selected, selectable: !implementations.is_empty(), notice }
}

/// 实现音频能力的播放器和它所属插件的名字，按 Vue 的标签、插件名、插件标识排序。
/// 插件只取启用且状态不是 error 的，和后端登记播放器类型的口径一致，也就是 Nana 真能放的那些。
fn audio_implementations(candidates: &[PlayerCandidate], plugins: &[PluginManifest]) -> Vec<(PlayerCandidate, String)> {
    let plugin_name = |plugin_id: &str| plugins.iter().find(|plugin| plugin.plugin_id == plugin_id).map(|plugin| plugin.name.clone());
    let mut found: Vec<(PlayerCandidate, String)> = candidates
        .iter()
        .filter(|candidate| capability_id(candidate) == AUDIO_CAPABILITY)
        .map(|candidate| (candidate.clone(), plugin_name(&candidate.plugin_id).unwrap_or_else(|| candidate.label.clone())))
        .collect();
    for plugin in plugins.iter().filter(|plugin| plugin.enabled && plugin.status != "error") {
        for candidate in contributed_players(plugin) {
            let known = found.iter().any(|(item, _)| item.plugin_id == candidate.plugin_id && item.player_type_id == candidate.player_type_id);
            if !known && capability_id(&candidate) == AUDIO_CAPABILITY {
                found.push((candidate, plugin.name.clone()));
            }
        }
    }
    found.sort_by(|(left, left_name), (right, right_name)| {
        left.label.cmp(&right.label).then(left_name.cmp(right_name)).then(left.plugin_id.cmp(&right.plugin_id))
    });
    found
}

/// 插件清单 `contributes.playlistPlayers` 里的播放器，带上清单里的 `capabilityId`。字段不全的条目记日志后跳过。
fn contributed_players(plugin: &PluginManifest) -> Vec<PlayerCandidate> {
    let Some(entries) = plugin.contributes.get("playlistPlayers").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| match serde_json::from_value::<PlaylistPlayerContribution>(entry.clone()) {
            Ok(player) => Some(PlayerCandidate {
                plugin_id: plugin.plugin_id.clone(),
                player_type_id: player.player_type_id,
                capability_id: entry.get("capabilityId").and_then(Value::as_str).map(str::to_string),
                label: player.label,
                file_class: player.file_class,
                extensions: player.supported_extensions,
                supports_seek: player.supports_seek,
                supports_volume: player.supports_volume,
            }),
            Err(error) => {
                eprintln!("Nana 插件 {} 的播放器条目无法读取：{error}", plugin.plugin_id);
                None
            }
        })
        .collect()
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

/// 来源种类的显示名。不认识的种类原样显示，和 Vue 一样。
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
    [record.location.module_path.clone(), record.location.file.clone(), record.location.line.map(|line| line.to_string())]
        .into_iter()
        .flatten()
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join(":")
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

/// 任务弹层的一行：资源库操作或运行中的任务。
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

/// 资源库操作放第一，其余按更新时间倒序。
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

/// 拓展页的工具：已启用、可加载前端模块的插件里 `contributes.toolPages` 的条目，
/// 按 `order`（缺省 100）、名称、标识排序，和 Vue 的注册表一致。
pub fn tool_pages_from_plugins(plugins: &[PluginManifest]) -> Vec<super::ToolPageEntry> {
    let mut pages = Vec::new();
    for plugin in plugins {
        let loadable = plugin.enabled
            && plugin.sdk == "frontend"
            && plugin.runtime == "vue-module"
            && plugin.entry.get("frontend").and_then(|item| item.get("module")).and_then(|item| item.as_str()).is_some_and(|text| !text.trim().is_empty())
            && !matches!(plugin.status.as_str(), "disabled" | "unavailable" | "error");
        if !loadable {
            continue;
        }
        let Some(entries) = plugin.contributes.get("toolPages").and_then(|item| item.as_array()) else {
            continue;
        };
        for entry in entries {
            let Some(id) = entry.get("toolPageId").and_then(|item| item.as_str()).map(str::trim).filter(|text| !text.is_empty()) else {
                eprintln!("Nana 插件 {} 的工具页缺少 toolPageId", plugin.plugin_id);
                continue;
            };
            let label = entry.get("label").and_then(|item| item.as_str()).unwrap_or(id).to_string();
            let order = entry.get("order").and_then(|item| item.as_f64()).unwrap_or(100.0);
            pages.push((
                order,
                super::ToolPageEntry {
                    id: id.to_string(),
                    label,
                    plugin_name: plugin.name.clone(),
                    description: entry.get("description").and_then(|item| item.as_str()).unwrap_or_default().to_string(),
                    native: is_builtin_tool_page(id),
                },
            ));
        }
    }
    pages.sort_by(|left, right| {
        left.0
            .partial_cmp(&right.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.1.label.cmp(&right.1.label))
            .then_with(|| left.1.id.cmp(&right.1.id))
    });
    pages.into_iter().map(|(_, page)| page).collect()
}
