//! 全壳共用的 Nana 标题栏。
//!
//! 结构照 Vue `AppShell.vue` 和 `WorkspaceTitleBarSearch.vue`：左侧折叠侧栏按钮，中间是全局搜索
//! 和筛选开关。搜索框 `--bg-subtle` 底、无描边、占位文字居中，悬停或聚焦换 `--bg-hover`；
//! 筛选开关 28×28，筛选栏打开或有筛选条件时 `--accent-soft` 底、强调色，并在右上角显示条件数。
//! 窗口按钮用 `AppTitleBar` 自带的自定义控件：指针点击由 Nana 宿主执行（关闭经 `close_requested`
//! 走关闭确认），键盘和读屏的激活由这里接到同一套窗口动作。

use std::sync::Arc;

use nana_ui::icons_tabler::{ADJUSTMENTS_HORIZONTAL, LAYOUT_SIDEBAR_LEFT_COLLAPSE, LAYOUT_SIDEBAR_LEFT_EXPAND};
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    component_descriptors, Activate, AlignSpec, AppTitleBar, AppTitleBarControls, Entity, IconButton, JustifySpec,
    LengthSpec, RadiusTier, RuntimeDocument, SemanticColorRole, SemanticPaint, Stack, StableNodeId, TextChanged,
    TextHorizontalAlignment, TextInput,
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

/// 把标题栏的三个窗口按钮接上窗口动作，供键盘和读屏激活。
///
/// 指针点击由 Nana 宿主直接执行；这里发出的是同一套绝对命令（最大化按当前窗口状态取最大化或还原），
/// 和宿主重复时结果不变。按钮由 Nana 在组装标题栏时创建，视图闭包里拿不到节点，所以挂载后按按钮组的
/// 次序登记：最小化、最大化/还原、关闭，和宿主的映射一致；不按会随框架文案表变化的无障碍名称匹配。
pub(super) fn bind_window_controls(document: &mut RuntimeDocument) -> Result<(), nana_ui::runtime::FrameworkError> {
    let buttons = window_control_buttons(document);
    if buttons.iter().any(Option::is_none) {
        eprintln!("Nana 标题栏窗口控件不全：{buttons:?}");
    }
    let actions = [super::WindowAction::Minimize, super::WindowAction::ToggleMaximize, super::WindowAction::Close];
    for (id, action) in buttons.into_iter().zip(actions).filter_map(|(id, action)| Some((id?, action))) {
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

/// 标题栏按钮组里的最小化、最大化/还原、关闭三个按钮。
///
/// 按钮组写明了节点就用它，否则按子节点次序取，和 Nana 宿主认按钮的规则一致。找不到的位置是 `None`。
fn window_control_buttons(document: &RuntimeDocument) -> [Option<StableNodeId>; 3] {
    let context = document.context();
    let world = context.world();
    let Some(bar) = world.nodes_of_component(document.document(), component_descriptors::APP_TITLE_BAR.type_id).next() else {
        eprintln!("Nana 文档里没有标题栏，窗口按钮没有接上");
        return [None; 3];
    };
    let Some(controls) = context.read(Entity::<AppTitleBar>::from_stable_id(bar), |bar| bar.controls).ok().flatten() else {
        eprintln!("Nana 标题栏还没有窗口按钮组");
        return [None; 3];
    };
    let explicit = context
        .read(Entity::<AppTitleBarControls>::from_stable_id(controls), |group| [group.minimize, group.maximize, group.close])
        .unwrap_or([None; 3]);
    if explicit.iter().any(Option::is_some) {
        return explicit;
    }
    let children = world.node(controls).map(|node| node.children).unwrap_or_default();
    [children.first().copied(), children.get(1).copied(), children.get(2).copied()]
}
