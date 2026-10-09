//! 对话框正文里的字段：标签加输入框、多行输入、下拉框和两列栅格，照 Vue `.dialog-field`。
//!
//! 标签 12px/600 次要色，下面是控件，间距 6；输入框、多行输入和下拉框最小高 32、左右内边距 8、
//! `--radius-sm` 圆角、14px 字。输入框和多行输入用 `.model(草稿信号)` 受控，草稿由浮层会话按
//! `ModelField` 的规矩回写，按键不会被排在后台消息后面的旧草稿冲掉；改动和回车只发消息。

use std::sync::Arc;

use nana_ui::runtime::view::{fields, node_ref, widget, AnyView, IntoProp, IntoView, NodeRef, Signal};
use nana_ui::runtime::{
    AlignSpec, LengthSpec, RadiusTier, Select, SelectChanged, SelectOption, SemanticColorRole, Stack, Text, TextArea,
    TextChanged, TextInput, TextSubmitted,
};

use crate::shell::ShellMessage;
use super::MessageFn;

/// 下拉框的选项：值和显示文字。
pub(crate) type Choices = Vec<(String, String)>;

/// `.dialog-field` 的竖排：标签在上，控件在下，间距 6，占满宽度。
fn field_column() -> Stack {
    Stack::column(6.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))
}

/// 字段标签：12px、600、次要文字色。
fn caption(label: &'static str) -> Text {
    Text::new(label).font_size(12.0).font_weight(600).color(SemanticColorRole::Muted).line_height(18.6)
}

/// 输入框字段。`draft` 是浮层会话登记的草稿信号；`on_submit` 为 `Some` 时回车提交。
#[allow(clippy::too_many_arguments)]
pub(crate) fn text_field(
    label: &'static str,
    key: &'static str,
    draft: Signal<String>,
    placeholder: impl IntoProp<Arc<str>>,
    secure: bool,
    disabled: impl IntoProp<bool>,
    on_change: impl Fn(String) -> ShellMessage + Send + Sync + 'static,
    on_submit: Option<MessageFn>,
) -> AnyView {
    let mut input = TextInput::new(draft.get_untracked()).label(label).secure(secure);
    input.style.radius = Some(RadiusTier::Sm);
    input.style.control_height = None;
    {
        let layout = Arc::make_mut(&mut input.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.height = Some(LengthSpec::Px(32.0));
        layout.min_height = Some(LengthSpec::Px(32.0));
        layout.padding_left = Some(LengthSpec::Px(8.0));
        layout.padding_right = Some(LengthSpec::Px(8.0));
        layout.font_size = Some(14.0);
    }
    let mut control = widget(input)
        .model(draft)
        .key(key)
        .prop::<Arc<str>, fields::text_input::placeholder>(placeholder)
        .prop::<bool, fields::text_input::disabled>(disabled)
        .on_cx(move |_, event: &TextChanged, cx| cx.dispatch_program_all(on_change(event.value.to_string())));
    if let Some(submit) = on_submit {
        control = control.on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program_all(submit()));
    }
    widget(field_column())
        .key(format!("{key}-field"))
        .children((widget(caption(label)).key(format!("{key}-label")), control))
        .into_any()
}

/// 多行输入字段：`rows` 行高，每行 20px 加上下内边距。
#[allow(clippy::too_many_arguments)]
pub(crate) fn text_area_field(
    label: &'static str,
    key: &'static str,
    draft: Signal<String>,
    placeholder: &'static str,
    rows: f32,
    disabled: impl IntoProp<bool>,
    on_change: impl Fn(String) -> ShellMessage + Send + Sync + 'static,
) -> AnyView {
    let mut area = TextArea::new(draft.get_untracked()).label(label).placeholder(placeholder).height(rows * 20.0 + 14.0);
    area.style.radius = Some(RadiusTier::Sm);
    {
        let layout = Arc::make_mut(&mut area.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.padding_left = Some(LengthSpec::Px(8.0));
        layout.padding_right = Some(LengthSpec::Px(8.0));
        layout.font_size = Some(14.0);
    }
    let control = widget(area)
        .model(draft)
        .key(key)
        .prop::<bool, fields::text_area::disabled>(disabled)
        .on_cx(move |_, event: &TextChanged, cx| cx.dispatch_program_all(on_change(event.value.to_string())));
    widget(field_column())
        .key(format!("{key}-field"))
        .children((widget(caption(label)).key(format!("{key}-label")), control))
        .into_any()
}

/// 下拉框字段。下拉框没有自己的名称，用 `.labelled_by` 以字段名命名，和 Vue `<label>` 包着
/// `<select>` 一样；`value` 和 `choices` 都可以读信号，打开期间变了只改这两个字段。
pub(crate) fn select_field(
    label: &'static str,
    key: &'static str,
    value: impl Fn() -> Option<String> + Send + 'static,
    choices: impl Fn() -> Choices + Send + 'static,
    disabled: impl IntoProp<bool>,
    on_change: impl Fn(String) -> ShellMessage + Send + Sync + 'static,
) -> AnyView {
    let named: NodeRef = node_ref();
    let mut select = Select::new(value()).options(options(choices()));
    select.style.radius = Some(RadiusTier::Sm);
    select.style.control_height = None;
    {
        let layout = Arc::make_mut(&mut select.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.height = Some(LengthSpec::Px(32.0));
        layout.min_height = Some(LengthSpec::Px(32.0));
        layout.font_size = Some(14.0);
    }
    let control = widget(select)
        .key(key)
        .labelled_by(named)
        .prop::<Option<Arc<str>>, fields::select::value>(move || value().map(Arc::from))
        .prop::<Vec<SelectOption>, fields::select::options>(move || options(choices()))
        .prop::<bool, fields::select::disabled>(disabled)
        .on_cx(move |_, event: &SelectChanged, cx| cx.dispatch_program_all(on_change(event.value.to_string())));
    widget(field_column())
        .key(format!("{key}-field"))
        .children((widget(caption(label)).node_ref(named).key(format!("{key}-label")), control))
        .into_any()
}

fn options(choices: Choices) -> Vec<SelectOption> {
    choices.into_iter().map(|(value, text)| SelectOption::new(value, text)).collect()
}

/// 两列等宽的栅格（`grid-template-columns: repeat(2, minmax(0, 1fr))`），行列间距都是 `gap`。
/// 单数个格子时最后一行右边空着。
pub(crate) fn two_columns(key: &'static str, gap: f32, cells: Vec<AnyView>) -> AnyView {
    let column = |cell: Option<AnyView>| -> AnyView {
        widget(Stack::column(0.0).grow(1.0).shrink(1.0).min_width(LengthSpec::Px(0.0)).with_layout(|layout| {
            layout.flex_basis = Some(LengthSpec::Px(0.0));
        }))
        .children((cell,))
        .into_any()
    };
    let mut rows = Vec::new();
    let mut cells = cells.into_iter();
    while let Some(first) = cells.next() {
        let second = cells.next();
        rows.push(widget(Stack::bar(gap).align(AlignSpec::Start)).children((column(Some(first)), column(second))).into_any());
    }
    widget(Stack::column(gap).width(LengthSpec::Fill)).key(key).children(rows).into_any()
}
