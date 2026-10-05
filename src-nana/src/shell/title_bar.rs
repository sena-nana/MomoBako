//! 全壳共用的 Nana 标题栏。
//!
//! 窗口按钮用 `AppTitleBar` 自带的自定义控件槽。点击后再送进现有的
//! 最小化、最大化切换和关闭确认，不让关闭按钮直接关掉窗口。

use std::sync::Arc;

use nana_ui::runtime::view::{icon_button, widget, IntoView};
use nana_ui::runtime::{
    Activate, AppTitleBar, Entity, IconButton, LengthSpec, RuntimeDocument, TextChanged, TextInput,
};
use nana_ui::{ControlSize, Icon};

use super::{InspectMessage, ShellMessage, ShellViewModel};

/// 左侧侧栏开关，中间全局搜索和筛选开关。窗口控件槽保持打开。
pub(super) fn title_bar(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let sidebar_label = if model.workspace.sidebar_collapsed {
        "展开侧边栏"
    } else {
        "折叠侧边栏"
    };
    let filter_label = if model.inspect.filter_bar_open {
        "隐藏筛选栏"
    } else {
        "显示筛选栏"
    };
    let query = model.inspect.query.clone();
    widget(
        AppTitleBar::new("MomoBako")
            .show_window_controls(true)
            .native_controls(false)
            .center_width(460.0),
    )
    .leading(
        icon_button(Icon::Sidebar, sidebar_label)
            .key("sidebar-toggle")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::ToggleSidebar);
            }),
    )
    .center(widget(nana_ui::runtime::Stack::row(4.0)).children((
        widget(search_field(query))
            .key("global-search")
            .on_cx(|_, event: &TextChanged, cx| {
                cx.dispatch_program(ShellMessage::Inspect(InspectMessage::SetQuery(
                    event.value.to_string(),
                )));
            }),
        widget(filter_button(filter_label))
            .key("filter-toggle")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::Inspect(InspectMessage::ToggleFilterBar));
            }),
    )))
}

/// 28 像素的筛选图标。无障碍名称仍是「显示筛选栏」或「隐藏筛选栏」。
fn filter_button(label: &str) -> IconButton {
    let mut button = IconButton::new(nana_ui::icons_tabler::ADJUSTMENTS_HORIZONTAL, label)
        .size(ControlSize::Small);
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(28.0));
    layout.height = Some(LengthSpec::Px(28.0));
    layout.min_width = Some(LengthSpec::Px(28.0));
    layout.min_height = Some(LengthSpec::Px(28.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    button
}

/// 搜索框不用百分百宽度，给筛选开关留出中间槽里的可见位置。
fn search_field(query: String) -> TextInput {
    let mut field = TextInput::new(query)
        .label("全局搜索")
        .placeholder("搜索文件名、标签、元数据")
        .size(ControlSize::Small);
    let layout = Arc::make_mut(&mut field.style.layout);
    layout.width = Some(LengthSpec::Px(360.0));
    layout.min_width = Some(LengthSpec::Px(360.0));
    layout.max_width = Some(LengthSpec::Px(360.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    field
}

/// 把组件生成的 Minimize / Maximize / Close 接上现有窗口动作。
///
/// 这些按钮由 Nana 在组装标题栏时创建，视图闭包里拿不到它们的节点。
/// 无障碍点击走 `Activate`，所以要在挂载后再登记。
pub(super) fn bind_window_controls(document: &mut RuntimeDocument) -> Result<(), nana_ui::runtime::FrameworkError> {
    let document_id = document.document();
    let found: Vec<_> = document
        .context()
        .world()
        .project_accessibility(document_id)
        .into_iter()
        .filter_map(|node| {
            let action = match node.label.as_deref() {
                Some("Minimize") => super::WindowAction::Minimize,
                Some("Maximize" | "Restore") => super::WindowAction::ToggleMaximize,
                Some("Close") => super::WindowAction::Close,
                _ => return None,
            };
            Some((node.id, action))
        })
        .collect();
    if found.len() < 3 {
        eprintln!("Nana 标题栏窗口控件不足：{}", found.len());
    }
    for (id, action) in found {
        let entity = Entity::<IconButton>::from_stable_id(id);
        if let Err(error) = document.context_mut().on_keyed(
            entity,
            "momobako-window",
            move |_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::WindowAction(action));
            },
        ) {
            eprintln!("Nana 窗口控件没有接上点击：{error}");
        }
    }
    Ok(())
}
