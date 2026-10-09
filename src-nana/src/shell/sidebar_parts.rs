//! 侧栏零件：导航行、分组标题、标题工具、空状态说明和底部入口，以及常驻侧栏按信号改字段用的写入器。
//!
//! 尺寸、字号和状态色照 Vue `shell.css` 里 `workspace-*` 类声明的设计值：导航行 28px、
//! 分组标题 24px、标题工具 22px、底部入口 26px。Lilia 全局 `button { height: 32px }` 会把
//! 只写了 `min-height` 的导航行顶成 32px，这里按组件自己声明的 28px 画。
//! 颜色只写语义角色，行里的图标和文字继承行的前景色，悬停和当前态由行的交互样式整体切换，
//! 和 Vue 的 `color` 继承一致。
//!
//! 侧栏常驻以后，导航行、标题工具和底部入口的当前态、禁用和计数都按信号绑定：建一次，之后只改
//! 那几个字段。绑定的取值函数是可复制的闭包，同一行的几个字段各绑一份。

use std::sync::Arc;

use nana_ui::runtime::view::{fields, widget, AnyView, El, FieldWrite, IntoProp, IntoView, Signal};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, IconButton, IconGlyph, InteractionStyle, LengthSpec, ListItem, NodeStyle, RadiusTier,
    SemanticColorRole, SemanticPaint, Stack, Text, ViewContext,
};
use nana_ui::{ControlSize, Icon};

/// Vue `button:disabled` 的整体透明度。
pub(crate) const DISABLED_OPACITY: f32 = 0.45;
/// 导航行高度：`.workspace-shortcuts__item` 的 `min-height: 28px`。
const NAV_ROW_HEIGHT: f32 = 28.0;
/// 分组标题高度：`.workspace-group__header { height: 24px }`。
const GROUP_HEADER_HEIGHT: f32 = 24.0;
/// 标题工具：`.workspace-tree-action` 22×22。
const TREE_ACTION_EDGE: f32 = 22.0;
/// 底部入口：`.workspace-footer__btn` 26×26。
const FOOTER_BUTTON_EDGE: f32 = 26.0;

/// 侧栏行的状态色。`accent` 为 true 时当前态是强调色（快捷方式、底部入口），
/// 否则是 `--bg-active` 灰底（文件夹树）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActiveTone {
    Accent,
    Neutral,
}

/// 侧栏行的节点样式。`height` 和左右内边距按 Vue 的类给，悬停和当前态换底色和前景。
pub(crate) fn row_style(height: f32, pad_left: f32, pad_right: f32, gap: f32, tone: ActiveTone, disabled: bool) -> NodeStyle {
    let mut style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(nana_ui_core::FlexDirection::Row);
        layout.align_items = AlignSpec::Center;
        layout.gap = Some(LengthSpec::Px(gap));
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.height = Some(LengthSpec::Px(height));
        layout.min_height = Some(LengthSpec::Px(height));
        layout.padding_left = Some(LengthSpec::Px(pad_left));
        layout.padding_right = Some(LengthSpec::Px(pad_right));
        layout.font_size = Some(13.0);
        layout.font_weight = Some(500);
        layout.white_space_nowrap = true;
        layout.text_overflow_ellipsis = true;
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        if disabled {
            layout.opacity = Some(DISABLED_OPACITY);
        }
    }
    style.radius = Some(RadiusTier::Sm);
    style.foreground = Some(SemanticColorRole::Text);
    let hover = SemanticPaint {
        background: Some(SemanticColorRole::Hover),
        foreground: Some(SemanticColorRole::Text),
        ..SemanticPaint::default()
    };
    let current = match tone {
        ActiveTone::Accent => SemanticPaint {
            background: Some(SemanticColorRole::AccentSoft),
            foreground: Some(SemanticColorRole::Accent),
            ..SemanticPaint::default()
        },
        ActiveTone::Neutral => SemanticPaint {
            background: Some(SemanticColorRole::Active),
            foreground: Some(SemanticColorRole::Text),
            ..SemanticPaint::default()
        },
    };
    style.interaction = InteractionStyle {
        hovered: hover,
        pressed: hover,
        // Vue 的 `.is-active` 写在 `:hover` 之后、同优先级，悬停不改当前行的颜色。
        selected: current,
        selected_hovered: current,
        selected_pressed: current,
        focused: SemanticPaint {
            border: Some(SemanticColorRole::Accent),
            ..hover
        },
        ..InteractionStyle::default()
    };
    style.text_vertical_alignment = nana_ui::runtime::TextVerticalAlignment::Center;
    style
}

/// 主操作按钮：`--accent-soft` 底、`--accent-on-soft` 字、600 字重（DESIGN.md 5.1 的 `.primary`）。
pub(crate) fn primary_button(label: impl Into<String>) -> Button {
    let mut button = Button::new(label).kind(nana_ui::ButtonKind::Primary);
    Arc::make_mut(&mut button.style.layout).font_weight = Some(600);
    button
}

/// 不带固定颜色的图标：跟着行的前景色走（Vue 的 `currentColor`）。
pub(crate) fn inherit_icon(icon: Icon, size: f32) -> IconGlyph {
    IconGlyph::new(icon).size(size).style(NodeStyle::default())
}

/// 一段文字。`color` 为 `None` 时继承父节点前景色。
pub(crate) fn label_text(value: impl Into<String>, size: f32, weight: u16, color: Option<SemanticColorRole>) -> Text {
    let mut node = Text::new(value).font_size(size).font_weight(weight);
    if let Some(color) = color {
        node = node.color(color);
    }
    node
}

/// 占满剩余宽度、超出时省略的单行文字。
pub(crate) fn fill_label(value: impl Into<String>, size: f32, weight: u16) -> AnyView {
    widget(fill_text(value, size, weight)).into_any()
}

/// [`fill_label`] 的文字节点本身，常驻视图拿它再绑定文字。
pub(crate) fn fill_text(value: impl Into<String>, size: f32, weight: u16) -> Text {
    let mut node = label_text(value, size, weight, None).truncating();
    let layout = Arc::make_mut(&mut node.style.layout);
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    node
}

/// 导航行随状态变的部分：当前态、禁用和右侧计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NavLook {
    pub active: bool,
    pub disabled: bool,
    /// 右侧计数；`None` 的行没有计数这一格。
    pub count: Option<usize>,
}

/// 快捷方式这类导航行：图标、名称，右侧计数。对应 `.workspace-shortcuts__item`。
///
/// 行本身是可聚焦的列表项，无障碍名是行文字。`look` 现读信号，当前态、禁用（连同 0.45 透明度）
/// 和计数各自绑定；有没有计数这一格按建行时的取值定。禁用的列表项收不到激活，处理器不再判断。
pub(crate) fn nav_row(
    label: &str,
    icon: Option<Icon>,
    look: impl Fn() -> NavLook + Copy + Send + 'static,
    key: String,
    on_activate: impl Fn(&mut ViewContext<ListItem>) + Send + 'static,
) -> AnyView {
    let initial = look();
    let mut parts: Vec<AnyView> = Vec::new();
    if let Some(icon) = icon {
        parts.push(widget(inherit_icon(icon, 15.0)).into_any());
    }
    parts.push(fill_label(label, 13.0, 500));
    if let Some(count) = initial.count {
        parts.push(
            widget(label_text(count.to_string(), 12.0, 600, Some(SemanticColorRole::Muted)))
                .prop::<String, fields::text::value>(move || look().count.unwrap_or_default().to_string())
                .into_any(),
        );
    }
    let content = widget(Stack::fill_row(8.0).align(AlignSpec::Center)).children(parts);
    widget(
        ListItem::new(label)
            .selected(initial.active)
            .disabled(initial.disabled)
            .style(row_style(NAV_ROW_HEIGHT, 8.0, 8.0, 8.0, ActiveTone::Accent, initial.disabled)),
    )
    .prop::<bool, fields::list_item::selected>(move || look().active)
    .prop::<bool, DimmedRow>(move || look().disabled)
    .content(content)
    .key(key)
    .on_cx(move |_, _: &Activate, cx| on_activate(cx))
    .into_any()
}

/// 分组标题：11px 粗体弱色字，右侧工具。对应 `.workspace-group__header`。
/// `title` 可以是一段文字，也可以是播放集那样可点的标题按钮。
pub(crate) fn group_header(title: AnyView, tools: Vec<AnyView>, key: &'static str) -> AnyView {
    let tools = (!tools.is_empty()).then(|| widget(Stack::row(2.0).align(AlignSpec::Center)).children(tools).into_any());
    widget(
        Stack::bar(0.0)
            .align(AlignSpec::Center)
            .height(LengthSpec::Px(GROUP_HEADER_HEIGHT))
            .min_height(LengthSpec::Px(GROUP_HEADER_HEIGHT))
            .with_layout(|layout| {
                layout.padding_left = Some(LengthSpec::Px(8.0));
                layout.padding_right = Some(LengthSpec::Px(6.0));
            }),
    )
    .children((title, widget(Stack::spacer()), tools))
    .key(key)
    .into_any()
}

/// 分组标题文字：11px、700、`--text-faint`、字距 0.4px。
pub(crate) fn group_title(value: &str) -> Text {
    let mut node = label_text(value, 11.0, 700, Some(SemanticColorRole::Faint));
    Arc::make_mut(&mut node.style.layout).letter_spacing = Some(0.4);
    node
}

/// 分组标题右侧的小图标按钮。对应 `.workspace-tree-action`：22×22、`--radius-xs`、弱色 13px 图标。
/// 禁用（连同 0.45 透明度）按 `disabled` 绑定，常量也行；返回元素本身，调用方还能再绑图标。
pub(crate) fn tree_action(
    icon: Icon,
    label: &'static str,
    key: &'static str,
    disabled: impl IntoProp<bool>,
    on_activate: impl Fn(&mut ViewContext<IconButton>) + Send + 'static,
) -> El<IconButton> {
    let mut button = IconButton::new(icon, label).size(ControlSize::Medium).with_tooltip(label);
    button.style = icon_button_style(TREE_ACTION_EDGE, RadiusTier::Xs, SemanticColorRole::Faint, None, false);
    widget(button.colors_from_style())
        .prop::<bool, DimmedTool>(disabled)
        .key(key)
        .on_cx(move |_, _: &Activate, cx| on_activate(cx))
}

/// 方形图标按钮的样式：常态透明，悬停换 `--bg-hover` 和正文色，禁用整体 0.45。
pub(crate) fn icon_button_style(
    edge: f32,
    radius: RadiusTier,
    foreground: SemanticColorRole,
    background: Option<SemanticColorRole>,
    disabled: bool,
) -> NodeStyle {
    let mut style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        let edge = LengthSpec::Px(edge);
        layout.width = Some(edge);
        layout.height = Some(edge);
        layout.min_width = Some(edge);
        layout.min_height = Some(edge);
        layout.padding_left = Some(LengthSpec::Px(0.0));
        layout.padding_right = Some(LengthSpec::Px(0.0));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        if disabled {
            layout.opacity = Some(DISABLED_OPACITY);
        }
    }
    style.radius = Some(radius);
    style.foreground = Some(foreground);
    style.background = background;
    style.interaction = InteractionStyle {
        hovered: hover_paint(),
        pressed: hover_paint(),
        focused: SemanticPaint {
            border: Some(SemanticColorRole::Accent),
            ..hover_paint()
        },
        ..InteractionStyle::default()
    };
    style.text_horizontal_alignment = nana_ui::runtime::TextHorizontalAlignment::Center;
    style.text_vertical_alignment = nana_ui::runtime::TextVerticalAlignment::Center;
    style
}

/// 图标按钮悬停和按下时的 `--bg-hover` 底、正文色。
fn hover_paint() -> SemanticPaint {
    SemanticPaint {
        background: Some(SemanticColorRole::Hover),
        foreground: Some(SemanticColorRole::Text),
        ..SemanticPaint::default()
    }
}

/// 分组里的空状态说明。对应 `.workspace-empty--compact` + `.workspace-empty__text`：
/// 内边距 6px 8px，12px 弱色，行高 1.5。
pub(crate) fn empty_hint(copy: &str, key: impl Into<String>) -> AnyView {
    widget(Stack::column(0.0).padding_xy(8.0, 6.0))
        .children((widget(hint_text(copy)).key(key.into()),))
        .into_any()
}

/// 常驻分组的空状态说明：`copy` 现读信号，有说明时显示并改字，没有时藏起来、不占布局。
pub(crate) fn bound_hint(copy: impl Fn() -> Option<&'static str> + Copy + Send + 'static, key: &'static str) -> AnyView {
    widget(Stack::column(0.0).padding_xy(8.0, 6.0))
        .visible(move || copy().is_some())
        .children((widget(hint_text(copy().unwrap_or_default()))
            .prop::<String, fields::text::value>(move || copy().unwrap_or_default().to_string())
            .key(key),))
        .into_any()
}

/// 空状态说明的文字：12px 弱色、行高 18px，可在任意位置折行。
fn hint_text(copy: &str) -> Text {
    let mut node = label_text(copy, 12.0, 400, Some(SemanticColorRole::Faint)).line_height(18.0);
    let layout = Arc::make_mut(&mut node.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    node
}

/// 底部入口的状态：当前入口，以及任务入口有任务时的强调（Vue `.task-button.has-tasks`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FooterLook {
    pub active: bool,
    pub highlight: bool,
}

/// 底部入口按钮。对应 `.workspace-footer__btn`：26×26、14px 图标、`--text-muted`；
/// 当前入口 `--accent-soft` 底、强调色，其余按 `rest` 透明度（悬停底栏时为 1）。
///
/// 颜色按 `look` 绑定；平时的半透明随底栏悬停淡入淡出，绑在热信号 `rest` 上，当前入口和有任务时
/// 常亮，不读它。
pub(crate) fn footer_button(
    icon: Icon,
    label: &'static str,
    key: &'static str,
    look: impl Fn() -> FooterLook + Copy + Send + 'static,
    rest: Signal<f32>,
    on_activate: impl Fn(&mut ViewContext<IconButton>) + Send + 'static,
) -> AnyView {
    let mut button = IconButton::new(icon, label).size(ControlSize::Large).with_tooltip(label);
    button.style = icon_button_style(FOOTER_BUTTON_EDGE, RadiusTier::Sm, SemanticColorRole::Muted, None, false);
    widget(button.colors_from_style())
        .prop::<FooterLook, FooterPaint>(look)
        .prop::<f32, super::super::hot::OpacityField>(move || {
            let look = look();
            if look.active || look.highlight { 1.0 } else { rest.get() }
        })
        .key(key)
        .on_cx(move |_, _: &Activate, cx| on_activate(cx))
        .into_any()
}

/// 底部入口的前景、底色和悬停按下的样子。当前入口悬停也保持强调色底，和 `.is-active` 压过
/// `:hover` 一致。
fn footer_paint(look: FooterLook) -> (SemanticColorRole, Option<SemanticColorRole>, SemanticPaint) {
    let foreground = if look.active || look.highlight { SemanticColorRole::Accent } else { SemanticColorRole::Muted };
    if look.active {
        let current = SemanticPaint {
            background: Some(SemanticColorRole::AccentSoft),
            foreground: Some(SemanticColorRole::Accent),
            ..SemanticPaint::default()
        };
        return (foreground, Some(SemanticColorRole::AccentSoft), current);
    }
    (foreground, None, hover_paint())
}

/// 底部入口按状态换颜色：前景、底色、悬停和按下。透明度另由 `OpacityField` 绑。
pub(crate) struct FooterPaint;

impl FieldWrite<IconButton, FooterLook> for FooterPaint {
    const FIELD: &'static str = "IconButton.style.foreground+background+interaction.hovered+pressed";

    fn write(target: &mut IconButton, look: FooterLook) {
        let (foreground, background, hover) = footer_paint(look);
        target.style.foreground = Some(foreground);
        target.style.background = background;
        target.style.interaction.hovered = hover;
        target.style.interaction.pressed = hover;
    }

    fn differs(target: &IconButton, look: &FooterLook) -> bool {
        let (foreground, background, hover) = footer_paint(*look);
        let style = &target.style;
        style.foreground != Some(foreground)
            || style.background != background
            || style.interaction.hovered != hover
            || style.interaction.pressed != hover
    }
}

/// 列表项的禁用：不可点，整行 0.45 透明度（Vue `button:disabled`）。
pub(crate) struct DimmedRow;

impl FieldWrite<ListItem, bool> for DimmedRow {
    const FIELD: &'static str = "ListItem.disabled+style.layout.opacity";

    fn write(target: &mut ListItem, disabled: bool) {
        target.disabled = disabled;
        Arc::make_mut(&mut target.style.layout).opacity = disabled.then_some(DISABLED_OPACITY);
    }

    fn differs(target: &ListItem, disabled: &bool) -> bool {
        target.disabled != *disabled || target.style.layout.opacity != disabled.then_some(DISABLED_OPACITY)
    }
}

/// 图标按钮的禁用：不可点，整颗 0.45 透明度。
pub(crate) struct DimmedTool;

impl FieldWrite<IconButton, bool> for DimmedTool {
    const FIELD: &'static str = "IconButton.disabled+style.layout.opacity";

    fn write(target: &mut IconButton, disabled: bool) {
        target.disabled = disabled;
        Arc::make_mut(&mut target.style.layout).opacity = disabled.then_some(DISABLED_OPACITY);
    }

    fn differs(target: &IconButton, disabled: &bool) -> bool {
        target.disabled != *disabled || target.style.layout.opacity != disabled.then_some(DISABLED_OPACITY)
    }
}

/// 图标字形本身，文件夹开合这类随状态换图标的地方用。
pub(crate) struct GlyphIcon;

impl FieldWrite<IconGlyph, Icon> for GlyphIcon {
    const FIELD: &'static str = "IconGlyph.icon";

    fn write(target: &mut IconGlyph, icon: Icon) {
        target.icon = icon;
    }

    fn differs(target: &IconGlyph, icon: &Icon) -> bool {
        target.icon != *icon
    }
}
