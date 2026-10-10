//! 设置、插件、日志、拓展和动作页常驻视图共用的绑定工具。
//!
//! - [`row_text`]、[`row_flag`]：列表行里的字段绑定，行已经删掉时读默认值；
//! - [`row_draft`]：列表行里受控输入框的草稿，行里的 ViewModel 值相对上次投影变了才写回；
//! - [`ActionDisabled`]、[`ReadOnly`]、[`FollowEnd`]：控件表里没有的可绑定字段；按钮图标和整份样式用各页共用的
//!   [`ButtonIcon`]、[`StyleField`]（`field_bind.rs`）。

use nana_ui::runtime::view::{untrack, watch_effect, FieldWrite, StorePath};
use nana_ui::runtime::{Button, ScrollView, TextInput};

pub(crate) use super::super::field_bind::{ButtonIcon, StyleField};
use super::super::hot::ModelField;
use crate::theme_map::DISABLED_OPACITY;

/// 行里的一段文字。行已经删掉时读空串，不去读不存在的行。
pub(crate) fn row_text<P: StorePath>(item: P, pick: fn(&P::Value) -> &String) -> impl Fn() -> String + Send + 'static {
    move || item.try_with(|row| pick(row).clone()).unwrap_or_default()
}

/// 行里按字段算出的值。行已经删掉时读默认值。
pub(crate) fn row_flag<P: StorePath, R: Default + 'static>(item: P, pick: fn(&P::Value) -> R) -> impl Fn() -> R + Send + 'static {
    move || item.try_with(pick).unwrap_or_default()
}

/// 列表行里受控输入框的草稿：在行的作用域里建，随行一起回收。`read` 读这一行在 ViewModel 里的值
/// （行已经删掉时为 `None`），值相对上次投影变了才写进输入框的信号，刚打的字不会被较旧的值冲掉。
pub(crate) fn row_draft(read: impl Fn() -> Option<String> + 'static) -> ModelField {
    let draft = ModelField::new(&untrack(&read).unwrap_or_default());
    watch_effect(move || {
        if let Some(value) = read() {
            untrack(|| draft.sync(&value));
        }
    });
    draft
}

/// `style::action` 按钮的禁用：Vue 原生按钮禁用时整体 45% 不透明，不换颜色，所以和禁用标志一起写透明度。
pub(crate) struct ActionDisabled;

impl FieldWrite<Button, bool> for ActionDisabled {
    const FIELD: &'static str = "Button.disabled+opacity";

    fn write(target: &mut Button, disabled: bool) {
        target.disabled = disabled;
        let opacity = disabled.then_some(DISABLED_OPACITY);
        if target.style.layout.opacity != opacity {
            std::sync::Arc::make_mut(&mut target.style.layout).opacity = opacity;
        }
    }

    fn differs(target: &Button, disabled: &bool) -> bool {
        target.disabled != *disabled || target.style.layout.opacity != disabled.then_some(DISABLED_OPACITY)
    }
}

/// 输入框只读：API Playground 的目标框只有 HTTP 端点能改。
pub(crate) struct ReadOnly;

impl FieldWrite<TextInput, bool> for ReadOnly {
    const FIELD: &'static str = "TextInput.read_only";

    fn write(target: &mut TextInput, read_only: bool) {
        target.read_only = read_only;
    }

    fn differs(target: &TextInput, read_only: &bool) -> bool {
        target.read_only != *read_only
    }
}

/// 滚动区是否跟随末尾：日志列表追踪时为真。写成真时运行时当场滚到底，之后内容变长也跟着；
/// 写成假时停在原位。
pub(crate) struct FollowEnd;

impl FieldWrite<ScrollView, bool> for FollowEnd {
    const FIELD: &'static str = "ScrollView.follow_end";

    fn write(target: &mut ScrollView, follow: bool) {
        target.follow_end = follow;
    }

    fn differs(target: &ScrollView, follow: &bool) -> bool {
        target.follow_end != *follow
    }
}
