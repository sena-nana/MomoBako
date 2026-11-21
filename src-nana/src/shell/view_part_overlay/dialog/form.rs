//! 对话框正文里的字段：标签加输入框、多行输入、下拉框和两列栅格。
//!
//! 每个字段是 NanaUI 的 `FormField`：小号字段的标签是次要色小字，控件在标签下面。
//! 输入框和下拉框取中号控件高（默认 32），左右内边距是控件自带的档（输入框、多行输入是字段档，默认 10），
//! `--radius-sm` 圆角、14px 字。
//! 输入框和多行输入用 `.model(草稿信号)` 受控，草稿由浮层会话按
//! `ModelField` 的规矩回写，按键不会被排在后台消息后面的旧草稿冲掉；改动和回车只发消息。

use std::sync::Arc;

use nana_ui::ControlSize;
use nana_ui::runtime::view::{AnyView, IntoProp, IntoView, Signal, fields, widget};
use nana_ui::runtime::{
    AlignSpec, FormField, LengthSpec, RadiusTier, Select, SelectChanged, SelectOption, Stack,
    TextArea, TextChanged, TextInput, TextSubmitted,
};
use nana_ui_core::{ControlHeight, SpacingStep};

use super::MessageFn;
use crate::shell::ShellMessage;

/// 下拉框的选项：值和显示文字。
pub(crate) type Choices = Vec<(String, String)>;

/// 一个字段：小号 `FormField`，标签在上，`control` 在下，占满宽度。标签也是字段的读屏名。
pub(crate) fn form_field(label: &'static str, key: &str, control: impl IntoView) -> AnyView {
    widget(FormField::new(label).size(ControlSize::Small))
        .key(format!("{key}-field"))
        .control(control)
        .into_any()
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
    let mut input = TextInput::new(draft.get_untracked())
        .label(label)
        .secure(secure);
    input.style.radius = Some(RadiusTier::Sm);
    input.style.control_height = Some(ControlHeight::Exact(ControlSize::Medium));
    {
        let layout = Arc::make_mut(&mut input.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }
    let mut control = widget(input)
        .model(draft)
        .key(key)
        .prop::<Arc<str>, fields::text_input::placeholder>(placeholder)
        .prop::<bool, fields::text_input::disabled>(disabled)
        .on_cx(move |_, event: &TextChanged, cx| {
            cx.dispatch_program_all(on_change(event.value.to_string()))
        });
    if let Some(submit) = on_submit {
        control = control.on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program_all(submit()));
    }
    form_field(label, key, control)
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
    let mut area = TextArea::new(draft.get_untracked())
        .label(label)
        .placeholder(placeholder)
        .height(rows * 20.0 + 14.0);
    area.style.radius = Some(RadiusTier::Sm);
    {
        let layout = Arc::make_mut(&mut area.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }
    let control = widget(area)
        .model(draft)
        .key(key)
        .prop::<bool, fields::text_area::disabled>(disabled)
        .on_cx(move |_, event: &TextChanged, cx| {
            cx.dispatch_program_all(on_change(event.value.to_string()))
        });
    form_field(label, key, control)
}

/// 下拉框字段。下拉框的读屏名是字段名；
/// `value` 和 `choices` 都可以读信号，打开期间变了只改这两个字段。
pub(crate) fn select_field(
    label: &'static str,
    key: &'static str,
    value: impl Fn() -> Option<String> + Send + 'static,
    choices: impl Fn() -> Choices + Send + 'static,
    disabled: impl IntoProp<bool>,
    on_change: impl Fn(String) -> ShellMessage + Send + Sync + 'static,
) -> AnyView {
    let mut select = Select::new(value())
        .options(options(choices()))
        .label(label);
    select.style.radius = Some(RadiusTier::Sm);
    select.style.control_height = Some(ControlHeight::Exact(ControlSize::Medium));
    {
        let layout = Arc::make_mut(&mut select.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }
    let control = widget(select)
        .key(key)
        .prop::<Option<Arc<str>>, fields::select::value>(move || value().map(Arc::from))
        .prop::<Vec<SelectOption>, fields::select::options>(move || options(choices()))
        .prop::<bool, fields::select::disabled>(disabled)
        .on_cx(move |_, event: &SelectChanged, cx| {
            cx.dispatch_program_all(on_change(event.value.to_string()))
        });
    form_field(label, key, control)
}

fn options(choices: Choices) -> Vec<SelectOption> {
    choices
        .into_iter()
        .map(|(value, text)| SelectOption::new(value, text))
        .collect()
}

/// 两列等宽的栅格（`grid-template-columns: repeat(2, minmax(0, 1fr))`），行列间距都是 `gap` 档。
/// 单数个格子时最后一行右边空着。
pub(crate) fn two_columns(key: &'static str, gap: SpacingStep, cells: Vec<AnyView>) -> AnyView {
    let column = |cell: Option<AnyView>| -> AnyView {
        widget(
            Stack::column(0.0)
                .grow(1.0)
                .shrink(1.0)
                .min_width(LengthSpec::Px(0.0))
                .with_layout(|layout| {
                    layout.flex_basis = Some(LengthSpec::Px(0.0));
                }),
        )
        .children((cell,))
        .into_any()
    };
    let mut rows = Vec::new();
    let mut cells = cells.into_iter();
    while let Some(first) = cells.next() {
        let second = cells.next();
        rows.push(
            widget(Stack::bar(0.0).gap_step(gap).align(AlignSpec::Start))
                .children((column(Some(first)), column(second)))
                .into_any(),
        );
    }
    widget(Stack::column(0.0).gap_step(gap).width(LengthSpec::Fill))
        .key(key)
        .children(rows)
        .into_any()
}
