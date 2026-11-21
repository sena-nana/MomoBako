//! 侧栏零件：导航行、分组标题、标题工具、空状态说明和底部入口，以及常驻侧栏按信号改字段用的写入器。
//!
//! 导航行是 NanaUI 的侧栏行（`SidebarRow`），高度、字号和当前态的底色按主题配方；其余零件按设计值：
//! 分组标题 24px、标题工具 22px、底部入口 26px。
//! 颜色只写语义角色，树行里的图标和文字继承行的前景色，悬停和当前态由行的交互样式整体切换。
//!
//! 侧栏常驻以后，导航行、标题工具和底部入口的当前态、禁用和计数都按信号绑定：建一次，之后只改
//! 那几个字段。绑定的取值函数是可复制的闭包，同一行的几个字段各绑一份。

use std::sync::Arc;

use nana_ui::runtime::view::{
    AnyView, El, FieldWrite, IntoProp, IntoView, Signal, fields, stylesheet, widget,
};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, IconButton, IconGlyph, InteractionStyle, LengthSpec, NodeStyle,
    Popover, PopoverAlignment, PopoverPlacement, PopoverToggled, RadiusTier, SemanticColorRole,
    SemanticPaint, SidebarRow, SidebarRowIcon, SidebarRowState, Stack, Text, ViewContext,
};
use nana_ui::{ControlSize, Icon};

use crate::theme_map::{DISABLED_OPACITY, disabled_paint};

/// 分组标题高度。
const GROUP_HEADER_HEIGHT: f32 = 24.0;
/// 标题工具：22×22。
const TREE_ACTION_EDGE: f32 = 22.0;
/// 底部入口：26×26。
const FOOTER_BUTTON_EDGE: f32 = 26.0;

stylesheet! {
    pub(crate) mod footer_styles;
    /* 底部入口平时 0.44 透明度，亮起（悬停底栏、当前入口、有任务）时为 1，按 0.35s ease 淡入淡出。 */
    .footer-entry { opacity: 0.44; transition: opacity 350ms ease; }
    .footer-entry.lit { opacity: 1; }
}

/// 侧栏行的状态色。`accent` 为 true 时当前态是强调色（快捷方式、底部入口），
/// 否则是 `--bg-active` 灰底（文件夹树）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActiveTone {
    Accent,
    Neutral,
}

/// 侧栏行的节点样式。`height` 和左右内边距按行的种类给，悬停和当前态换底色和前景。
pub(crate) fn row_style(
    height: f32,
    pad_left: f32,
    pad_right: f32,
    gap: f32,
    tone: ActiveTone,
    disabled: bool,
) -> NodeStyle {
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
        // 当前态优先，悬停不改当前行的颜色。
        selected: current,
        selected_hovered: current,
        selected_pressed: current,
        focused: SemanticPaint {
            border: Some(SemanticColorRole::Accent),
            ..hover
        },
        ..InteractionStyle::default()
    }
    .into();
    style.text_vertical_alignment = nana_ui::runtime::TextVerticalAlignment::Center;
    style
}

/// 主操作按钮：`--accent-soft` 底、`--accent-on-soft` 字、600 字重（DESIGN.md 5.1 的 `.primary`）。
pub(crate) fn primary_button(label: impl Into<String>) -> Button {
    let mut button = Button::new(label).kind(nana_ui::ButtonKind::Primary);
    Arc::make_mut(&mut button.style.layout).font_weight = Some(600);
    button
}

/// 不带固定颜色的图标：跟着行的前景色走。
pub(crate) fn inherit_icon(icon: Icon, size: f32) -> IconGlyph {
    IconGlyph::new(icon).size(size).style(NodeStyle::default())
}

/// 一段文字。`color` 为 `None` 时继承父节点前景色。
pub(crate) fn label_text(
    value: impl Into<String>,
    size: f32,
    weight: u16,
    color: Option<SemanticColorRole>,
) -> Text {
    let mut node = crate::theme_map::typed_text(Text::new(value), size, weight);
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

impl NavLook {
    /// 导航行的状态：禁用压过当前态。
    fn state(self) -> SidebarRowState {
        if self.disabled {
            SidebarRowState::Disabled
        } else if self.active {
            SidebarRowState::Active
        } else {
            SidebarRowState::Idle
        }
    }
}

/// 快捷方式这类导航行：NanaUI 的侧栏行（`SidebarRow`），前面是行图标，后面是计数。
///
/// 行本身可聚焦，无障碍名是行文字。`look` 现读信号，当前态和禁用合成一个行状态绑定，计数另绑；
/// 有没有计数这一格按建行时的取值定。禁用的行收不到激活，处理器不再判断。
pub(crate) fn nav_row(
    label: &str,
    icon: Option<Icon>,
    look: impl Fn() -> NavLook + Copy + Send + 'static,
    key: String,
    on_activate: impl Fn(&mut ViewContext<SidebarRow>) + Send + 'static,
) -> AnyView {
    let initial = look();
    let mut row = widget(SidebarRow::new(label).state(initial.state()))
        .prop::<SidebarRowState, RowState>(move || look().state());
    if let Some(icon) = icon {
        row = row.leading(widget(SidebarRowIcon::new(icon)));
    }
    if let Some(count) = initial.count {
        row = row.trailing(
            widget(label_text(
                count.to_string(),
                12.0,
                600,
                Some(SemanticColorRole::Muted),
            ))
            .prop::<String, fields::text::value>(move || {
                look().count.unwrap_or_default().to_string()
            }),
        );
    }
    row.key(key)
        .on_cx(move |_, _: &Activate, cx| on_activate(cx))
        .into_any()
}

/// 侧栏行的状态：当前、禁用或空闲。
pub(crate) struct RowState;

impl FieldWrite<SidebarRow, SidebarRowState> for RowState {
    const FIELD: &'static str = "SidebarRow.state";

    fn write(target: &mut SidebarRow, state: SidebarRowState) {
        target.state = state;
    }

    fn differs(target: &SidebarRow, state: &SidebarRowState) -> bool {
        target.state != *state
    }
}

/// 分组标题：11px 粗体弱色字，右侧工具。
/// `title` 可以是一段文字，也可以是播放集那样可点的标题按钮。
pub(crate) fn group_header(title: AnyView, tools: Vec<AnyView>, key: &'static str) -> AnyView {
    let tools = (!tools.is_empty()).then(|| {
        widget(Stack::row(2.0).align(AlignSpec::Center))
            .children(tools)
            .into_any()
    });
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
    Arc::make_mut(&mut node.style.layout)
        .text_ext
        .letter_spacing = Some(0.4);
    node
}

/// 分组标题右侧的小图标按钮：22×22、`--radius-xs`、弱色 13px 图标。
/// 禁用按 `disabled` 绑定，常量也行。返回元素本身，调用方还能再绑图标。
///
/// 停用时按 0.45 把弱色和侧栏底色（`--bg-elev`）在 sRGB 里混成不透明色，
/// 颜色照写的来（`colors_from_style`），NanaUI 不再整枚淡化。播放集行上的「播放」不走这一个底色，
/// 选中时改和 `--accent-soft` 混，见 [`PlayBase`]。
pub(crate) fn tree_action(
    icon: Icon,
    label: &'static str,
    key: &'static str,
    disabled: impl IntoProp<bool>,
    on_activate: impl Fn(&mut ViewContext<IconButton>) + Send + 'static,
) -> El<IconButton> {
    tree_action_titled(icon, label, label, key, disabled, on_activate)
}

/// 标题工具，悬停提示和读屏名可以不是同一句。
///
/// `label` 是无障碍名，`tip` 是悬停时浮出的提示。两边一样时用 [`tree_action`]。
/// 播放集行的「播放」读屏名是「播放播放集」，提示是「播放」。
pub(crate) fn tree_action_titled(
    icon: Icon,
    label: &'static str,
    tip: &'static str,
    key: &'static str,
    disabled: impl IntoProp<bool>,
    on_activate: impl Fn(&mut ViewContext<IconButton>) + Send + 'static,
) -> El<IconButton> {
    let mut button = IconButton::new(icon, label)
        .size(ControlSize::Medium)
        .with_tooltip(tip);
    button.style = icon_button_style(
        TREE_ACTION_EDGE,
        RadiusTier::Xs,
        SemanticColorRole::Faint,
        None,
    );
    button.style.interaction.disabled =
        disabled_paint(SemanticColorRole::Faint, SemanticColorRole::Surface);
    widget(button.colors_from_style())
        .prop::<bool, fields::icon_button::disabled>(disabled)
        .key(key)
        .on_cx(move |_, _: &Activate, cx| on_activate(cx))
}

/// 播放集行「播放」的停用底色：这一行选中时是 `--accent-soft`，否则仍是侧栏底。
///
/// 这个按钮底是透明的，字色 `--text-faint`，停用时整颗 0.45 不透明。
/// 选中行的底是 `--accent-soft`（深色主题约 0.14、浅色约 0.10 的强调色），半透明字形会透出那一点强调色；
/// 没选中时行没有这层底，透出来的是侧栏 `--bg-elev`。这里不改 NanaUI 的混合，只按行换 `disabled_paint`
/// 的底色。`--accent-soft` 自己带透明度，和它混出的字色也带着透明度，画上去会再叠一行底。
pub(crate) struct PlayBase;

impl FieldWrite<IconButton, bool> for PlayBase {
    const FIELD: &'static str = "IconButton.style.interaction.disabled";

    fn write(target: &mut IconButton, selected: bool) {
        target.style.interaction.disabled = play_disabled_paint(selected);
    }

    fn differs(target: &IconButton, selected: &bool) -> bool {
        target.style.interaction.disabled != play_disabled_paint(*selected)
    }
}

/// 选中行和 `--accent-soft` 混，其余行和侧栏底混。
fn play_disabled_paint(selected: bool) -> SemanticPaint {
    let base = if selected {
        SemanticColorRole::AccentSoft
    } else {
        SemanticColorRole::Surface
    };
    disabled_paint(SemanticColorRole::Faint, base)
}

/// 方形图标按钮的样式：常态透明，悬停换 `--bg-hover` 和正文色。会停用的按钮另写停用样子
/// （`theme_map::disabled_paint`），要按它实际所在的底色混。
pub(crate) fn icon_button_style(
    edge: f32,
    radius: RadiusTier,
    foreground: SemanticColorRole,
    background: Option<SemanticColorRole>,
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
    }
    .into();
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

/// 分组里的空状态说明：
/// 内边距 6px 8px，12px 弱色，行高 1.5。
pub(crate) fn empty_hint(copy: &str, key: impl Into<String>) -> AnyView {
    widget(Stack::column(0.0).padding_xy(8.0, 6.0))
        .children((widget(hint_text(copy)).key(key.into()),))
        .into_any()
}

/// 常驻分组的空状态说明：`copy` 现读信号，有说明时显示并改字，没有时藏起来、不占布局。
pub(crate) fn bound_hint(
    copy: impl Fn() -> Option<&'static str> + Copy + Send + 'static,
    key: &'static str,
) -> AnyView {
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
    layout.text_ext.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    node
}

/// 底部入口的状态：当前入口，以及任务入口有任务时的强调。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FooterLook {
    pub active: bool,
    pub highlight: bool,
}

/// 底部入口按钮：26×26、14px 图标、`--text-muted`；
/// 当前入口 `--accent-soft` 底、强调色，其余平时 0.44 透明度（悬停底栏时为 1）。
///
/// 颜色按 `look` 绑定；透明度是样式表的 `footer-entry` 和 `lit`（[`footer_lit`]），亮暗之间由 NanaUI 按
/// `transition: opacity 0.35s ease` 淡入淡出。
pub(crate) fn footer_button(
    icon: Icon,
    label: &'static str,
    key: &'static str,
    look: impl Fn() -> FooterLook + Copy + Send + 'static,
    hover: Signal<bool>,
    on_activate: impl Fn(&mut ViewContext<IconButton>) + Send + 'static,
) -> AnyView {
    let mut button = IconButton::new(icon, label)
        .size(ControlSize::Large)
        .with_tooltip(label);
    button.style = icon_button_style(
        FOOTER_BUTTON_EDGE,
        RadiusTier::Sm,
        SemanticColorRole::Muted,
        None,
    );
    widget(button.colors_from_style())
        .prop::<FooterLook, FooterPaint>(look)
        .class(footer_styles::footer_entry)
        .class_when(footer_styles::lit, footer_lit(look, hover))
        .key(key)
        .on_cx(move |_, _: &Activate, cx| on_activate(cx))
        .into_any()
}

/// 底部入口亮着：指针停在底栏上，或者是当前入口、有任务的「任务」入口（这两种常亮，不跟悬停）。
pub(crate) fn footer_lit(
    look: impl Fn() -> FooterLook + Copy + Send + 'static,
    hover: Signal<bool>,
) -> impl Fn() -> bool + Copy + Send + 'static {
    move || {
        let look = look();
        look.active || look.highlight || hover.get()
    }
}

/// 「任务」入口：外观和别的底部入口一样，是 NanaUI `Popover` 的触发器，面板是任务列表
/// （`admin::task_panel`）。开合、点外面、Escape 和焦点归还交给框架；框架开合时发的 `PopoverToggled`
/// 由 `on_toggle` 写回 ViewModel，ViewModel 关上（点了面板里的关闭）时经 `open` 写回弹层。
pub(crate) fn footer_popover(
    icon: Icon,
    label: &'static str,
    key: &'static str,
    look: impl Fn() -> FooterLook + Copy + Send + 'static,
    open: impl Fn() -> bool + Send + 'static,
    panel: AnyView,
    on_toggle: impl Fn(bool, &mut ViewContext<Popover>) + Send + 'static,
) -> AnyView {
    let mut popover = Popover::new()
        .trigger_icon(icon, label)
        .compact_trigger(true)
        .placement(PopoverPlacement::Top)
        .alignment(PopoverAlignment::Start)
        .width(340.0)
        .trigger_icon_size(14.0);
    popover.trigger_style = Some(footer_trigger_style(look()));
    widget(popover)
        .prop::<FooterLook, FooterTrigger>(look)
        .prop::<bool, PopoverOpen>(open)
        .children((panel,))
        .key(key)
        .on_cx(move |_, toggled: &PopoverToggled, cx| on_toggle(toggled.open, cx))
        .into_any()
}

/// 底部入口当触发器时的样子：26 见方、`Sm` 圆角，颜色和别的入口一样按状态换。
fn footer_trigger_style(look: FooterLook) -> NodeStyle {
    let mut style = icon_button_style(
        FOOTER_BUTTON_EDGE,
        RadiusTier::Sm,
        SemanticColorRole::Muted,
        None,
    );
    let (foreground, background, hover) = footer_paint(look);
    style.foreground = Some(foreground);
    style.background = background;
    style.interaction.hovered = hover;
    style.interaction.pressed = hover;
    style
}

/// 「任务」触发器按状态换颜色。
pub(crate) struct FooterTrigger;

impl FieldWrite<Popover, FooterLook> for FooterTrigger {
    const FIELD: &'static str = "Popover.trigger_style";

    fn write(target: &mut Popover, look: FooterLook) {
        target.trigger_style = Some(footer_trigger_style(look));
    }

    fn differs(target: &Popover, look: &FooterLook) -> bool {
        target.trigger_style.as_ref() != Some(&footer_trigger_style(*look))
    }
}

/// 弹层开没开着。
pub(crate) struct PopoverOpen;

impl FieldWrite<Popover, bool> for PopoverOpen {
    const FIELD: &'static str = "Popover.open";

    fn write(target: &mut Popover, open: bool) {
        target.open = open;
    }

    fn differs(target: &Popover, open: &bool) -> bool {
        target.open != *open
    }
}

/// 底部入口的前景、底色和悬停按下的样子。当前入口悬停也保持强调色底：
/// 当前态压过悬停。
fn footer_paint(look: FooterLook) -> (SemanticColorRole, Option<SemanticColorRole>, SemanticPaint) {
    let foreground = if look.active || look.highlight {
        SemanticColorRole::Accent
    } else {
        SemanticColorRole::Muted
    };
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

/// 底部入口按状态换颜色：前景、底色、悬停和按下。透明度另由样式表的 `footer-entry` 给。
pub(crate) struct FooterPaint;

impl FieldWrite<IconButton, FooterLook> for FooterPaint {
    const FIELD: &'static str =
        "IconButton.style.foreground+background+interaction.hovered+pressed";

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
