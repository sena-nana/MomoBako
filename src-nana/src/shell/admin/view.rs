//! 任务弹层。首页的日志、拓展和动作面板是常驻路由，见 `route_admin.rs`。
//!
//! 任务弹层的面板：每个任务一块，来源加 NanaUI 的 `Progress` 进度条（下面是标签、细节、百分比）；
//! 没有任务时是紧凑的 NanaUI `EmptyState`。面板挂在侧栏底部「任务」按钮的
//! NanaUI `Popover` 里（`sidebar_view.rs`），开合、点外面、Escape 和焦点归还都由框架负责。面板常驻：
//! 任务按键增删行，进度、标签和细节只改绑定的字段；进度条的值按 0.18s 过渡（填充的时钟在
//! `progress_fill.rs`，逐帧的值经热信号写进来）。不确定进度由 `Progress` 自己沿轨道扫。

use nana_ui::runtime::view::{
    AnyView, IntoView, Signal, computed, each, empty_state, fields, progress, widget,
};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, IconButton, LengthSpec, ScrollAxes, ScrollView, Stack,
};
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};

use super::super::hot::{self, TaskFills};
use super::super::{ShellMessage, ShellViewModel};
use super::AdminMessage;
use super::icons;
use super::style::{self, column, label, pad, row};

/// 任务弹层的一行。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TaskRowView {
    /// 行的键：任务编号，重复时加序号。
    pub key: String,
    pub source: String,
    pub label: String,
    pub detail: String,
    /// 后台任务编号。仓库操作行不是可取消的任务。
    pub task_id: String,
    /// 排队或运行中才显示取消。
    pub cancellable: bool,
    /// 四舍五入后的百分比；不确定进度时为 `None`，不显示百分比。
    pub percent: Option<f64>,
    /// 进度条的值（百分比，不确定进度时不画）。
    pub fill: f32,
}

/// 任务弹层要显示的东西：开没开着，开着时的各行（关着时不投影行，进度变化不打扰侧栏）。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TaskPopoverView {
    pub open: bool,
    pub rows: Vec<TaskRowView>,
}

impl TaskPopoverView {
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        if !model.admin.popover_open {
            return Self::default();
        }
        Self {
            open: true,
            rows: task_row_views(model),
        }
    }
}

/// 弹层的行：任务编号做键（重复时加序号），百分比夹到 0–100 再取整。
fn task_row_views(model: &ShellViewModel) -> Vec<TaskRowView> {
    let tasks = model.task_rows();
    let keys = style::unique_keys(tasks.iter().map(|task| task.id.as_str()));
    tasks
        .iter()
        .zip(keys)
        .map(|(task, key)| TaskRowView {
            key,
            source: task.source.clone(),
            label: task.label.clone(),
            detail: task.detail.clone(),
            task_id: task.id.clone(),
            cancellable: task.cancellable,
            percent: (!task.indeterminate).then(|| task.value.clamp(0.0, 100.0).round()),
            fill: task.value.clamp(0.0, 100.0).round() as f32,
        })
        .collect()
}

/// 弹层开着时各行进度条的目标值（行键、百分比），交给填充过渡（`progress_fill.rs`）按 0.18s 走；关着时为空。
pub(crate) fn task_fill_targets(model: &ShellViewModel) -> Vec<(String, f32)> {
    if !model.admin.popover_open {
        return Vec::new();
    }
    task_row_views(model)
        .into_iter()
        .map(|row| (row.key, row.fill))
        .collect()
}

/// 任务弹层的面板：标题行（「任务」和关闭）、空态和可滚动的任务列表。列表最高 300，任务多时自己滚动。
pub(crate) fn task_panel(view: Signal<TaskPopoverView>) -> AnyView {
    let fills = hot::current(|signals| signals.task_fills);
    let header = widget(
        style::spread(8.0, AlignSpec::Center)
            .with_layout(|layout| layout.min_height = Some(LengthSpec::Px(24.0))),
    )
    .children((
        widget(label("任务", 13.0, 700, Role::Text)).key("admin-task-title"),
        widget(close_button())
            .key("admin-task-close")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::CloseTaskPopover));
            }),
    ))
    .into_any();
    let empty = empty_state("当前没有运行中的任务。")
        .compact(true)
        .visible(move || view.with(|view| view.rows.is_empty()))
        .key("admin-task-empty")
        .into_any();
    let scroll = ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.max_height = Some(LengthSpec::Px(300.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
    });
    let keys = computed(move || {
        view.with(|view| {
            view.rows
                .iter()
                .map(|row| row.key.clone())
                .collect::<Vec<_>>()
        })
    });
    let list = widget(scroll)
        .visible(move || view.with(|view| !view.rows.is_empty()))
        .children((each(keys, Clone::clone, move |key: String| {
            task_item(view, key, fills)
        })
        .gap(10.0)
        .key("admin-task-list"),))
        .key("admin-task-list-scroll")
        .into_any();
    widget(column(10.0).width(LengthSpec::Fill))
        .children((header, empty, list))
        .key("admin-task-popover")
        .into_any()
}

/// 取消排队或运行中的后台任务。仓库操作行和已经在取消的任务不显示。
fn cancel_button(view: Signal<TaskPopoverView>, key: &str) -> AnyView {
    let row_key = key.to_string();
    let task_id = {
        let row_key = row_key.clone();
        move || {
            view.with(|view| {
                view.rows
                    .iter()
                    .find(|row| row.key == row_key)
                    .map(|row| row.task_id.clone())
                    .unwrap_or_default()
            })
        }
    };
    let visible_key = row_key.clone();
    widget(Button::new("取消"))
        .key(format!("admin-task-cancel-{key}"))
        .visible(move || {
            view.with(|view| {
                view.rows
                    .iter()
                    .any(|row| row.key == visible_key && row.cancellable)
            })
        })
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::CancelTask(task_id())));
        })
        .into_any()
}

/// 关闭按钮：22 见方、sm 圆角、弱色叉号 13。显式宽高优先于主题的方形尺寸。
fn close_button() -> IconButton {
    let mut button = IconButton::new(icons::X, "关闭任务");
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(22.0));
    layout.height = Some(LengthSpec::Px(22.0));
    layout.min_width = Some(LengthSpec::Px(22.0));
    layout.min_height = Some(LengthSpec::Px(22.0));
    button.style.foreground = Some(Role::Faint);
    button
}

/// 一个任务：`bg` 底、`border-soft` 边线、sm 圆角、内边距 9；来源加进度条。字段按键从投影里取。
fn task_item(
    view: Signal<TaskPopoverView>,
    key: String,
    fills: Option<Signal<TaskFills>>,
) -> AnyView {
    let pick = {
        let key = key.clone();
        move |read: fn(&TaskRowView) -> String| {
            let key = key.clone();
            move || {
                view.with(|view| {
                    view.rows
                        .iter()
                        .find(|row| row.key == key)
                        .map(read)
                        .unwrap_or_default()
                })
            }
        }
    };
    widget(
        pad(column(6.0), 9.0, 9.0, 9.0, 9.0)
            .surface(Role::Background)
            .border(Role::BorderSoft, 1.0)
            .radius(RadiusTier::Sm),
    )
    .children((
        widget(style::spread(8.0, AlignSpec::Center)).children((
            widget(label(String::new(), 12.0, 600, Role::Faint))
                .key(format!("admin-task-source-{key}"))
                .prop::<String, fields::text::value>(pick(|row| row.source.clone())),
            cancel_button(view, &key),
        )),
        progress_bar(
            view,
            &key,
            fills,
            pick(|row| row.label.clone()),
            pick(|row| row.detail.clone()),
        ),
    ))
    .key(format!("admin-task-{key}"))
    .into_any()
}

/// 进度条：NanaUI 的 `Progress`（6 高的轨道，满格 100）；下面是标签、细节和百分比，间距 7。值按
/// 0.18s ease 过渡，取填充过渡这一帧的值（`fills`；不经 `ShellView` 建树时没有，直接用目标值）。
/// 不确定进度不写百分比，进度条改成沿轨道扫的不确定样式（减少动态效果时由组件改成淡填充）。
fn progress_bar(
    view: Signal<TaskPopoverView>,
    key: &str,
    fills: Option<Signal<TaskFills>>,
    task_label: impl Fn() -> String + Send + 'static,
    detail: impl Fn() -> String + Send + 'static,
) -> AnyView {
    let id = key.to_string();
    let fill_id = id.clone();
    let fill = move || {
        let target = view.with(|view| {
            view.rows
                .iter()
                .find(|row| row.key == fill_id)
                .map_or(0.0, |row| row.fill)
        });
        f64::from(
            fills
                .and_then(|fills| fills.with(|fills| fills.get(&fill_id)))
                .unwrap_or(target),
        )
    };
    let percent = move || {
        view.with(|view| {
            view.rows
                .iter()
                .find(|row| row.key == id)
                .and_then(|row| row.percent)
        })
    };
    let indeterminate = {
        let percent = percent.clone();
        move || percent().is_none()
    };
    let track = progress(100.0)
        .value(fill)
        .indeterminate(indeterminate)
        .key(format!("admin-task-fill-{key}"))
        .into_any();
    let meta = vec![
        widget(style::label_lh(String::new(), 12.0, 600, Role::Text, 1.3))
            .key(format!("admin-task-label-{key}"))
            .prop::<String, fields::text::value>(task_label)
            .into_any(),
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Shrink)
                .grow(1.0)
                .shrink(1.0)
                .min_width(LengthSpec::Px(0.0)),
        )
        .children((
            widget(style::label_lh(String::new(), 12.0, 400, Role::Faint, 1.3))
                .key(format!("admin-task-detail-{key}"))
                .prop::<String, fields::text::value>(detail),
        ))
        .into_any(),
        widget(style::label_lh(String::new(), 12.0, 400, Role::Muted, 1.3))
            .key(format!("admin-task-percent-{key}"))
            .prop::<String, fields::text::value>({
                let percent = percent.clone();
                move || {
                    percent()
                        .map(|percent| format!("{percent}%"))
                        .unwrap_or_default()
                }
            })
            .visible(move || percent().is_some())
            .into_any(),
    ];
    widget(column(7.0))
        .children((
            track,
            widget(row(8.0).width(LengthSpec::Fill))
                .children(meta)
                .into_any(),
        ))
        .into_any()
}
