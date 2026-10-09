//! 全壳共用的 Nana 标题栏。
//!
//! 结构照 Vue `AppShell.vue` 和 `WorkspaceTitleBarSearch.vue`：左侧折叠侧栏按钮，中间是全局搜索
//! 和筛选开关。搜索框 `--bg-subtle` 底、无描边、占位文字居中，悬停或聚焦换 `--bg-hover`；
//! 筛选开关 28×28，筛选栏打开或有筛选条件时 `--accent-soft` 底、强调色，并在右上角显示条件数。
//! 窗口按钮用 `AppTitleBar` 自带的自定义控件槽，点击后再送进现有的最小化、最大化切换和关闭确认。

use std::sync::Arc;

use nana_ui::icons_tabler::{ADJUSTMENTS_HORIZONTAL, LAYOUT_SIDEBAR_LEFT_COLLAPSE, LAYOUT_SIDEBAR_LEFT_EXPAND};
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, AppTitleBar, Entity, IconButton, JustifySpec, LengthSpec, RadiusTier, RuntimeDocument,
    SemanticColorRole, SemanticPaint, Stack, TextChanged, TextHorizontalAlignment, TextInput,
};
use nana_ui::{ControlSize, Icon};

use super::sidebar_view::parts;
use super::workspace::WorkspacePanel;
use super::{InspectMessage, ShellMessage, ShellViewModel};

/// 搜索框宽度：Vue 搜索组 392px，其中筛选开关 28px、间距 4px。
const SEARCH_WIDTH: f32 = 360.0;
const CONTROL_EDGE: f32 = 28.0;

/// 左侧侧栏开关，中间全局搜索和筛选开关。窗口控件槽保持打开。
pub(super) fn title_bar(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let collapsed = model.workspace.sidebar_collapsed;
    let sidebar_label = if collapsed { "展开侧边栏" } else { "折叠侧边栏" };
    let sidebar_icon = if collapsed { LAYOUT_SIDEBAR_LEFT_EXPAND } else { LAYOUT_SIDEBAR_LEFT_COLLAPSE };
    let filter_open = model.inspect.filter_bar_open;
    let filter_label = if filter_open { "隐藏筛选栏" } else { "显示筛选栏" };
    let filter_count = model.inspect.active_filter_count();
    let query = model.inspect.query.clone();
    widget(
        AppTitleBar::new("MomoBako")
            .show_window_controls(true)
            .native_controls(false)
            .center_width(460.0),
    )
    .leading(
        widget(chrome_button(sidebar_icon, sidebar_label, false))
            .key("sidebar-toggle")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::ToggleSidebar);
            }),
    )
    .center(widget(Stack::row(4.0).align(AlignSpec::Center)).children((
        widget(search_field(query)).key("global-search").on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program_all(ShellMessage::SetWorkspacePanel(WorkspacePanel::Search));
            cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::SetQuery(event.value.to_string())));
        }),
        filter_toggle(filter_label, filter_open || filter_count > 0, filter_count),
    )))
}

/// 标题栏图标按钮：28×28、`--radius-sm`、`--text-muted`，悬停 `--bg-hover`。对应 `.titlebar__btn`。
fn chrome_button(icon: Icon, label: &'static str, disabled: bool) -> IconButton {
    let mut button = IconButton::new(icon, label).size(ControlSize::Large).disabled(disabled).with_tooltip(label);
    button.style = parts::icon_button_style(CONTROL_EDGE, RadiusTier::Sm, SemanticColorRole::Muted, None, disabled);
    button.colors_from_style()
}

/// 28 像素图标按钮。文字只作无障碍名称，不画在按钮上。缺失仓库页的警示图标也用它。
pub(super) fn shell_icon(icon: Icon, label: impl Into<Arc<str>>, disabled: bool) -> IconButton {
    let mut button = IconButton::new(icon, label).size(ControlSize::Small).disabled(disabled);
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(CONTROL_EDGE));
    layout.height = Some(LengthSpec::Px(CONTROL_EDGE));
    layout.min_width = Some(LengthSpec::Px(CONTROL_EDGE));
    layout.min_height = Some(LengthSpec::Px(CONTROL_EDGE));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    button
}

/// 筛选开关。打开或有条件时强调色底；条件数大于 0 时右上角 14px 角标。对应 `.titlebar__filter-btn`。
fn filter_toggle(label: &'static str, active: bool, count: usize) -> AnyView {
    let mut button = IconButton::new(ADJUSTMENTS_HORIZONTAL, label).size(ControlSize::Large).with_tooltip("筛选");
    let mut style = parts::icon_button_style(CONTROL_EDGE, RadiusTier::Sm, SemanticColorRole::Muted, Some(SemanticColorRole::Subtle), false);
    let accent = SemanticPaint {
        background: Some(SemanticColorRole::AccentSoft),
        foreground: Some(SemanticColorRole::Accent),
        ..SemanticPaint::default()
    };
    style.interaction.hovered = accent;
    style.interaction.pressed = accent;
    if active {
        style.background = Some(SemanticColorRole::AccentSoft);
        style.foreground = Some(SemanticColorRole::Accent);
    }
    button.style = style;
    let toggle = widget(button.colors_from_style()).key("filter-toggle").on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program_all(ShellMessage::SetWorkspacePanel(WorkspacePanel::Search));
        cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::ToggleFilterBar));
    });
    if count == 0 {
        return toggle.into_any();
    }
    let badge = widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .surface(SemanticColorRole::Accent)
            .radius_px(999.0)
            .with_layout(|layout| {
                layout.position = nana_ui_core::PositionSpec::Absolute;
                layout.offset_top = Some(LengthSpec::Px(-4.0));
                layout.offset_right = Some(LengthSpec::Px(-4.0));
                layout.min_width = Some(LengthSpec::Px(14.0));
                layout.height = Some(LengthSpec::Px(14.0));
                layout.padding_left = Some(LengthSpec::Px(4.0));
                layout.padding_right = Some(LengthSpec::Px(4.0));
                layout.pointer_events = Some(nana_ui_core::PointerEventsSpec::None);
            }),
    )
    .children((widget(parts::label_text(count.to_string(), 10.0, 700, Some(SemanticColorRole::Background)).line_height(14.0)),))
    .key("filter-count");
    widget(Stack::row(0.0).with_layout(|layout| layout.position = nana_ui_core::PositionSpec::Relative))
        .children((toggle, badge))
        .into_any()
}

/// 搜索框：`--bg-subtle` 底、无描边、左右 10px、文字和占位居中；悬停或聚焦换 `--bg-hover`。
fn search_field(query: String) -> TextInput {
    let field = TextInput::new(query)
        .label("全局搜索")
        .placeholder("搜索文件名、标签、元数据")
        .size(ControlSize::Small);
    let mut style = field.style.clone();
    style.background = Some(SemanticColorRole::Subtle);
    style.border = None;
    style.radius = Some(RadiusTier::Sm);
    style.control_padding_x = None;
    let hover = SemanticPaint { background: Some(SemanticColorRole::Hover), ..SemanticPaint::default() };
    style.interaction.hovered = hover;
    style.interaction.focused = hover;
    style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Px(SEARCH_WIDTH));
        layout.min_width = Some(LengthSpec::Px(SEARCH_WIDTH));
        layout.max_width = Some(LengthSpec::Px(SEARCH_WIDTH));
        layout.height = Some(LengthSpec::Px(CONTROL_EDGE));
        layout.padding_left = Some(LengthSpec::Px(10.0));
        layout.padding_right = Some(LengthSpec::Px(10.0));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        layout.font_size = Some(14.0);
        layout.border_width = Some(0.0);
    }
    field.style(style)
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
                cx.dispatch_program_all(ShellMessage::WindowAction(action));
            },
        ) {
            eprintln!("Nana 窗口控件没有接上点击：{error}");
        }
    }
    Ok(())
}
