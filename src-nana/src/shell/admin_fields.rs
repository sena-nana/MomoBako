//! 插件设置字段。
//!
//! 展开后按字段类型接到已有的配置消息。下载服务和缺方法名的来源认证
//! 不进字段编辑器，改由设置卡片画出。其它空字段页面仍只显示升级文案。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, Chip, LabeledValue, Stack, TextChanged, TextInput};
use serde_json::Value;

use super::admin::support::{self, ConfigField};
use super::admin::AdminMessage;
use super::{ShellMessage, ShellViewModel};

/// 当前插件已展开且声明了字段时，画出输入、重置和数据目录。
pub(super) fn field_rows(model: &ShellViewModel, plugin_id: &str) -> Vec<AnyView> {
    if model.admin.active_settings_plugin_id.as_deref() != Some(plugin_id) {
        return Vec::new();
    }
    let Some(plugin) = model.admin.plugins.iter().find(|plugin| plugin.plugin_id == plugin_id) else {
        return Vec::new();
    };
    if support::has_vue_settings_page(plugin) {
        return Vec::new();
    }
    let fields = support::settings_fields(plugin);
    if fields.is_empty() {
        return Vec::new();
    }
    let snapshot = model.admin.config_snapshots.get(plugin_id);
    let mut rows = Vec::new();
    for field in fields {
        let current = snapshot.and_then(|snapshot| snapshot.values.get(&field.key)).or(field.default_value.as_ref());
        rows.push(
            widget(LabeledValue::new(field.label.clone(), value_text(current)))
                .key(format!("admin-field-label-{plugin_id}-{}", field.key))
                .into_any(),
        );
        rows.push(field_editor(plugin_id, &field, current, model));
        let reset_id = plugin_id.to_string();
        let reset_key = field.key.clone();
        rows.push(
            widget(super::workbench::ghost_button(format!("重置 {}", field.label)))
                .key(format!("admin-field-reset-{plugin_id}-{}", field.key))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::ResetConfig {
                        plugin_id: reset_id.clone(),
                        key: reset_key.clone(),
                    }));
                })
                .into_any(),
        );
    }
    let directory_id = plugin_id.to_string();
    rows.push(
        widget(super::workbench::ghost_button("打开数据目录"))
            .key(format!("admin-field-directory-{plugin_id}"))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Admin(AdminMessage::OpenDataDirectory(directory_id.clone())));
            })
            .into_any(),
    );
    rows
}

/// 按字段类型选择布尔开关、选项、JSON 草稿或普通文本。
fn field_editor(plugin_id: &str, field: &ConfigField, current: Option<&Value>, model: &ShellViewModel) -> AnyView {
    if field.field_type == "boolean" {
        let checked = current.and_then(|value| value.as_bool()).unwrap_or(false);
        let id = plugin_id.to_string();
        let key = field.key.clone();
        return widget(super::workbench::ghost_button(if checked { "关闭" } else { "开启" }))
            .key(format!("admin-field-bool-{plugin_id}-{}", field.key))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Admin(AdminMessage::ConfigInput {
                    plugin_id: id.clone(),
                    key: key.clone(),
                    text: String::new(),
                    checked: Some(!checked),
                }));
            })
            .into_any();
    }
    if field.field_type == "select" && !field.options.is_empty() {
        let current_text = value_text(current);
        let options: Vec<AnyView> = field
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let id = plugin_id.to_string();
                let key = field.key.clone();
                let text = option_text(option);
                widget(Chip::new(text.clone()).selected(text == current_text))
                    .key(format!("admin-field-option-{plugin_id}-{}-{index}", field.key))
                    .on_cx(move |_, _: &Activate, cx| {
                        cx.dispatch_program(ShellMessage::Admin(AdminMessage::ConfigInput {
                            plugin_id: id.clone(),
                            key: key.clone(),
                            text: text.clone(),
                            checked: None,
                        }));
                    })
                    .into_any()
            })
            .collect();
        return widget(Stack::row(8.0).wrap(true)).children(options).into_any();
    }
    if field.field_type == "json" {
        let draft = model
            .admin
            .json_drafts
            .get(plugin_id)
            .and_then(|drafts| drafts.get(&field.key))
            .cloned()
            .unwrap_or_else(|| current.map(support::json_draft_text).unwrap_or_default());
        let id = plugin_id.to_string();
        let key = field.key.clone();
        let save_id = id.clone();
        let save_key = key.clone();
        return widget(Stack::column(6.0))
            .children((
                widget(TextInput::new(draft).label(field.label.clone())).on_cx(move |_, event: &TextChanged, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::JsonDraft {
                        plugin_id: id.clone(),
                        key: key.clone(),
                        value: event.value.to_string(),
                    }));
                }),
                widget(super::workbench::primary_button("保存 JSON")).key(format!("admin-field-json-{plugin_id}-{}", field.key)).on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Admin(AdminMessage::SaveJson {
                        plugin_id: save_id.clone(),
                        key: save_key.clone(),
                    }));
                }),
            ))
            .into_any();
    }
    let text_value = value_text(current);
    let id = plugin_id.to_string();
    let key = field.key.clone();
    widget(TextInput::new(text_value).label(field.label.clone()))
        .key(format!("admin-field-text-{plugin_id}-{}", field.key))
        .on_cx(move |_, event: &TextChanged, cx| {
            cx.dispatch_program(ShellMessage::Admin(AdminMessage::ConfigInput {
                plugin_id: id.clone(),
                key: key.clone(),
                text: event.value.to_string(),
                checked: None,
            }));
        })
        .into_any()
}

fn value_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

fn option_text(value: &Value) -> String {
    value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string())
}
