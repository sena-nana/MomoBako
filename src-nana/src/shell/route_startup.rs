//! 启动路由（常驻）：首屏加载时的启动页，加载失败时也在这里。
//!
//! 这是第一块改成常驻的区域，也是后续代理照着改的样板：
//! - [`StartupView`] 是从 ViewModel 算出的投影（`PartialEq`，有单测）；
//! - [`StartupSignals`] 在主区块的常驻作用域里建，同步时只写变了的信号；
//! - 视图只建一次：文字按字段绑定，可有可无的块用 `.visible`（节点留着、不占布局），
//!   步骤和日志用带 key 的 `each`，状态变了的那一行才重建；进度条直接绑热信号；
//! - 按钮的事件处理器只发意图消息，不捕获任何会过期的值。
//!
//! 版式照 Vue `AppShell.vue` 的 `.workspace-startup`：眉题、步骤标题、「第 N / M 步」、6px 进度条、
//! 步骤说明、四个步骤圆标、错误条、「加载日志」和失败后的「重试」。

use std::sync::Arc;

use nana_ui::icons_tabler::REFRESH;
use nana_ui::runtime::view::{each, fields, signal, widget, AnyView, IntoView, Signal};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, JustifySpec, LengthSpec, RadiusTier, ScrollAxes, ScrollView, SemanticColorRole, Stack,
    Text, TextHorizontalAlignment,
};
use nana_ui::ButtonKind;

use super::hot::{FillPercentField, HotSignals};
use super::shell_tint::SoftFill;
use super::startup_view::{eyebrow, fill_section, line, margin, wrapping, ROOT_LINE_HEIGHT};
use super::workspace::{StartupLog, StartupStatus, StartupStepItem, StartupStepState};
use super::{ShellMessage, ShellViewModel};

/// 启动页内容最宽 680px（`.workspace-startup__panel`）。
const PANEL_WIDTH: f32 = 680.0;
/// 加载日志框最高 240px，超出滚动。
const LOG_PANEL_MAX_HEIGHT: f32 = 240.0;
/// 日志和表头左列宽 74px。
const LOG_TIME_WIDTH: f32 = 74.0;

/// 启动页要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StartupView {
    pub text: StartupText,
    pub steps: Vec<StartupStepItem>,
    /// 最近 8 条日志，新的在前。
    pub logs: Vec<LogRow>,
}

/// 标题、步数、说明、错误和能否重试：经常一起变，放在一个信号里。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StartupText {
    pub title: String,
    /// 「第 N / M 步」。
    pub meta: String,
    /// 步骤说明，空时不显示。
    pub detail: String,
    pub error: Option<String>,
    /// 启动失败时显示「重试」。
    pub retry: bool,
}

/// 日志框的一行。日志没有自己的编号，行的身份是位置加内容：新日志进来、旧的往下挪时按内容变了重建。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LogRow {
    pub index: usize,
    pub log: StartupLog,
}

impl StartupView {
    /// 从 ViewModel 取启动页的投影。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let startup = &model.workspace.startup;
        Self {
            text: StartupText {
                title: startup.step_label.clone(),
                meta: format!("第 {} / {} 步", startup.current_step, startup.total_steps),
                detail: startup.step_detail.clone(),
                error: startup.error.clone(),
                retry: startup.status == StartupStatus::Error,
            },
            steps: startup.step_items().to_vec(),
            logs: startup
                .visible_logs()
                .into_iter()
                .enumerate()
                .map(|(index, log)| LogRow { index, log: log.clone() })
                .collect(),
        }
    }
}

/// 启动页的信号。句柄是 `Copy` 的 id，值在主区块的常驻作用域里。
#[derive(Clone, Copy)]
pub(crate) struct StartupSignals {
    text: Signal<StartupText>,
    steps: Signal<Vec<StartupStepItem>>,
    logs: Signal<Vec<LogRow>>,
}

impl StartupSignals {
    /// 在常驻作用域里建信号，初值是这一刻的投影。
    pub(crate) fn new(model: &ShellViewModel) -> Self {
        let view = StartupView::project(model);
        Self { text: signal(view.text), steps: signal(view.steps), logs: signal(view.logs) }
    }

    /// 写入投影，只写变了的信号；信号已随文档回收时不写。
    pub(crate) fn write(&self, view: StartupView) {
        self.text.try_set_if_changed(view.text);
        self.steps.try_set_if_changed(view.steps);
        self.logs.try_set_if_changed(view.logs);
    }
}

/// 启动页：Vue 主区内容层 `overflow: auto`，窗口矮时整页连同内边距一起滚动；
/// `.workspace-startup` 的 `min-height: 100%` 让内容在高窗口里上下居中。
pub(super) fn view(signals: StartupSignals, hot: HotSignals) -> AnyView {
    let section = widget(fill_section()).children((panel(signals, hot),)).key("workspace-startup").into_any();
    super::route_home::scroll_route(section, "workspace-startup-scroll")
}

/// 启动页内容。没有说明、错误、日志或不能重试时，这几块留着但不占布局。
fn panel(signals: StartupSignals, hot: HotSignals) -> AnyView {
    let text = signals.text;
    let initial = text.get_untracked();
    let title = widget(line(initial.title.clone(), 20.0, 700, SemanticColorRole::Text, 25.0))
        .prop::<String, fields::text::value>(move || text.with(|text| text.title.clone()))
        .key("startup-title");
    let meta = widget(line(initial.meta.clone(), 13.0, 400, SemanticColorRole::Muted, 13.0 * ROOT_LINE_HEIGHT))
        .prop::<String, fields::text::value>(move || text.with(|text| text.meta.clone()))
        .key("startup-meta");
    let detail = widget(margin(-2.0, 0.0)).visible(move || text.with(|text| !text.detail.is_empty())).children((
        widget(wrapping(initial.detail.clone(), 13.0, 400, SemanticColorRole::Text, 19.5))
            .prop::<String, fields::text::value>(move || text.with(|text| text.detail.clone()))
            .key("startup-detail"),
    ));
    let steps = widget(margin(2.0, 0.0)).children((each(signals.steps, step_key, step_row).gap(8.0).key("startup-steps"),));
    let error = widget(Stack::column(0.0).width(LengthSpec::Fill).padding_xy(12.0, 10.0).painter(SoftFill::err()))
        .visible(move || text.with(|text| text.error.is_some()))
        .children((widget(wrapping(initial.error.clone().unwrap_or_default(), 13.0, 400, SemanticColorRole::Danger, 13.0 * ROOT_LINE_HEIGHT))
            .prop::<String, fields::text::value>(move || text.with(|text| text.error.clone().unwrap_or_default()))
            .key("startup-error"),));
    let retry = Button::new("重试").kind(ButtonKind::Ghost).icon(REFRESH).icon_size(14.0);
    let retry = widget(Stack::row(0.0)).visible(move || text.with(|text| text.retry)).children((widget(retry)
        .key("startup-retry")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::StartupRetry)),));
    widget(Stack::column(12.0).width(LengthSpec::Fill).with_layout(|layout| {
        layout.max_width = Some(LengthSpec::Px(PANEL_WIDTH));
    }))
    .children((
        eyebrow("MOMOBAKO", "startup-eyebrow"),
        title,
        meta,
        progress_bar(hot.startup),
        detail,
        steps,
        error,
        log_panel(signals.logs),
        retry,
    ))
    .key("startup-panel")
    .into_any()
}

/// 6px 进度条：`--bg-subtle` 胶囊轨道，强调色填充，最少露出 4px。填充宽度绑在热信号上，逐帧走动效。
fn progress_bar(percent: Signal<f32>) -> AnyView {
    let fill = widget(Stack::row(0.0).surface(SemanticColorRole::Accent).radius_px(999.0).with_layout(|layout| {
        layout.min_width = Some(LengthSpec::Px(4.0));
        layout.height = Some(LengthSpec::Fill);
    }))
    .prop::<f32, FillPercentField>(percent);
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

/// 步骤行的身份：步号加状态。圆标的样式由状态决定，状态变了整行重建，别的行不动。
fn step_key(item: &StartupStepItem) -> (u8, StartupStepState) {
    (item.number, item.state)
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

/// 「加载日志」：最近 8 条，新的在前。框体描边不动，里面的内容超过 240px 时滚动。没有日志时不占布局。
fn log_panel(logs: Signal<Vec<LogRow>>) -> AnyView {
    let count = logs.with_untracked(Vec::len);
    let summary = widget(line(format!("{count} 条最近记录"), 12.0, 400, SemanticColorRole::Muted, 12.0 * ROOT_LINE_HEIGHT))
        .prop::<String, fields::text::value>(move || logs.with(|logs| format!("{} 条最近记录", logs.len())))
        .into_any();
    let header = log_columns(line("加载日志", 13.0, 700, SemanticColorRole::Text, 13.0 * ROOT_LINE_HEIGHT), summary);
    let entries = each(logs, LogRow::clone, log_row).gap(7.0);
    let content = widget(Stack::column(8.0).width(LengthSpec::Fill).padding_xy(12.0, 10.0)).children((header, entries));
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
    .visible(move || logs.with(|logs| !logs.is_empty()))
    .children((scroll,))
    .key("startup-logs")
    .into_any()
}

/// 一条日志：左列时间，右列消息和明细。警告和错误按级别着色。
fn log_row(row: LogRow) -> AnyView {
    let record = row.log;
    let color = match record.level {
        "warn" => SemanticColorRole::Warning,
        "error" => SemanticColorRole::Danger,
        _ => SemanticColorRole::Text,
    };
    let mut lines = vec![widget(wrapping(record.message, 12.0, 600, color, 17.4)).into_any()];
    if let Some(detail) = record.detail {
        lines.push(widget(wrapping(detail, 11.0, 400, SemanticColorRole::Muted, 15.95)).into_any());
    }
    let body = widget(Stack::column(2.0).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children(lines).into_any();
    let columns = log_columns(line(record.time, 12.0, 400, SemanticColorRole::Muted, 17.4), body);
    widget(Stack::column(0.0).width(LengthSpec::Fill))
        .children((columns,))
        .key(format!("startup-log-{}", row.index))
        .into_any()
}

#[cfg(test)]
#[path = "route_startup_tests.rs"]
mod tests;
