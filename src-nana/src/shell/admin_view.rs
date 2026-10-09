//! 任务弹层。首页的日志、拓展和动作面板是常驻路由，见 `route_admin.rs`。
//!
//! 任务弹层照 `TaskPopover.vue`：340 宽，贴在侧栏底部任务按钮上方；每个任务一块，
//! 来源加进度条（标签、细节、百分比）。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, IconButton, JustifySpec, LengthSpec, ScrollAxes, ScrollView, Stack};
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};

use super::super::{ShellMessage, ShellViewModel};
use super::icons;
use super::style::{self, column, label, pad, row};
use super::support::PopoverRow;
use super::AdminMessage;

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
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::CloseTaskPopover));
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
        let keys = style::unique_keys(rows.iter().map(|task| task.id.as_str()));
        let items = rows.iter().zip(&keys).map(|(task, key)| task_item(task, key)).collect::<Vec<_>>();
        // `.task-popover__list`：弹层最高 360，任务多时列表自己滚动。
        let scroll = ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
            layout.flex_grow = Some(1.0);
            layout.flex_shrink = Some(1.0);
            layout.min_height = Some(LengthSpec::Px(0.0));
        });
        let list = widget(column(10.0)).children(items).key("admin-task-list");
        body.push(widget(scroll).children((list,)).key("admin-task-list-scroll").into_any());
    }
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
    // 默认的方形尺寸取主题的图标按钮档（28），这里按 Vue 写死的 22。
    button.style.square = None;
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(22.0));
    layout.height = Some(LengthSpec::Px(22.0));
    layout.min_width = Some(LengthSpec::Px(22.0));
    layout.min_height = Some(LengthSpec::Px(22.0));
    button.style.foreground = Some(Role::Faint);
    button
}

/// 一个任务：`bg` 底、`border-soft` 边线、sm 圆角、内边距 9；来源加进度条。
fn task_item(task: &PopoverRow, id: &str) -> AnyView {
    widget(pad(column(6.0), 9.0, 9.0, 9.0, 9.0).surface(Role::Background).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Sm))
        .children((
            widget(label(task.source.clone(), 12.0, 600, Role::Faint)).key(format!("admin-task-source-{id}")),
            progress_bar(task, id),
        ))
        .key(format!("admin-task-{id}"))
        .into_any()
}

/// `ProgressBar.vue`：6 高的药丸轨道，填充至少 18 宽；下面是标签、细节和百分比，间距 7。
fn progress_bar(task: &PopoverRow, id: &str) -> AnyView {
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
