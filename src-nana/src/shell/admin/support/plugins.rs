//! 插件分类、标签、依赖与钩子状态、搜索分组，以及插件设置字段的生成与输入规范化。

use super::*;

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
    if PLUGIN_GROUP_ORDER[..5].contains(&category.as_str()) {
        category
    } else {
        "unclassified".into()
    }
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

/// 运行时芯片：清单认得的三种运行时各有中文名，其余是「未知运行时」。
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

pub(super) fn hook_executions<'a>(
    records: &'a [PluginHookExecutionRecord],
    plugin_id: &str,
) -> Vec<&'a PluginHookExecutionRecord> {
    records
        .iter()
        .filter(|record| record.plugin_id == plugin_id)
        .take(3)
        .collect()
}

pub fn plugin_search_text(
    plugin: &PluginManifest,
    records: &[PluginHookExecutionRecord],
) -> String {
    let mut parts = vec![
        plugin.name.clone(),
        plugin.description.clone(),
        plugin.plugin_id.clone(),
        plugin.disable_reason.clone().unwrap_or_default(),
        plugin.degradation_reason.clone().unwrap_or_default(),
        plugin
            .r#type
            .as_ref()
            .map(|item| item.layer.clone())
            .unwrap_or_default(),
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
    parts
        .into_iter()
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase()
}

pub fn filtered_plugins<'a>(
    plugins: &'a [PluginManifest],
    keyword: &str,
    records: &[PluginHookExecutionRecord],
) -> Vec<&'a PluginManifest> {
    let keyword = keyword.trim().to_lowercase();
    if keyword.is_empty() {
        return plugins.iter().collect();
    }
    plugins
        .iter()
        .filter(|plugin| plugin_search_text(plugin, records).contains(&keyword))
        .collect()
}

pub fn grouped_plugin_ids(
    plugins: &[PluginManifest],
    keyword: &str,
    records: &[PluginHookExecutionRecord],
) -> Vec<(String, Vec<String>)> {
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

/// 插件声明的设置字段。只认 `contributes.settings.fields`，缺键或名称的跳过。
pub fn settings_fields(plugin: &PluginManifest) -> Vec<ConfigField> {
    let Some(fields) = plugin
        .contributes
        .get("settings")
        .and_then(|item| item.get("fields"))
        .and_then(|item| item.as_array())
    else {
        return Vec::new();
    };
    fields
        .iter()
        .filter_map(|field| {
            let key = field
                .get("key")
                .and_then(|item| item.as_str())
                .unwrap_or("")
                .trim();
            let label = field
                .get("label")
                .and_then(|item| item.as_str())
                .unwrap_or("")
                .trim();
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
                            let label = option
                                .get("label")
                                .and_then(|item| item.as_str())
                                .map(str::to_string)
                                .unwrap_or_else(|| option_wire(&value));
                            Some(FieldOption { label, value })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(ConfigField {
                key: key.to_string(),
                label: label.to_string(),
                description: field
                    .get("description")
                    .and_then(|item| item.as_str())
                    .map(str::to_string),
                field_type: field
                    .get("type")
                    .and_then(|item| item.as_str())
                    .unwrap_or("string")
                    .to_string(),
                options,
                default_value: field.get("default").cloned(),
                placeholder: field
                    .get("placeholder")
                    .and_then(|item| item.as_str())
                    .map(str::to_string),
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
        .or_else(|| {
            has_source_authentication(plugin)
                .then(|| "管理来源账号、登录状态与关联仓库。".to_string())
        })
        .unwrap_or_default()
}

pub(super) fn settings_page_text(plugin: &PluginManifest, key: &str) -> Option<String> {
    plugin
        .contributes
        .get("settings")
        .and_then(|item| item.get("settingsPage"))
        .and_then(|item| item.get(key))
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

pub fn has_source_authentication(plugin: &PluginManifest) -> bool {
    plugin
        .contributes
        .get("source")
        .and_then(|item| item.get("authentication"))
        .is_some_and(|item| !item.is_null())
}

/// 规范化设置字段的输入。数字和选项的空值是重置。
pub fn normalize_field_input(
    field: &ConfigField,
    text: &str,
    checked: Option<bool>,
) -> FieldChange {
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
        let matched = field
            .options
            .iter()
            .find(|option| option_wire(&option.value) == text);
        return FieldChange::Set(
            matched
                .map(|option| option.value.clone())
                .unwrap_or_else(|| Value::String(text.to_string())),
        );
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

/// 字段现在的值：插件配置快照里有就用快照，否则用清单声明的默认值。
pub fn config_value<'a>(
    snapshots: &'a BTreeMap<String, crate::backend::services::repository::PluginConfigSnapshot>,
    plugin_id: &str,
    field: &'a ConfigField,
) -> Option<&'a Value> {
    snapshots
        .get(plugin_id)
        .and_then(|snapshot| snapshot.values.get(&field.key))
        .or(field.default_value.as_ref())
}

/// 单行字段里显示的文字：字符串原样，其它值写成 JSON 文本，空值为空。
pub fn field_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}
