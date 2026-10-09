//! 首页的日志、拓展和动作面板入口，以及任务弹层。
//!
//! 面板根节点是普通竖排，滚动和页边距由壳层主区提供。任务弹层照 `TaskPopover.vue`：
//! 340 宽，贴在侧栏底部任务按钮上方；每个任务一块，来源加进度条（标签、细节、百分比）。
//! 任务弹层常驻：任务按键增删行，进度、标签和细节只改绑定的字段；点弹层外面关闭（Vue 的外部按下）。

use std::sync::Arc;

use nana_ui::runtime::view::{computed, each, fields, signal, widget, AnyView, FieldWrite, IntoView, Signal, StyledComponent};
use nana_ui::runtime::{Activate, AlignSpec, IconButton, JustifySpec, LengthSpec, ListItem, ScrollAxes, ScrollView, Stack};
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};

use super::super::hot::FillPercentField;
use super::super::view_part_overlay::session::Projected;
use super::super::{ShellMessage, ShellViewModel, WorkspacePanel};
use super::icons;
use super::style::{self, action, column, label, pad, row, wrapping, Tone};
use super::support::{action_can_run, action_status_label};
use super::AdminMessage;

/// 当前首页面板：日志、拓展或仓库动作。设置页走 [`super::settings_page`]。
pub(crate) fn admin_surface(model: &ShellViewModel) -> AnyView {
    if model.admin_workspace_visible(WorkspacePanel::Logs) {
        return super::logs_view::logs_panel(model);
    }
    if model.admin_workspace_visible(WorkspacePanel::Extensions) {
        return super::tools::extensions_page(model);
    }
    if model.admin_workspace_visible(WorkspacePanel::Actions) {
        return actions_panel(model);
    }
    widget(column(0.0)).key("admin-surface").into_any()
}

/// Vue 定位弹层用的高度估计：`min(360, max(180, 96 + 任务数 × 70))`。
fn estimated_height(count: usize) -> f32 {
    (96.0 + count as f32 * 70.0).clamp(180.0, 360.0)
}

/// 任务弹层的一行。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TaskRowView {
    /// 行的键：任务编号，重复时加序号。
    pub key: String,
    pub source: String,
    pub label: String,
    pub detail: String,
    /// 四舍五入后的百分比；不确定进度时为 `None`，不显示百分比。
    pub percent: Option<f64>,
    /// 进度轨填充的百分比（不确定进度时也按当前值画）。
    pub fill: f32,
}

/// 任务弹层要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TaskPopoverView {
    pub rows: Vec<TaskRowView>,
    /// 弹层底边到窗口底边的高度：估计高度加 8，再加按钮离底边的 36。
    pub anchor: f32,
}

impl TaskPopoverView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        if !model.admin.popover_open {
            return None;
        }
        let tasks = model.task_rows();
        let keys = style::unique_keys(tasks.iter().map(|task| task.id.as_str()));
        let rows = tasks
            .iter()
            .zip(keys)
            .map(|(task, key)| TaskRowView {
                key,
                source: task.source.clone(),
                label: task.label.clone(),
                detail: task.detail.clone(),
                percent: (!task.indeterminate).then(|| task.value.clamp(0.0, 100.0).round()),
                fill: ((task.value.clamp(0.0, 100.0).round() / 100.0) as f32) * 100.0,
            })
            .collect::<Vec<_>>();
        // 侧栏底部按钮离窗口底边 36（按钮高 26、下边距 10）。
        Some(Self { anchor: estimated_height(rows.len()) + 8.0 + 36.0, rows })
    }
}

/// 任务弹层。左边对齐任务按钮（x=64），顶边在按钮上方「估计高度 + 8」处。
pub(crate) fn task_popover(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(TaskPopoverView::project(model)?);
    Projected::register(view, TaskPopoverView::project);
    let header = widget(style::spread(8.0, AlignSpec::Center).with_layout(|layout| layout.min_height = Some(LengthSpec::Px(24.0))))
        .children((
            widget(label("任务", 13.0, 700, Role::Text)).key("admin-task-title"),
            widget(close_button()).key("admin-task-close").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::CloseTaskPopover));
            }),
        ))
        .into_any();
    let empty = widget(pad(column(0.0), 10.0, 2.0, 4.0, 2.0))
        .visible(move || view.with(|view| view.rows.is_empty()))
        .children((widget(label("当前没有运行中的任务。", 12.0, 400, Role::Faint)).key("admin-task-empty"),))
        .into_any();
    // `.task-popover__list`：弹层最高 360，任务多时列表自己滚动。
    let scroll = ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.min_height = Some(LengthSpec::Px(0.0));
    });
    let keys = computed(move || view.with(|view| view.rows.iter().map(|row| row.key.clone()).collect::<Vec<_>>()));
    let list = widget(scroll)
        .visible(move || view.with(|view| !view.rows.is_empty()))
        .children((each(keys, Clone::clone, move |key: String| task_item(view, key)).gap(10.0).key("admin-task-list"),))
        .key("admin-task-list-scroll")
        .into_any();
    let popover = pad(column(10.0), 10.0, 10.0, 10.0, 10.0)
        .surface(Role::Surface)
        .outline(Role::BorderStrong, 1.0)
        .radius(RadiusTier::Md)
        .with_layout(|layout| {
            layout.width = Some(LengthSpec::Px(340.0));
            layout.max_height = Some(LengthSpec::Px(360.0));
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        });
    let anchored = Stack::column(0.0).align(AlignSpec::Start).with_layout(move |layout| {
        layout.padding_left = Some(LengthSpec::Px(64.0));
        layout.overflow_y = nana_ui_core::OverflowSpec::Visible;
    });
    Some(
        widget(Stack::fill_column(0.0).justify(JustifySpec::End).align(AlignSpec::Start).with_layout(|layout| {
            layout.position = nana_ui_core::PositionSpec::Relative;
        }))
        .children((
            dismiss_layer(),
            widget(anchored)
                .prop::<f32, AnchorHeight>(move || view.with(|view| view.anchor))
                .children((widget(popover).children((header, empty, list)).key("admin-task-popover"),)),
        ))
        .into_any(),
    )
}

/// 弹层外的透明点击层：点弹层外面关闭，和 Vue 的外部按下一致。
fn dismiss_layer() -> AnyView {
    let mut style = nana_ui::runtime::NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.position = nana_ui_core::PositionSpec::Absolute;
        layout.offset_left = Some(LengthSpec::Px(0.0));
        layout.offset_top = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Percent(100.0));
        layout.height = Some(LengthSpec::Percent(100.0));
    }
    widget(ListItem::new("关闭任务弹层").style(style))
        .content(widget(Stack::row(0.0)))
        .key("admin-task-dismiss")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::CloseTaskPopover)))
        .into_any()
}

/// 弹层锚点那一层的高度：任务数变了，弹层顶边跟着估计高度挪。
struct AnchorHeight;

impl FieldWrite<Stack, f32> for AnchorHeight {
    const FIELD: &'static str = "Stack.style.layout.height";

    fn write(target: &mut Stack, height: f32) {
        Arc::make_mut(&mut target.node_style_mut().layout).height = Some(LengthSpec::Px(height));
    }

    fn differs(target: &Stack, height: &f32) -> bool {
        target.node_style().layout.height != Some(LengthSpec::Px(*height))
    }
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
fn task_item(view: Signal<TaskPopoverView>, key: String) -> AnyView {
    let pick = {
        let key = key.clone();
        move |read: fn(&TaskRowView) -> String| {
            let key = key.clone();
            move || view.with(|view| view.rows.iter().find(|row| row.key == key).map(read).unwrap_or_default())
        }
    };
    widget(pad(column(6.0), 9.0, 9.0, 9.0, 9.0).surface(Role::Background).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Sm))
        .children((
            widget(label(String::new(), 12.0, 600, Role::Faint))
                .key(format!("admin-task-source-{key}"))
                .prop::<String, fields::text::value>(pick(|row| row.source.clone())),
            progress_bar(view, &key, pick(|row| row.label.clone()), pick(|row| row.detail.clone())),
        ))
        .key(format!("admin-task-{key}"))
        .into_any()
}

/// `ProgressBar.vue`：6 高的药丸轨道，填充至少 18 宽；下面是标签、细节和百分比，间距 7。
fn progress_bar(
    view: Signal<TaskPopoverView>,
    key: &str,
    task_label: impl Fn() -> String + Send + 'static,
    detail: impl Fn() -> String + Send + 'static,
) -> AnyView {
    let id = key.to_string();
    let fill_id = id.clone();
    let fill = move || view.with(|view| view.rows.iter().find(|row| row.key == fill_id).map_or(0.0, |row| row.fill));
    let percent = move || view.with(|view| view.rows.iter().find(|row| row.key == id).and_then(|row| row.percent));
    let track = widget(
        style::rounded(Stack::row(0.0), None)
            .surface(Role::Subtle)
            .outline(Role::BorderSoft, 1.0)
            .with_layout(|layout| {
                layout.width = Some(LengthSpec::Fill);
                layout.height = Some(LengthSpec::Px(6.0));
                layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
                layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
            }),
    )
    .children((widget(style::rounded(Stack::row(0.0), None).surface(Role::Accent).with_layout(|layout| {
        layout.min_width = Some(LengthSpec::Px(18.0));
        layout.height = Some(LengthSpec::Fill);
    }))
    .prop::<f32, FillPercentField>(fill),))
    .into_any();
    let meta = vec![
        widget(style::label_lh(String::new(), 12.0, 600, Role::Text, 1.3))
            .key(format!("admin-task-label-{key}"))
            .prop::<String, fields::text::value>(task_label)
            .into_any(),
        widget(Stack::row(0.0).width(LengthSpec::Shrink).grow(1.0).shrink(1.0).min_width(LengthSpec::Px(0.0)))
            .children((widget(style::label_lh(String::new(), 12.0, 400, Role::Faint, 1.3))
                .key(format!("admin-task-detail-{key}"))
                .prop::<String, fields::text::value>(detail),))
            .into_any(),
        widget(style::label_lh(String::new(), 12.0, 400, Role::Muted, 1.3))
            .key(format!("admin-task-percent-{key}"))
            .prop::<String, fields::text::value>({
                let percent = percent.clone();
                move || percent().map(|percent| format!("{percent}%")).unwrap_or_default()
            })
            .visible(move || percent().is_some())
            .into_any(),
    ];
    widget(column(7.0))
        .children((track, widget(row(8.0).width(LengthSpec::Fill)).children(meta).into_any()))
        .into_any()
}

/// 仓库动作面板，照 `RepositoryActionsPanel.vue`。
fn actions_panel(model: &ShellViewModel) -> AnyView {
    let admin = &model.admin;
    let header = widget(style::spread(12.0, AlignSpec::Center))
        .children((
            widget(column(0.0)).children((
                widget(style::eyebrow_text("动作")).key("admin-actions-eyebrow"),
                widget(style::label_lh("仓库动作", 18.0, 700, Role::Text, 1.25)).key("admin-actions-title"),
            )),
            style::hint_chip(format!("{} 项", admin.actions.len()), "admin-actions-count"),
        ))
        .into_any();
    let mut body = vec![header];
    if admin.actions_loading {
        body.push(style::state_notice("正在加载动作".into(), false, "admin-actions-loading"));
    } else if admin.actions.is_empty() {
        body.push(style::state_notice("当前仓库没有导入动作。".into(), false, "admin-actions-empty"));
    } else {
        body.push(action_body(model));
    }
    if !admin.actions_error.is_empty() {
        body.push(style::state_notice(admin.actions_error.clone(), true, "admin-actions-error"));
    }
    widget(pad(column(12.0), 18.0, 18.0, 18.0, 18.0)).children(body).key("admin-actions").into_any()
}

/// 左边动作列表（220–300 宽），右边选中动作的详情和步骤。
fn action_body(model: &ShellViewModel) -> AnyView {
    let admin = &model.admin;
    let active = admin.actions.iter().find(|action| Some(action.action_id.as_str()) == admin.active_action_id.as_deref()).or(admin.actions.first());
    let keys = style::unique_keys(admin.actions.iter().map(|action| action.action_id.as_str()));
    let items = admin
        .actions
        .iter()
        .zip(&keys)
        .map(|(item, key)| {
            let id = item.action_id.clone();
            let selected = active.is_some_and(|current| current.action_id == item.action_id);
            let mut node = nana_ui::runtime::NodeStyle {
                background: selected.then_some(Role::Hover),
                interaction: nana_ui::runtime::InteractionStyle {
                    hovered: nana_ui::runtime::SemanticPaint { background: Some(Role::Hover), ..Default::default() },
                    ..Default::default()
                },
                ..Default::default()
            };
            {
                let layout = std::sync::Arc::make_mut(&mut node.layout);
                layout.width = Some(LengthSpec::Fill);
                layout.min_height = Some(LengthSpec::Px(54.0));
                layout.padding_top = Some(LengthSpec::Px(10.0));
                layout.padding_bottom = Some(LengthSpec::Px(10.0));
                layout.padding_left = Some(LengthSpec::Px(12.0));
                layout.padding_right = Some(LengthSpec::Px(12.0));
                layout.direction = Some(nana_ui_core::FlexDirection::Row);
                layout.align_items = AlignSpec::Center;
                layout.justify_content = JustifySpec::SpaceBetween;
                layout.gap = Some(LengthSpec::Px(10.0));
            }
            let mut children = vec![widget(Stack::column(3.0).width(LengthSpec::Shrink).grow(1.0).shrink(1.0).min_width(LengthSpec::Px(0.0)))
                .children((
                    widget(label(item.name.clone(), 13.0, 600, Role::Text)),
                    widget(wrapping(label(
                        format!("{} · {} 步 · {}", item.source, item.steps.len(), action_status_label(&item.status, item.enabled)),
                        12.0,
                        400,
                        Role::Muted,
                    ))),
                ))
                .into_any()];
            if item.status != "ready" {
                children.push(widget(style::glyph(icons::SHIELD_ALERT, 14.0, Role::Text)).into_any());
            }
            widget(style::bottom_rule(Stack::row(10.0).style(node)).hittable())
                .children(children)
                .key(format!("admin-action-{key}"))
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SelectAction(id.clone()))))
                .into_any()
        })
        .collect::<Vec<_>>();
    let list = widget(column(0.0).surface(Role::Surface).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Md)).children(items).key("admin-action-list").into_any();
    let detail = active.map(|current| action_detail(model, current)).unwrap_or_else(|| widget(column(0.0)).into_any());
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid_columns = Some(vec![
            style::capped(220.0, 300.0),
            nana_ui_core::GridTrack::MinMax { min_px: 0.0, fr: 1.0, max_px: None },
        ]);
        layout.gap = Some(LengthSpec::Px(12.0));
        layout.width = Some(LengthSpec::Fill);
        layout.align_items = AlignSpec::Start;
    });
    widget(grid).children((list, detail)).into_any()
}

fn action_detail(model: &ShellViewModel, current: &crate::backend::services::repository::RepositoryAction) -> AnyView {
    let admin = &model.admin;
    let selected = model.files.selected_paths().len();
    let can_run = action_can_run(&current.status, current.enabled, selected, admin.actions_running);
    let action_id = current.action_id.clone();
    let last = current.last_run.as_ref().map(|run| run.status.clone()).unwrap_or_else(|| "无".into());
    let run_icon = if admin.actions_running { icons::LOADER_CIRCLE } else { icons::PLAY };
    let mut body = vec![widget(style::spread(12.0, AlignSpec::Center))
        .children((
            widget(column(4.0)).children((
                widget(style::label_lh(current.name.clone(), 18.0, 700, Role::Text, 1.25)),
                widget(label(format!("{} · 最近运行 {last}", action_status_label(&current.status, current.enabled)), 12.0, 400, Role::Muted)).key("admin-action-detail"),
            )),
            widget(action("执行", Some(run_icon), Tone::Primary, !can_run)).key("admin-action-run").on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RunAction(Some(action_id.clone()))));
            }),
        ))
        .into_any()];
    if let Some(reason) = current.unsupported_reason.clone() {
        body.push(style::state_notice(reason, true, "admin-action-unsupported"));
    }
    let steps = current
        .steps
        .iter()
        .map(|step| {
            let mut children = vec![widget(column(3.0).shrink(1.0)).children((
                widget(label(step.label.clone(), 13.0, 600, Role::Text)),
                widget(label(format!("{} · {}", step.step_kind, step.status), 12.0, 400, Role::Muted)),
            ))
            .into_any()];
            if let Some(reason) = step.unsupported_reason.clone() {
                children.push(widget(style::align_end(label(reason, 12.0, 400, Role::Danger))).into_any());
            }
            widget(pad(style::spread(10.0, AlignSpec::Start), 10.0, 10.0, 10.0, 10.0).surface(Role::Subtle).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Sm))
                .children(children)
                .into_any()
        })
        .collect::<Vec<_>>();
    body.push(widget(pad(column(8.0), 12.0, 0.0, 0.0, 0.0)).children(steps).into_any());
    widget(pad(column(0.0), 14.0, 14.0, 14.0, 14.0).surface(Role::Surface).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Md))
        .children(body)
        .key("admin-action-detail-card")
        .into_any()
}
