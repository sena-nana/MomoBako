//! 首页的日志、拓展和动作面板入口，以及任务弹层。
//!
//! 面板根节点是普通竖排，滚动和页边距由壳层主区提供。任务弹层照 `TaskPopover.vue`：
//! 340 宽，贴在侧栏底部任务按钮上方；每个任务一块，来源加进度条（标签、细节、百分比）。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, IconButton, JustifySpec, LengthSpec, Stack};
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};

use super::super::{ShellMessage, ShellViewModel, WorkspacePanel};
use super::icons;
use super::style::{self, action, column, label, pad, row, wrapping, Tone};
use super::support::{action_can_run, action_status_label, PopoverRow};
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

/// 任务弹层。左边对齐任务按钮（x=64），顶边在按钮上方「估计高度 + 8」处。
pub(crate) fn task_popover(model: &ShellViewModel) -> Option<AnyView> {
    if !model.admin.popover_open {
        return None;
    }
    let rows = model.task_rows();
    // 侧栏底部按钮离窗口底边 36（按钮高 26、下边距 10）。
    let anchor = estimated_height(rows.len()) + 8.0 + 36.0;
    let header = widget(style::spread(8.0, AlignSpec::Center).with_layout(|layout| layout.min_height = Some(LengthSpec::Px(24.0))))
        .children((
            widget(label("任务", 13.0, 700, Role::Text)).key("admin-task-title"),
            widget(close_button()).key("admin-task-close").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Admin(AdminMessage::CloseTaskPopover));
            }),
        ))
        .into_any();
    let mut body = vec![header];
    if rows.is_empty() {
        body.push(
            widget(pad(column(0.0), 10.0, 2.0, 4.0, 2.0))
                .children((widget(label("当前没有运行中的任务。", 12.0, 400, Role::Faint)).key("admin-task-empty"),))
                .into_any(),
        );
    } else {
        let items = rows.iter().map(task_item).collect::<Vec<_>>();
        body.push(widget(column(10.0)).children(items).key("admin-task-list").into_any());
    }
    let popover = pad(column(10.0), 11.0, 11.0, 11.0, 11.0)
        .surface(Role::Surface)
        .outline(Role::BorderStrong, 1.0)
        .radius(RadiusTier::Md)
        .with_layout(|layout| {
            layout.width = Some(LengthSpec::Px(340.0));
            layout.max_height = Some(LengthSpec::Px(360.0));
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        });
    let anchored = Stack::column(0.0).align(AlignSpec::Start).with_layout(move |layout| {
        layout.height = Some(LengthSpec::Px(anchor));
        layout.padding_left = Some(LengthSpec::Px(64.0));
        layout.overflow_y = nana_ui_core::OverflowSpec::Visible;
    });
    Some(
        widget(Stack::fill_column(0.0).justify(JustifySpec::End).align(AlignSpec::Start))
            .children((widget(anchored).children((widget(popover).children(body).key("admin-task-popover"),)),))
            .into_any(),
    )
}

/// 关闭按钮：22 见方、sm 圆角、弱色叉号 13。
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

/// 一个任务：`bg` 底、`border-soft` 边线、sm 圆角、内边距 9；来源加进度条。
fn task_item(task: &PopoverRow) -> AnyView {
    let id = task.id.as_str();
    widget(pad(column(6.0), 10.0, 10.0, 10.0, 10.0).surface(Role::Background).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Sm))
        .children((
            widget(label(task.source.clone(), 12.0, 600, Role::Faint)).key(format!("admin-task-source-{id}")),
            progress_bar(task),
        ))
        .key(format!("admin-task-{id}"))
        .into_any()
}

/// `ProgressBar.vue`：6 高的药丸轨道，填充至少 18 宽；下面是标签、细节和百分比，间距 7。
fn progress_bar(task: &PopoverRow) -> AnyView {
    let id = task.id.as_str();
    let percent = task.value.clamp(0.0, 100.0).round();
    let fill = (percent / 100.0) as f32;
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
    .children((widget(style::rounded(Stack::row(0.0), None).surface(Role::Accent).with_layout(move |layout| {
        layout.width = Some(LengthSpec::Percent(fill * 100.0));
        layout.min_width = Some(LengthSpec::Px(18.0));
        layout.height = Some(LengthSpec::Fill);
    })),))
    .into_any();
    let mut meta = vec![widget(style::label_lh(task.label.clone(), 12.0, 600, Role::Text, 1.3)).key(format!("admin-task-label-{id}")).into_any()];
    meta.push(
        widget(Stack::row(0.0).width(LengthSpec::Shrink).grow(1.0).shrink(1.0).min_width(LengthSpec::Px(0.0)))
            .children((widget(style::label_lh(task.detail.clone(), 12.0, 400, Role::Faint, 1.3)).key(format!("admin-task-detail-{id}")),))
            .into_any(),
    );
    if !task.indeterminate {
        meta.push(widget(style::label_lh(format!("{percent}%"), 12.0, 400, Role::Muted, 1.3)).key(format!("admin-task-percent-{id}")).into_any());
    }
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
    let items = admin
        .actions
        .iter()
        .map(|item| {
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
                .key(format!("admin-action-{}", item.action_id))
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(ShellMessage::Admin(AdminMessage::SelectAction(id.clone()))))
                .into_any()
        })
        .collect::<Vec<_>>();
    let list = widget(column(0.0).surface(Role::Surface).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Md)).children(items).key("admin-action-list").into_any();
    let detail = active.map(|current| action_detail(model, current)).unwrap_or_else(|| widget(column(0.0)).into_any());
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid_columns = Some(vec![
            nana_ui_core::GridTrack::MinMax { min_px: 220.0, fr: 1.0, max_px: Some(300.0) },
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
                cx.dispatch_program(ShellMessage::Admin(AdminMessage::RunAction(Some(action_id.clone()))));
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
            widget(pad(style::spread(10.0, AlignSpec::Start), 11.0, 11.0, 11.0, 11.0).surface(Role::Subtle).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Sm))
                .children(children)
                .into_any()
        })
        .collect::<Vec<_>>();
    body.push(widget(pad(column(8.0), 12.0, 0.0, 0.0, 0.0)).children(steps).into_any());
    widget(pad(column(0.0), 15.0, 15.0, 15.0, 15.0).surface(Role::Surface).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Md))
        .children(body)
        .key("admin-action-detail-card")
        .into_any()
}
