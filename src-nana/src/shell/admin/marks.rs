//! 设置、插件、日志、拓展和动作页共用的 NanaUI 小部件写法：提示条、计数标签、空状态、加载转圈，
//! 以及插件设置区里下载服务和 Office 转换共用的状态卡、警示说明和状态行。
//!
//! 都是 NanaUI 现成的组件（`callout`、`tag_frame`、`EmptyState`、`Spinner`、`Card`、`LabeledValue`），
//! 这里只统一各页的用法：外面一层带键的列，键给测试和定位用；文字可以绑定。

use std::sync::Arc;

use nana_ui::icons_tabler::{ALERT_CIRCLE, ALERT_TRIANGLE, INFO_CIRCLE};
use nana_ui::runtime::view::{
    AnyView, Callout, El, IntoProp, IntoView, callout, empty_state, labeled_value, spinner,
    tag_frame, widget,
};
use nana_ui::runtime::{Card, EmptyState, Spinner, Stack, StatusTone, Text};
use nana_ui_core::{CardKind, SemanticColorRole as Role, TextWeight, TypeRole};

use super::style::column;

/// 带键的提示条：NanaUI 的 `callout`，错误用危险色调和圆叉，否则信息色调。文字为空时整条藏起来；
/// 返回元素，调用方还能再绑显隐。
pub(crate) fn notice(
    text: impl IntoProp<String>,
    error: bool,
    key: impl Into<String>,
) -> El<Stack, (Callout,)> {
    let (icon, tone) = if error {
        (ALERT_CIRCLE, StatusTone::Danger)
    } else {
        (INFO_CIRCLE, StatusTone::Info)
    };
    widget(column(0.0))
        .children((callout(icon, tone, text),))
        .key(key.into())
}

/// 标签：NanaUI 的 `tag_frame`（浅底、细边、小号弱色字），不能点。计数和日志来源都用它，文字可以绑定。
pub(crate) fn bound_tag(
    text: impl IntoProp<String>,
    key: impl Into<String>,
) -> El<Stack, (El<Text>,)> {
    widget(tag_frame()).children((widget(
        Text::new("")
            .role(TypeRole::Meta)
            .weight(TextWeight::Medium)
            .color(Role::Muted),
    )
    .prop::<String, nana_ui::runtime::view::fields::text::value>(text)
    .key(key.into()),))
}

/// 空状态：NanaUI 的 `EmptyState`，标题和说明可以绑定，键在空状态自己身上。
pub(crate) fn empty(
    title: impl Fn() -> String + Send + 'static,
    message: impl Fn() -> String + Send + 'static,
    key: impl Into<String>,
) -> El<EmptyState> {
    empty_state(move || Arc::<str>::from(title()))
        .message(move || Some(Arc::<str>::from(message())).filter(|text| !text.is_empty()))
        .key(key.into())
}

/// 加载中：NanaUI 的转圈，标签就是要说的话（也是读屏名字）。
pub(crate) fn loading(text: &str, key: impl Into<String>) -> El<Spinner> {
    spinner().label(Arc::<str>::from(text)).key(key.into())
}

/// 插件设置区里的状态卡：NanaUI 的选中样式卡片（选中底色加柔和边框），里面竖排间距 10。
pub(crate) fn status_card(body: Vec<AnyView>, key: &'static str) -> AnyView {
    widget(Card::new().kind(CardKind::Selected))
        .children((widget(column(10.0)).children(body),))
        .key(key)
        .into_any()
}

/// 警示色调的说明条：错误、空状态和页底说明都用它。
pub(crate) fn warning_note(text: &str, key: &'static str) -> AnyView {
    widget(column(0.0))
        .children((callout(
            ALERT_TRIANGLE,
            StatusTone::Warning,
            text.to_string(),
        ),))
        .key(key)
        .into_any()
}

/// 一列状态行：每行是 NanaUI 的 `LabeledValue`（标签靠左、值靠右），行键是 `{prefix}-row-{序号}`。
pub(crate) fn status_rows(items: &[(String, String)], prefix: &str) -> AnyView {
    let lines = items
        .iter()
        .enumerate()
        .map(|(index, (name, value))| {
            labeled_value(Arc::<str>::from(name.as_str()))
                .value(Arc::<str>::from(value.as_str()))
                .compact(false)
                .key(format!("{prefix}-row-{index}"))
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(column(0.0))
        .children(lines)
        .key(format!("{prefix}-rows"))
        .into_any()
}
