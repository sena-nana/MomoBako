//! 缺失仓库页，以及它和启动页共用的排版零件。
//!
//! 缺失仓库页照 `MissingRepositoryState.vue`：警示图标、眉题、仓库名、说明、路径框、错误和三个操作。
//! 启动页本身在 `route_startup.rs`（常驻路由）。行高按 Vue 根字号的 `line-height: 1.55` 和各类
//! 自己的行高换算成像素。

use std::sync::Arc;

use nana_ui::icons_tabler::ALERT_TRIANGLE;
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, IconGlyph, JustifySpec, LengthSpec, RadiusTier, SemanticColorRole, Stack, Text,
    TextChanged, TextInput,
};
use nana_ui::ButtonKind;

use super::render::{PRIMARY_INSET_Y, TITLE_BAR_PX};
use super::shell_tint::SoftFill;
use super::sidebar_view::parts::{label_text, primary_button};
use super::{ShellMessage, ShellViewModel};

/// Vue 根元素的行高倍数。
pub(super) const ROOT_LINE_HEIGHT: f32 = 1.55;
/// 缺失仓库页内容最宽 560px（`.missing-repository-page__panel`）。
const MISSING_PANEL_WIDTH: f32 = 560.0;
/// Vue `--font-mono` 的字体栈。
const MONO_FONTS: &str =
    "ui-monospace, \"SF Mono\", \"Cascadia Mono\", \"Cascadia Code\", \"JetBrains Mono\", Menlo, Consolas, \"Liberation Mono\", monospace";

/// 占满主区可见高度、内容居中的一节，内边距 24px。
///
/// 对应 Vue `.workspace-startup`、`.missing-repository-page` 和 `.empty-state-page` 的
/// `min-height: 100%`：主区可见高度是窗口高减去标题栏和主区上下内边距。窗口矮时内容
/// 撑高这一节，交给外层滚动。
pub(crate) fn fill_section() -> Stack {
    Stack::column(0.0)
        .min_height(LengthSpec::CalcViewportOffset {
            axis: nana_ui_core::ViewportAxis::Height,
            value: 100.0,
            offset_px: -(TITLE_BAR_PX + PRIMARY_INSET_Y * 2.0),
        })
        .padding(24.0)
        .justify(JustifySpec::Center)
        .align(AlignSpec::Center)
}

/// 一段文字，行高写成像素。
pub(super) fn line(value: impl Into<String>, size: f32, weight: u16, color: SemanticColorRole, line_height: f32) -> Text {
    label_text(value, size, weight, Some(color)).line_height(line_height)
}

/// 占满列宽、可在任意位置折行的一段字。
pub(super) fn wrapping(value: impl Into<String>, size: f32, weight: u16, color: SemanticColorRole, line_height: f32) -> Text {
    let mut node = line(value, size, weight, color, line_height);
    let layout = Arc::make_mut(&mut node.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    node
}

/// 眉题：11px、600、弱色、字距 0.4px。Vue 用 `text-transform: uppercase`，英文直接写大写。
pub(super) fn eyebrow(value: &str, key: &'static str) -> AnyView {
    let mut node = line(value, 11.0, 600, SemanticColorRole::Faint, 11.0 * ROOT_LINE_HEIGHT);
    Arc::make_mut(&mut node.style.layout).letter_spacing = Some(0.4);
    widget(node).key(key).into_any()
}

/// 带外边距的一层。Vue 用 `margin` 微调的几处间距照搬。
pub(super) fn margin(top: f32, bottom: f32) -> Stack {
    Stack::column(0.0).width(LengthSpec::Fill).with_layout(move |layout| {
        layout.margin_top = Some(LengthSpec::Px(top));
        layout.margin_bottom = Some(LengthSpec::Px(bottom));
    })
}

/// 给一个节点套上外边距。
fn with_margin(child: AnyView, top: f32, bottom: f32) -> AnyView {
    widget(margin(top, bottom)).children((child,)).into_any()
}

/// 缺失仓库页：放在占满主页主体的居中一节里。
pub(super) fn missing_repository_section(model: &ShellViewModel) -> AnyView {
    widget(fill_section()).children((missing_repository_panel(model),)).key("missing-repository-page").into_any()
}

/// 缺失仓库：重定向、刷新和删除。忙或删除中时按钮不可用。
fn missing_repository_panel(model: &ShellViewModel) -> AnyView {
    let repository = model.workspace.active_repository();
    let name = repository.map(|item| item.name.clone()).unwrap_or_else(|| "资源库不可用".into());
    let path = repository.map(|item| item.path.clone()).unwrap_or_default();
    let cache_issue = repository.is_some_and(|item| item.is_source_cache_issue());
    let summary = if cache_issue {
        "这个来源资源库需要在插件设置中配置本地缓存或重新认证。仓库记录和已有缓存不会被删除。"
    } else {
        "MomoBako 找不到这个资源库的本地文件夹。可以重定向到原资源库位置，或移除这条注册记录和本机缓存。"
    };
    let busy = model.workspace.missing_busy();
    let icon = widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(42.0))
            .height(LengthSpec::Px(42.0))
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .painter(SoftFill::err().radius(RadiusTier::Md)),
    )
    .children((widget(IconGlyph::new(ALERT_TRIANGLE).size(22.0).role(SemanticColorRole::Danger)),))
    .key("missing-icon");
    let mut summary_text = wrapping(summary, 13.0, 400, SemanticColorRole::Muted, 13.0 * 1.6);
    Arc::make_mut(&mut summary_text.style.layout).max_width = Some(LengthSpec::Px(520.0));
    let mut rows: Vec<AnyView> = vec![
        with_margin(icon.into_any(), 0.0, 14.0),
        eyebrow("资源库丢失", "missing-eyebrow"),
        with_margin(widget(line(name, 22.0, 650, SemanticColorRole::Text, 22.0 * ROOT_LINE_HEIGHT)).key("missing-name").into_any(), 4.0, 0.0),
        with_margin(widget(summary_text).key("missing-summary").into_any(), 12.0, 0.0),
        with_margin(path_box(path), 16.0, 0.0),
    ];
    if !model.workspace.missing_error.is_empty() {
        let error = widget(Stack::column(0.0).width(LengthSpec::Fill).padding_xy(10.0, 8.0).painter(SoftFill::err()))
            .children((widget(wrapping(model.workspace.missing_error.clone(), 12.0, 400, SemanticColorRole::Danger, 17.4)).key("missing-error"),));
        rows.push(with_margin(error.into_any(), 12.0, 0.0));
    }
    if model.workspace.path_prompt && !cache_issue {
        let draft = model.workspace.path_draft.clone();
        let editor = widget(Stack::column(8.0).width(LengthSpec::Fill)).children((
            widget(TextInput::new(draft).label("资源库新位置")).on_cx(|_, event: &TextChanged, cx| {
                cx.dispatch_program_all(ShellMessage::MissingPathChanged(event.value.to_string()));
            }),
            widget(Stack::row(0.0)).children((widget(primary_button("确认重定向").disabled(busy))
                .key("missing-submit-path")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::MissingSubmitPath)),)),
        ));
        rows.push(with_margin(editor.into_any(), 12.0, 0.0));
    }
    let actions = widget(Stack::row(8.0).align(AlignSpec::Center).wrap(true)).children((
        widget(primary_button(model.workspace.missing_primary_label()).disabled(busy))
            .key("missing-primary")
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(if cache_issue { ShellMessage::MissingOpenSourceSettings } else { ShellMessage::MissingChoosePath });
            }),
        widget(Button::new("刷新").kind(ButtonKind::Ghost).disabled(busy))
            .key("missing-refresh")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::MissingRefresh)),
        widget(Button::new(model.workspace.missing_delete_label()).kind(ButtonKind::Danger).disabled(busy))
            .key("missing-delete")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::MissingOpenDelete)),
    ));
    rows.push(with_margin(actions.into_any(), 18.0, 0.0));
    widget(Stack::column(0.0).width(LengthSpec::Fill).align(AlignSpec::Start).with_layout(|layout| {
        layout.max_width = Some(LengthSpec::Px(MISSING_PANEL_WIDTH));
    }))
    .children(rows)
    .key("missing-panel")
    .into_any()
}

/// 路径框：`--bg` 底、`--border` 描边、等宽 12px、可在任意位置折行。
fn path_box(path: String) -> AnyView {
    let mut node = wrapping(path, 12.0, 400, SemanticColorRole::Text, 18.0);
    Arc::make_mut(&mut node.style.layout).font_family = Some(MONO_FONTS.into());
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .padding_xy(12.0, 10.0)
            .surface(SemanticColorRole::Background)
            .outline(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Sm),
    )
    .children((widget(node).key("missing-path"),))
    .into_any()
}
