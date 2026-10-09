//! 全壳共用的 Nana 标题栏。
//!
//! 结构照 Vue `AppShell.vue` 和 `WorkspaceTitleBarSearch.vue`：左侧折叠侧栏按钮，中间是全局搜索
//! 和筛选开关。搜索框 `--bg-subtle` 底、无描边、占位文字居中，悬停或聚焦换 `--bg-hover`；
//! 筛选开关 28×28，筛选栏打开或有筛选条件时 `--accent-soft` 底、强调色，并在右上角显示条件数。
//! 窗口按钮用 `AppTitleBar` 自带的自定义控件：指针点击由 Nana 宿主执行（关闭经 `close_requested`
//! 走关闭确认），键盘和读屏的激活由这里接到同一套窗口动作。

use std::sync::Arc;

use nana_ui::icons_tabler::{ADJUSTMENTS_HORIZONTAL, LAYOUT_SIDEBAR_LEFT_COLLAPSE, LAYOUT_SIDEBAR_LEFT_EXPAND};
use nana_ui::runtime::view::{fields, widget, AnyView, FieldWrite, IntoView, Signal};
use nana_ui::runtime::{
    component_descriptors, Activate, AlignSpec, AppTitleBar, AppTitleBarControls, Entity, IconButton, JustifySpec,
    LengthSpec, NodeStyle, RadiusTier, RuntimeDocument, SemanticColorRole, SemanticPaint, Stack, StableNodeId,
    TextChanged, TextHorizontalAlignment, TextInput,
};
use nana_ui::{ControlSize, Icon};

use super::hot::{FilterLook, TitleSignals};
use super::sidebar_view::parts;
use super::workspace::WorkspacePanel;
use super::{InspectMessage, ShellMessage};

/// 搜索框宽度：Vue 搜索组 392px，其中筛选开关 28px、间距 4px。
const SEARCH_WIDTH: f32 = 360.0;
const CONTROL_EDGE: f32 = 28.0;

/// 左侧侧栏开关，中间全局搜索和筛选开关。窗口控件槽保持打开。
///
/// 标题栏是常驻骨架的一部分：侧栏开关的图标和说明、筛选开关的样子和角标按 `signals` 绑定，
/// 搜索框用 `.model(query)` 受控，组合输入中更新也不会打断预编辑。事件处理器只发消息，不捕获状态。
pub(super) fn title_bar(signals: TitleSignals, query: Signal<String>, initial_query: &str) -> impl IntoView + use<> {
    widget(
        AppTitleBar::new("MomoBako")
            .show_window_controls(true)
            .native_controls(false)
            .center_width(460.0),
    )
    .leading(
        widget(chrome_button(LAYOUT_SIDEBAR_LEFT_COLLAPSE, "折叠侧边栏", false))
            .prop::<bool, SidebarToggleField>(signals.collapsed)
            .key("sidebar-toggle")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::ToggleSidebar);
            }),
    )
    .center(widget(Stack::row(4.0).align(AlignSpec::Center)).children((
        widget(search_field(initial_query)).model(query).key("global-search").on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program_all(ShellMessage::SetWorkspacePanel(WorkspacePanel::Search));
            cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::SetQuery(event.value.to_string())));
        }),
        filter_toggle(signals),
    )))
}

/// 侧栏开关的图标和说明：收起时是「展开侧边栏」，展开时是「折叠侧边栏」。
fn sidebar_toggle_look(collapsed: bool) -> (Icon, &'static str) {
    if collapsed {
        (LAYOUT_SIDEBAR_LEFT_EXPAND, "展开侧边栏")
    } else {
        (LAYOUT_SIDEBAR_LEFT_COLLAPSE, "折叠侧边栏")
    }
}

/// 侧栏开关的图标、无障碍名和提示，跟着侧栏是否收起。
pub(crate) struct SidebarToggleField;

impl FieldWrite<IconButton, bool> for SidebarToggleField {
    const FIELD: &'static str = "IconButton.icon+label+tooltip";

    fn write(target: &mut IconButton, collapsed: bool) {
        let (icon, label) = sidebar_toggle_look(collapsed);
        target.icon = icon;
        target.label = label.into();
        if let Some(tooltip) = &mut target.tooltip {
            tooltip.label = label.into();
        }
    }

    fn differs(target: &IconButton, collapsed: &bool) -> bool {
        let (icon, label) = sidebar_toggle_look(*collapsed);
        target.icon != icon || &*target.label != label || target.tooltip.as_ref().is_some_and(|tooltip| &*tooltip.label != label)
    }
}

/// 标题栏图标按钮：28×28、`--radius-sm`、`--text-muted`，悬停 `--bg-hover`。对应 `.titlebar__btn`。
fn chrome_button(icon: Icon, label: &'static str, disabled: bool) -> IconButton {
    let mut button = IconButton::new(icon, label).size(ControlSize::Large).disabled(disabled).with_tooltip(label);
    button.style = parts::icon_button_style(CONTROL_EDGE, RadiusTier::Sm, SemanticColorRole::Muted, None, disabled);
    button.colors_from_style()
}

/// 筛选开关的说明：筛选栏打开时是「隐藏筛选栏」。
fn filter_label(open: bool) -> &'static str {
    if open { "隐藏筛选栏" } else { "显示筛选栏" }
}

/// 筛选开关的样式：平时弱色，悬停和按下强调色；打开或有条件时强调色底。
fn filter_style(active: bool) -> NodeStyle {
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
    style
}

/// 筛选开关的无障碍名和样式，跟着筛选栏开合和条件数。
pub(crate) struct FilterLookField;

impl FieldWrite<IconButton, FilterLook> for FilterLookField {
    const FIELD: &'static str = "IconButton.label+style";

    fn write(target: &mut IconButton, look: FilterLook) {
        target.label = filter_label(look.open).into();
        target.style = filter_style(look.active);
    }

    fn differs(target: &IconButton, look: &FilterLook) -> bool {
        &*target.label != filter_label(look.open) || target.style != filter_style(look.active)
    }
}

/// 筛选开关。打开或有条件时强调色底；条件数大于 0 时右上角 14px 角标。对应 `.titlebar__filter-btn`。
/// 角标常驻，没有条件时藏起来。
fn filter_toggle(signals: TitleSignals) -> AnyView {
    let mut button = IconButton::new(ADJUSTMENTS_HORIZONTAL, filter_label(false)).size(ControlSize::Large).with_tooltip("筛选");
    button.style = filter_style(false);
    let toggle = widget(button.colors_from_style())
        .prop::<FilterLook, FilterLookField>(signals.filter)
        .key("filter-toggle")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::SetWorkspacePanel(WorkspacePanel::Search));
            cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::ToggleFilterBar));
        });
    let count = signals.count;
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
    .visible(move || count.get() > 0)
    .children((widget(parts::label_text("0", 10.0, 700, Some(SemanticColorRole::Background)).line_height(14.0))
        .prop::<String, fields::text::value>(move || count.get().to_string()),))
    .key("filter-count");
    widget(Stack::row(0.0).with_layout(|layout| layout.position = nana_ui_core::PositionSpec::Relative))
        .children((toggle, badge))
        .into_any()
}

/// 搜索框：`--bg-subtle` 底、无描边、左右 10px、文字和占位居中；悬停或聚焦换 `--bg-hover`。
/// 之后的文字由 `.model` 绑定的信号给；用挂载时的查询建，光标和新建的输入框一样在末尾。
fn search_field(initial_query: &str) -> TextInput {
    let field = TextInput::new(initial_query)
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
