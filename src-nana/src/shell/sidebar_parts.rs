//! 侧栏零件：导航行、分组标题、标题工具、空状态说明和底部入口。
//!
//! 尺寸、字号和状态色照 Vue `shell.css` 里 `workspace-*` 类声明的设计值：导航行 28px、
//! 分组标题 24px、标题工具 22px、底部入口 26px。Lilia 全局 `button { height: 32px }` 会把
//! 只写了 `min-height` 的导航行顶成 32px，这里按组件自己声明的 28px 画。
//! 颜色只写语义角色，行里的图标和文字继承行的前景色，悬停和当前态由行的交互样式整体切换，
//! 和 Vue 的 `color` 继承一致。

use std::sync::Arc;

use nana_ui::runtime::view::{widget, AnyView, IntoView};
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
    let mut node = label_text(value, size, weight, None).truncating();
    let layout = Arc::make_mut(&mut node.style.layout);
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    widget(node).into_any()
}

/// 快捷方式这类导航行：图标、名称，右侧计数。对应 `.workspace-shortcuts__item`。
pub(crate) struct NavRow {
    pub label: String,
    pub icon: Option<Icon>,
    pub count: Option<String>,
    pub active: bool,
    pub disabled: bool,
}

impl NavRow {
    /// 挂上点击。行本身是可聚焦的列表项，无障碍名是行文字。
    pub(crate) fn view(self, key: String, on_activate: impl Fn(&mut ViewContext<ListItem>) + Send + 'static) -> AnyView {
        let mut parts: Vec<AnyView> = Vec::new();
        if let Some(icon) = self.icon {
            parts.push(widget(inherit_icon(icon, 15.0)).into_any());
        }
        parts.push(fill_label(self.label.clone(), 13.0, 500));
        if let Some(count) = self.count {
            parts.push(widget(label_text(count, 12.0, 600, Some(SemanticColorRole::Muted))).into_any());
        }
        let content = widget(Stack::fill_row(8.0).align(AlignSpec::Center)).children(parts);
        let disabled = self.disabled;
        widget(
            ListItem::new(self.label)
                .selected(self.active)
                .disabled(disabled)
                .style(row_style(NAV_ROW_HEIGHT, 8.0, 8.0, 8.0, ActiveTone::Accent, disabled)),
        )
        .content(content)
        .key(key)
        .on_cx(move |_, _: &Activate, cx| {
            if !disabled {
                on_activate(cx);
            }
        })
        .into_any()
    }
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
pub(crate) fn tree_action(
    icon: Icon,
    label: &'static str,
    key: &'static str,
    disabled: bool,
    on_activate: impl Fn(&mut ViewContext<IconButton>) + Send + 'static,
) -> AnyView {
    let mut button = IconButton::new(icon, label).size(ControlSize::Medium).disabled(disabled).with_tooltip(label);
    button.style = icon_button_style(TREE_ACTION_EDGE, RadiusTier::Xs, SemanticColorRole::Faint, None, disabled);
    widget(button.colors_from_style())
        .key(key)
        .on_cx(move |_, _: &Activate, cx| on_activate(cx))
        .into_any()
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
    let hover = SemanticPaint {
        background: Some(SemanticColorRole::Hover),
        foreground: Some(SemanticColorRole::Text),
        ..SemanticPaint::default()
    };
    style.interaction = InteractionStyle {
        hovered: hover,
        pressed: hover,
        focused: SemanticPaint {
            border: Some(SemanticColorRole::Accent),
            ..hover
        },
        ..InteractionStyle::default()
    };
    style.text_horizontal_alignment = nana_ui::runtime::TextHorizontalAlignment::Center;
    style.text_vertical_alignment = nana_ui::runtime::TextVerticalAlignment::Center;
    style
}

/// 分组里的空状态说明。对应 `.workspace-empty--compact` + `.workspace-empty__text`：
/// 内边距 6px 8px，12px 弱色，行高 1.5。
pub(crate) fn empty_hint(copy: &str, key: impl Into<String>) -> AnyView {
    let mut node = label_text(copy, 12.0, 400, Some(SemanticColorRole::Faint)).line_height(18.0);
    {
        let layout = Arc::make_mut(&mut node.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    widget(Stack::column(0.0).padding_xy(8.0, 6.0))
        .children((widget(node).key(key.into()),))
        .into_any()
}

/// 底部入口按钮。对应 `.workspace-footer__btn`：26×26、14px 图标、`--text-muted`；
/// 当前入口 `--accent-soft` 底、强调色，其余按 `rest` 透明度（悬停底栏时为 1）。
pub(crate) struct FooterButton {
    pub icon: Icon,
    pub label: &'static str,
    pub active: bool,
    /// 任务入口在有任务时强调色、满不透明（Vue `.task-button.has-tasks`）。
    pub highlight: bool,
    pub rest_opacity: f32,
}

impl FooterButton {
    pub(crate) fn view(self, key: &'static str, on_activate: impl Fn(&mut ViewContext<IconButton>) + Send + 'static) -> AnyView {
        let foreground = if self.active || self.highlight { SemanticColorRole::Accent } else { SemanticColorRole::Muted };
        let background = self.active.then_some(SemanticColorRole::AccentSoft);
        let mut style = icon_button_style(FOOTER_BUTTON_EDGE, RadiusTier::Sm, foreground, background, false);
        if self.active {
            // 当前入口悬停也保持强调色底，和 `.is-active` 压过 `:hover` 一致。
            let current = SemanticPaint {
                background: Some(SemanticColorRole::AccentSoft),
                foreground: Some(SemanticColorRole::Accent),
                ..SemanticPaint::default()
            };
            style.interaction.hovered = current;
            style.interaction.pressed = current;
        }
        let opacity = if self.active || self.highlight { 1.0 } else { self.rest_opacity };
        Arc::make_mut(&mut style.layout).opacity = Some(opacity);
        let mut button = IconButton::new(self.icon, self.label).size(ControlSize::Large).with_tooltip(self.label);
        button.style = style;
        widget(button.colors_from_style())
            .key(key)
            .on_cx(move |_, _: &Activate, cx| on_activate(cx))
            .into_any()
    }
}
