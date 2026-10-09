//! 主区的启动页和缺失仓库页。
//!
//! 启动页照 Vue `AppShell.vue` 的 `.workspace-startup`：眉题、步骤标题、「第 N / M 步」、6px 进度条、
//! 步骤说明、四个步骤圆标、错误条、「加载日志」和失败后的「重试」。缺失仓库页照
//! `MissingRepositoryState.vue`：警示图标、眉题、仓库名、说明、路径框、错误和三个操作。
//! 行高按 Vue 根字号的 `line-height: 1.55` 和各类自己的行高换算成像素。

use std::sync::Arc;

use nana_ui::icons_tabler::{ALERT_TRIANGLE, REFRESH};
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, IconGlyph, JustifySpec, LengthSpec, RadiusTier, ScrollAxes, ScrollView,
    SemanticColorRole, Stack, Text, TextChanged, TextHorizontalAlignment, TextInput,
};
use nana_ui::ButtonKind;

use super::render::{PRIMARY_INSET_Y, TITLE_BAR_PX};
use super::shell_tint::SoftFill;
use super::sidebar_view::parts::{label_text, primary_button};
use super::workspace::{StartupLog, StartupStatus, StartupStepItem, StartupStepState};
use super::{ShellMessage, ShellViewModel};

/// Vue 根元素的行高倍数。
const ROOT_LINE_HEIGHT: f32 = 1.55;
/// 启动页内容最宽 680px（`.workspace-startup__panel`）。
const STARTUP_PANEL_WIDTH: f32 = 680.0;
/// 缺失仓库页内容最宽 560px（`.missing-repository-page__panel`）。
const MISSING_PANEL_WIDTH: f32 = 560.0;
/// 加载日志框最高 240px，超出滚动。
const LOG_PANEL_MAX_HEIGHT: f32 = 240.0;
/// 日志和表头左列宽 74px。
const LOG_TIME_WIDTH: f32 = 74.0;
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
fn line(value: impl Into<String>, size: f32, weight: u16, color: SemanticColorRole, line_height: f32) -> Text {
    label_text(value, size, weight, Some(color)).line_height(line_height)
}

/// 占满列宽、可在任意位置折行的一段字。
fn wrapping(value: impl Into<String>, size: f32, weight: u16, color: SemanticColorRole, line_height: f32) -> Text {
    let mut node = line(value, size, weight, color, line_height);
    let layout = Arc::make_mut(&mut node.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    node
}

/// 眉题：11px、600、弱色、字距 0.4px。Vue 用 `text-transform: uppercase`，英文直接写大写。
fn eyebrow(value: &str, key: &'static str) -> AnyView {
    let mut node = line(value, 11.0, 600, SemanticColorRole::Faint, 11.0 * ROOT_LINE_HEIGHT);
    Arc::make_mut(&mut node.style.layout).letter_spacing = Some(0.4);
    widget(node).key(key).into_any()
}

/// 给一个节点套上外边距。Vue 用 `margin` 微调的几处间距照搬。
fn with_margin(child: AnyView, top: f32, bottom: f32) -> AnyView {
    widget(Stack::column(0.0).width(LengthSpec::Fill).with_layout(move |layout| {
        layout.margin_top = Some(LengthSpec::Px(top));
        layout.margin_bottom = Some(LengthSpec::Px(bottom));
    }))
    .children((child,))
    .into_any()
}

/// 启动页：放在占满主区的居中一节里。
pub(super) fn startup_section(model: &ShellViewModel) -> AnyView {
    widget(fill_section()).children((startup_panel(model),)).key("workspace-startup").into_any()
}

/// 启动页内容。百分比走动效时钟。
fn startup_panel(model: &ShellViewModel) -> AnyView {
    let startup = &model.workspace.startup;
    let mut rows: Vec<AnyView> = vec![
        eyebrow("MOMOBAKO", "startup-eyebrow"),
        widget(line(startup.step_label.clone(), 20.0, 700, SemanticColorRole::Text, 25.0)).key("startup-title").into_any(),
        widget(line(
            format!("第 {} / {} 步", startup.current_step, startup.total_steps),
            13.0,
            400,
            SemanticColorRole::Muted,
            13.0 * ROOT_LINE_HEIGHT,
        ))
        .key("startup-meta")
        .into_any(),
        progress_bar(model.motion.startup_percent()),
    ];
    if !startup.step_detail.is_empty() {
        let detail = widget(wrapping(startup.step_detail.clone(), 13.0, 400, SemanticColorRole::Text, 19.5)).key("startup-detail");
        rows.push(with_margin(detail.into_any(), -2.0, 0.0));
    }
    let steps = widget(Stack::column(8.0).width(LengthSpec::Fill))
        .children(startup.step_items().into_iter().map(step_row).collect::<Vec<_>>())
        .key("startup-steps");
    rows.push(with_margin(steps.into_any(), 2.0, 0.0));
    if let Some(error) = &startup.error {
        rows.push(
            widget(Stack::column(0.0).width(LengthSpec::Fill).padding_xy(12.0, 10.0).painter(SoftFill::err()))
                .children((widget(wrapping(error.clone(), 13.0, 400, SemanticColorRole::Danger, 13.0 * ROOT_LINE_HEIGHT)).key("startup-error"),))
                .into_any(),
        );
    }
    let logs = startup.visible_logs();
    if !logs.is_empty() {
        rows.push(log_panel(&logs));
    }
    if startup.status == StartupStatus::Error {
        let retry = Button::new("重试").kind(ButtonKind::Ghost).icon(REFRESH).icon_size(14.0);
        rows.push(
            widget(Stack::row(0.0))
                .children((widget(retry)
                    .key("startup-retry")
                    .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::StartupRetry)),))
                .into_any(),
        );
    }
    widget(Stack::column(12.0).width(LengthSpec::Fill).with_layout(|layout| {
        layout.max_width = Some(LengthSpec::Px(STARTUP_PANEL_WIDTH));
    }))
    .children(rows)
    .key("startup-panel")
    .into_any()
}

/// 6px 进度条：`--bg-subtle` 胶囊轨道，强调色填充，最少露出 4px。填充宽度绑在热信号上，逐帧走动效。
fn progress_bar(percent: f32) -> AnyView {
    let fill = widget(Stack::row(0.0).surface(SemanticColorRole::Accent).radius_px(999.0).with_layout(|layout| {
        layout.min_width = Some(LengthSpec::Px(4.0));
        layout.height = Some(LengthSpec::Fill);
    }))
    .prop::<f32, super::hot::FillPercentField>(super::hot::prop(|signals| signals.startup, percent));
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Px(6.0))
            .surface(SemanticColorRole::Subtle)
            .radius_px(999.0)
            .with_layout(|layout| {
                layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
                layout.align_items = AlignSpec::Stretch;
            }),
    )
    .children((fill,))
    .key("startup-progress")
    .into_any()
}

/// 一个步骤：24px 圆标和两行文字。当前步强调色、完成成功色、失败危险色，其余弱色。
fn step_row(item: StartupStepItem) -> AnyView {
    let (foreground, label_color) = match item.state {
        StartupStepState::Current => (SemanticColorRole::Accent, SemanticColorRole::Text),
        StartupStepState::Done => (SemanticColorRole::Success, SemanticColorRole::Muted),
        StartupStepState::Error => (SemanticColorRole::Danger, SemanticColorRole::Muted),
        StartupStepState::Pending => (SemanticColorRole::Muted, SemanticColorRole::Muted),
    };
    let mut number = line(item.number.to_string(), 12.0, 400, foreground, 12.0);
    number.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let badge = Stack::row(0.0)
        .width(LengthSpec::Px(24.0))
        .height(LengthSpec::Px(24.0))
        .min_width(LengthSpec::Px(24.0))
        .grow(0.0)
        .shrink(0.0)
        .align(AlignSpec::Center)
        .justify(JustifySpec::Center);
    let badge = match item.state {
        StartupStepState::Current => badge.surface(SemanticColorRole::AccentSoft).outline(SemanticColorRole::Accent, 1.0).radius_px(999.0),
        StartupStepState::Done => badge.painter(SoftFill::ok().border(SemanticColorRole::Success, 1.0).pill()),
        StartupStepState::Error => badge.painter(SoftFill::err().border(SemanticColorRole::Danger, 1.0).pill()),
        StartupStepState::Pending => badge.surface(SemanticColorRole::Subtle).outline(SemanticColorRole::Border, 1.0).radius_px(999.0),
    };
    let copy = widget(Stack::column(2.0).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
        widget(wrapping(item.label, 13.0, 600, label_color, 17.55)).key(format!("startup-label-{}", item.number)),
        widget(wrapping(item.detail, 12.0, 400, SemanticColorRole::Muted, 17.4)).key(format!("startup-copy-{}", item.number)),
    ));
    widget(Stack::bar(10.0).align(AlignSpec::Start))
        .children((widget(badge).children((widget(number).key(format!("startup-index-{}", item.number)),)), copy))
        .into_any()
}

/// 日志和表头共用的两列：左列 74px，右列占满，间距 10px。
fn log_columns(left: Text, right: AnyView) -> AnyView {
    let mut left = left;
    {
        let layout = Arc::make_mut(&mut left.style.layout);
        layout.width = Some(LengthSpec::Px(LOG_TIME_WIDTH));
        layout.min_width = Some(LengthSpec::Px(LOG_TIME_WIDTH));
        layout.flex_shrink = Some(0.0);
    }
    widget(Stack::bar(10.0).align(AlignSpec::Start)).children((widget(left).into_any(), right)).into_any()
}

/// 「加载日志」：最近 8 条，新的在前。框体描边不动，里面的内容超过 240px 时滚动。
fn log_panel(logs: &[&StartupLog]) -> AnyView {
    let header = log_columns(
        line("加载日志", 13.0, 700, SemanticColorRole::Text, 13.0 * ROOT_LINE_HEIGHT),
        widget(line(format!("{} 条最近记录", logs.len()), 12.0, 400, SemanticColorRole::Muted, 12.0 * ROOT_LINE_HEIGHT)).into_any(),
    );
    let entries = logs
        .iter()
        .enumerate()
        .map(|(index, record)| {
            let color = match record.level {
                "warn" => SemanticColorRole::Warning,
                "error" => SemanticColorRole::Danger,
                _ => SemanticColorRole::Text,
            };
            let mut lines = vec![widget(wrapping(record.message.clone(), 12.0, 600, color, 17.4)).into_any()];
            if let Some(detail) = &record.detail {
                lines.push(widget(wrapping(detail.clone(), 11.0, 400, SemanticColorRole::Muted, 15.95)).into_any());
            }
            let body = widget(Stack::column(2.0).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children(lines).into_any();
            let row = log_columns(line(record.time.clone(), 12.0, 400, SemanticColorRole::Muted, 17.4), body);
            widget(Stack::column(0.0).width(LengthSpec::Fill)).children((row,)).key(format!("startup-log-{index}")).into_any()
        })
        .collect::<Vec<_>>();
    let content = widget(Stack::column(8.0).width(LengthSpec::Fill).padding_xy(12.0, 10.0))
        .children((header, widget(Stack::column(7.0).width(LengthSpec::Fill)).children(entries).into_any()));
    let scroll = widget(ScrollView::new(ScrollAxes::Vertical).label("首屏加载日志").with_layout(|layout| {
        layout.width = Some(LengthSpec::Fill);
        layout.max_height = Some(LengthSpec::Px(LOG_PANEL_MAX_HEIGHT - 2.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
    }))
    .children((content,));
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Sm)
            .with_layout(|layout| {
                layout.max_height = Some(LengthSpec::Px(LOG_PANEL_MAX_HEIGHT));
                layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
            }),
    )
    .children((scroll,))
    .key("startup-logs")
    .into_any()
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
