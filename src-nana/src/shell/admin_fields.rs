//! 插件设置表单。
//!
//! 照 `PluginManagerPanel.vue` 的 `.plugin-manager__settings-fields`：每个字段一行三列网格，
//! 左边粗体字段名和弱色说明，中间控件，右边「重置」。布尔字段是勾选框；选项字段是下拉框，
//! 第一项是空值；JSON 字段是等宽文本域；其余是单行输入，回车提交。

use std::sync::Arc;

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Checkbox, LengthSpec, Select, SelectChanged, SelectOption, Stack, TextArea, TextChanged, TextInput,
    TextSubmitted, ToggleChanged,
};
use nana_ui_core::{GridTrack, SemanticColorRole as Role};
use serde_json::Value;

use super::admin::style::{self, action, label, Tone};
use super::admin::support::{self, ConfigField};
use super::admin::AdminMessage;
use super::{ShellMessage, ShellViewModel};
use crate::backend::services::repository::PluginManifest;

/// 插件声明了字段时的表单。没有字段时不占位。
pub(super) fn fields_form(model: &ShellViewModel, plugin: &PluginManifest) -> Option<AnyView> {
    let fields = support::settings_fields(plugin);
    if fields.is_empty() {
        return None;
    }
    let keys = style::unique_keys(fields.iter().map(|field| field.key.as_str()));
    let rows = fields.iter().zip(&keys).map(|(field, key)| field_row(model, plugin, field, key)).collect::<Vec<_>>();
    Some(widget(style::column(10.0)).children(rows).key(format!("admin-fields-{}", style::key_part(&plugin.plugin_id))).into_any())
}

/// 一行字段。布尔字段的控件列不拉伸，其余字段中间列占满。
fn field_row(model: &ShellViewModel, plugin: &PluginManifest, field: &ConfigField, key: &str) -> AnyView {
    let plugin_id = plugin.plugin_id.as_str();
    let fkey = format!("{}-{key}", style::key_part(plugin_id));
    let boolean = field.field_type == "boolean";
    let columns = if boolean {
        vec![style::capped(140.0, 220.0), GridTrack::Auto, GridTrack::Auto]
    } else {
        vec![
            style::capped(140.0, 220.0),
            GridTrack::MinMax { min_px: 180.0, fr: 1.0, max_px: None },
            GridTrack::Auto,
        ]
    };
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(move |layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid_columns = Some(columns);
        layout.gap = Some(LengthSpec::Px(10.0));
        layout.width = Some(LengthSpec::Fill);
        layout.align_items = AlignSpec::Center;
    });
    let mut name = vec![widget(label(field.label.clone(), 13.0, 600, Role::Text)).key(format!("admin-field-label-{fkey}")).into_any()];
    if let Some(description) = field.description.as_deref().filter(|text| !text.is_empty()) {
        name.push(
            widget(style::pad(style::column(0.0), 3.0, 0.0, 0.0, 0.0))
                .children((widget(style::wrapping(style::label_lh(description, 12.0, 400, Role::Muted, 1.4))),))
                .into_any(),
        );
    }
    let reset_id = plugin_id.to_string();
    let reset_key = field.key.clone();
    let reset = widget(action("重置", None, Tone::Plain, model.admin.managing))
        .key(format!("admin-field-reset-{fkey}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ResetConfig { plugin_id: reset_id.clone(), key: reset_key.clone() }));
        })
        .into_any();
    widget(grid)
        .children((
            widget(Stack::column(0.0).min_width(LengthSpec::Px(0.0))).children(name),
            control(model, plugin_id, field, &fkey),
            reset,
        ))
        .key(format!("admin-field-{fkey}"))
        .into_any()
}

/// 当前值：配置快照里有就用快照，否则用声明的默认值。
fn current_value<'a>(model: &'a ShellViewModel, plugin_id: &str, field: &'a ConfigField) -> Option<&'a Value> {
    model
        .admin
        .config_snapshots
        .get(plugin_id)
        .and_then(|snapshot| snapshot.values.get(&field.key))
        .or(field.default_value.as_ref())
}

fn control(model: &ShellViewModel, plugin_id: &str, field: &ConfigField, fkey: &str) -> AnyView {
    let disabled = model.admin.managing;
    let current = current_value(model, plugin_id, field);
    let id = plugin_id.to_string();
    let key = field.key.clone();
    match field.field_type.as_str() {
        "boolean" => {
            let checked = current.is_some_and(truthy);
            let mut checkbox = Checkbox::new("", checked).disabled(disabled);
            {
                let layout = Arc::make_mut(&mut checkbox.style.layout);
                layout.width = Some(LengthSpec::Px(18.0));
                layout.height = Some(LengthSpec::Px(18.0));
            }
            widget(checkbox)
                .key(format!("admin-field-bool-{fkey}"))
                .on_cx(move |_, event: &ToggleChanged, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ConfigInput {
                        plugin_id: id.clone(),
                        key: key.clone(),
                        text: String::new(),
                        checked: Some(event.checked),
                    }));
                })
                .into_any()
        }
        "select" => {
            let mut options = vec![SelectOption::new("", "")];
            options.extend(field.options.iter().map(|option| SelectOption::new(support::option_wire(&option.value), option.label.clone())));
            let value = current.filter(|value| value.is_string() || value.is_number() || value.is_boolean()).map(support::option_wire).unwrap_or_default();
            let mut select = Select::new(Some(value)).options(options).disabled(disabled);
            {
                let layout = Arc::make_mut(&mut select.style.layout);
                layout.width = Some(LengthSpec::Fill);
                layout.min_width = Some(LengthSpec::Px(0.0));
                layout.height = Some(LengthSpec::Px(32.0));
                layout.font_size = Some(14.0);
            }
            widget(select)
                .key(format!("admin-field-select-{fkey}"))
                .on_cx(move |_, event: &SelectChanged, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ConfigInput {
                        plugin_id: id.clone(),
                        key: key.clone(),
                        text: event.value.to_string(),
                        checked: None,
                    }));
                })
                .into_any()
        }
        "json" => {
            let draft = model
                .admin
                .json_drafts
                .get(plugin_id)
                .and_then(|drafts| drafts.get(&field.key))
                .cloned()
                .unwrap_or_else(|| current.map(support::json_draft_text).unwrap_or_default());
            let mut area = TextArea::new(draft).placeholder(field.placeholder.clone().unwrap_or_default()).disabled(disabled);
            area.style = style::native_field_style(92.0);
            {
                let layout = Arc::make_mut(&mut area.style.layout);
                layout.height = None;
                layout.min_height = Some(LengthSpec::Px(92.0));
                layout.white_space_nowrap = false;
                layout.font_family = Some(style::MONO_FAMILY.into());
                layout.font_size = Some(13.0);
                layout.padding_top = Some(LengthSpec::Px(10.0));
                layout.padding_bottom = Some(LengthSpec::Px(10.0));
            }
            area.style.text_vertical_alignment = nana_ui::runtime::TextVerticalAlignment::Top;
            let submit_id = id.clone();
            let submit_key = key.clone();
            widget(area)
                .key(format!("admin-field-json-{fkey}"))
                .on_cx(move |_, event: &TextChanged, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::JsonDraft {
                        plugin_id: id.clone(),
                        key: key.clone(),
                        value: event.value.to_string(),
                    }));
                })
                .on_cx(move |_, _: &TextSubmitted, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SaveJson { plugin_id: submit_id.clone(), key: submit_key.clone() }));
                })
                .into_any()
        }
        _ => {
            let draft = model
                .admin
                .field_drafts
                .get(plugin_id)
                .and_then(|drafts| drafts.get(&field.key))
                .cloned()
                .unwrap_or_else(|| value_text(current));
            let input = style::native_input(
                TextInput::new(draft).label(field.label.clone()).placeholder(field.placeholder.clone().unwrap_or_default()).disabled(disabled),
            );
            let submit_id = id.clone();
            let submit_key = key.clone();
            widget(input)
                .key(format!("admin-field-text-{fkey}"))
                .on_cx(move |_, event: &TextChanged, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::FieldDraft {
                        plugin_id: id.clone(),
                        key: key.clone(),
                        value: event.value.to_string(),
                    }));
                })
                .on_cx(move |_, event: &TextSubmitted, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ConfigInput {
                        plugin_id: submit_id.clone(),
                        key: submit_key.clone(),
                        text: event.value.clone(),
                        checked: None,
                    }));
                })
                .into_any()
        }
    }
}

/// Vue `Boolean(value)`：空串、0、false、null 都是假。
fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Null => false,
        Value::Number(number) => number.as_f64().is_some_and(|item| item != 0.0),
        Value::String(text) => !text.is_empty(),
        _ => true,
    }
}

/// 输入框里的文字：字符串原样，其它值写成 JSON 文本，空值为空。
fn value_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}
