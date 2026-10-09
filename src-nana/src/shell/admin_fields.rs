//! 插件设置表单。
//!
//! 照 `PluginManagerPanel.vue` 的 `.plugin-manager__settings-fields`：每个字段一行三列网格，
//! 左边粗体字段名和弱色说明，中间控件，右边「重置」。布尔字段是勾选框；选项字段是下拉框，
//! 第一项是空值；JSON 字段是等宽文本域；其余是单行输入，回车提交。
//!
//! 字段行按「字段键 + 控件类型」做键，只在展开设置的那张插件卡片里建一次；勾选、选项、禁用和
//! 文字都是绑定。单行输入和 JSON 文本域的草稿在行里建 `ModelField`，ViewModel 的草稿或配置值
//! 变了才写回，组合输入和刚打的字不被冲掉。

use std::sync::Arc;

use nana_ui::runtime::view::{fields, widget, AnyView, IntoView, Item, Store, StoreList, StorePath};
use nana_ui::runtime::{
    Activate, AlignSpec, Checkbox, LengthSpec, Select, SelectChanged, SelectOption, Stack, TextArea, TextChanged, TextInput,
    TextSubmitted, ToggleChanged,
};
use nana_ui_core::{GridTrack, SemanticColorRole as Role};
use serde_json::Value;

use super::admin::bind::{row_draft, row_flag, row_text, ActionDisabled};
use super::admin::style::{self, action, label, Tone};
use super::admin::support::{self, ConfigField};
use super::admin::AdminMessage;
use super::{ShellMessage, ShellViewModel};
use crate::backend::services::repository::PluginManifest;

/// 字段的控件类型。类型变了整行重建。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum FieldKind {
    Boolean,
    Select,
    Json,
    Text,
}

/// 一行字段要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FieldRowView {
    pub plugin_id: String,
    pub field_key: String,
    pub kind: FieldKind,
    pub label: String,
    pub description: String,
    pub placeholder: String,
    /// 勾选框的勾选态。
    pub checked: bool,
    /// 下拉框的选项（第一项是空值）和当前值。
    pub options: Vec<(String, String)>,
    pub selected: String,
    /// 单行输入和 JSON 文本域显示的文字：有草稿用草稿，否则是当前值。
    pub text: String,
    /// 插件管理在途时控件和「重置」禁用。
    pub disabled: bool,
}

impl FieldRowView {
    /// `plugin` 声明的全部字段。没有字段时为空，表单不占位。
    pub(crate) fn project_all(model: &ShellViewModel, plugin: &PluginManifest) -> Vec<Self> {
        support::settings_fields(plugin).iter().map(|field| Self::project(model, plugin, field)).collect()
    }

    fn project(model: &ShellViewModel, plugin: &PluginManifest, field: &ConfigField) -> Self {
        let plugin_id = plugin.plugin_id.as_str();
        let current = current_value(model, plugin_id, field);
        let kind = match field.field_type.as_str() {
            "boolean" => FieldKind::Boolean,
            "select" => FieldKind::Select,
            "json" => FieldKind::Json,
            _ => FieldKind::Text,
        };
        let drafts = match kind {
            FieldKind::Json => model.admin.json_drafts.get(plugin_id),
            _ => model.admin.field_drafts.get(plugin_id),
        };
        let text = drafts.and_then(|drafts| drafts.get(&field.key)).cloned().unwrap_or_else(|| match kind {
            FieldKind::Json => current.map(support::json_draft_text).unwrap_or_default(),
            _ => value_text(current),
        });
        let mut options = vec![(String::new(), String::new())];
        options.extend(field.options.iter().map(|option| (support::option_wire(&option.value), option.label.clone())));
        Self {
            plugin_id: plugin_id.to_string(),
            field_key: field.key.clone(),
            kind,
            label: field.label.clone(),
            description: field.description.clone().unwrap_or_default(),
            placeholder: field.placeholder.clone().unwrap_or_default(),
            checked: current.is_some_and(truthy),
            options,
            selected: current.filter(|value| value.is_string() || value.is_number() || value.is_boolean()).map(support::option_wire).unwrap_or_default(),
            text,
            disabled: model.admin.managing,
        }
    }

    /// 行的键：字段键加控件类型。
    pub(crate) fn key(row: &FieldRowView) -> (String, FieldKind) {
        (row.field_key.clone(), row.kind)
    }
}

/// 字段行在 Store 里的句柄。
type FieldItem = Item<Store<Vec<FieldRowView>>, (String, FieldKind), FieldRowView>;

/// 插件声明了字段时的表单；没有字段时不占布局。`plugin_id` 是展开了设置的这张卡片的插件。
pub(super) fn fields_form(plugin_id: &str, fields: Store<Vec<FieldRowView>>) -> AnyView {
    fields
        .keyed(FieldRowView::key)
        .each(field_row)
        .gap(10.0)
        .visible(move || !fields.is_empty())
        .key(format!("admin-fields-{}", style::key_part(plugin_id)))
        .into_any()
}

/// 一行字段。布尔字段的控件列不拉伸，其余字段中间列占满。行的身份建行时就定了。
fn field_row(item: FieldItem) -> AnyView {
    let row = item.get_untracked();
    let fkey = format!("{}-{}", style::key_part(&row.plugin_id), style::key_part(&row.field_key));
    // 不用 `style::capped`：中间列有 180 的下限，第一列权重过大时同一轮会把中间列冻在下限。
    // 两列等权重，宽度够时第一列先停在 220，余下都给中间列。
    let columns = if row.kind == FieldKind::Boolean {
        vec![GridTrack::MinMax { min_px: 140.0, fr: 1.0, max_px: Some(220.0) }, GridTrack::Auto, GridTrack::Auto]
    } else {
        vec![
            GridTrack::MinMax { min_px: 140.0, fr: 1.0, max_px: Some(220.0) },
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
    let caption = nana_ui::runtime::view::node_ref();
    let name = widget(Stack::column(0.0).min_width(LengthSpec::Px(0.0))).children((
        style::bound(label(String::new(), 13.0, 600, Role::Text), row_text(item, |row| &row.label))
            .node_ref(caption)
            .key(format!("admin-field-label-{fkey}")),
        widget(style::pad(style::column(0.0), 3.0, 0.0, 0.0, 0.0))
            .visible(row_flag(item, |row| !row.description.is_empty()))
            .children((style::bound(style::wrapping(style::label_lh(String::new(), 12.0, 400, Role::Muted, 1.4)), row_text(item, |row| &row.description)),)),
    ));
    let (reset_id, reset_key) = (row.plugin_id.clone(), row.field_key.clone());
    let reset = widget(action("重置", None, Tone::Plain, false))
        .prop::<bool, ActionDisabled>(row_flag(item, |row| row.disabled))
        .key(format!("admin-field-reset-{fkey}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ResetConfig { plugin_id: reset_id.clone(), key: reset_key.clone() }));
        })
        .into_any();
    widget(grid)
        .children((name, control(item, &row, &fkey, caption), reset))
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

/// 字段的控件。勾选框、下拉框和文本域没有自己的名字，用左边的字段名当读屏名称。
fn control(item: FieldItem, row: &FieldRowView, fkey: &str, caption: nana_ui::runtime::view::NodeRef) -> AnyView {
    let id = row.plugin_id.clone();
    let key = row.field_key.clone();
    match row.kind {
        FieldKind::Boolean => {
            // `.plugin-manager__settings-field input[type=checkbox]` 是 18 见方。
            let checkbox = style::native_checkbox(Checkbox::new("", row.checked), 18.0);
            widget(checkbox)
                .prop::<bool, fields::checkbox::checked>(row_flag(item, |row| row.checked))
                .prop::<bool, fields::checkbox::disabled>(row_flag(item, |row| row.disabled))
                .labelled_by(caption)
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
        FieldKind::Select => {
            let mut select = Select::new(Some(row.selected.clone())).options(select_options(&row.options));
            {
                let layout = Arc::make_mut(&mut select.style.layout);
                layout.width = Some(LengthSpec::Fill);
                layout.min_width = Some(LengthSpec::Px(0.0));
                layout.height = Some(LengthSpec::Px(32.0));
                layout.font_size = Some(14.0);
            }
            widget(select)
                .prop::<Vec<SelectOption>, fields::select::options>(move || item.try_with(|row| select_options(&row.options)).unwrap_or_default())
                .prop::<Option<Arc<str>>, fields::select::value>(move || item.try_with(|row| Arc::from(row.selected.as_str())))
                .prop::<bool, fields::select::disabled>(row_flag(item, |row| row.disabled))
                .labelled_by(caption)
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
        FieldKind::Json => {
            let draft = row_draft(move || item.try_with(|row| row.text.clone()));
            let mut area = TextArea::new(draft.signal().get_untracked()).placeholder(row.placeholder.clone());
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
            let (submit_id, submit_key) = (id.clone(), key.clone());
            widget(area)
                .model(draft.signal())
                .prop::<Arc<str>, fields::text_area::placeholder>(move || Arc::from(item.try_with(|row| row.placeholder.clone()).unwrap_or_default()))
                .prop::<bool, fields::text_area::disabled>(row_flag(item, |row| row.disabled))
                .labelled_by(caption)
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
        FieldKind::Text => {
            let draft = row_draft(move || item.try_with(|row| row.text.clone()));
            let input = style::native_input(
                TextInput::new(draft.signal().get_untracked()).label(row.label.clone()).placeholder(row.placeholder.clone()),
            );
            let (submit_id, submit_key) = (id.clone(), key.clone());
            widget(input)
                .model(draft.signal())
                .prop::<Option<Arc<str>>, fields::text_input::label>(move || item.try_with(|row| Arc::from(row.label.as_str())))
                .prop::<Arc<str>, fields::text_input::placeholder>(move || Arc::from(item.try_with(|row| row.placeholder.clone()).unwrap_or_default()))
                .prop::<bool, fields::text_input::disabled>(row_flag(item, |row| row.disabled))
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

/// 下拉框选项：`(值, 文字)`。
fn select_options(options: &[(String, String)]) -> Vec<SelectOption> {
    options.iter().map(|(value, text)| SelectOption::new(value.clone(), text.clone())).collect()
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
