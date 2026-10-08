//! 播放条的零件：停留时长与画面适配设置行、当前队列浮层，以及几处共用的小部件。
//!
//! 浮层贴在卡片右上方，和 Vue `.workspace-player__queue` 一样右侧内缩 14px、
//! 底边离卡片 10px。绝对定位相对父级内容盒，所以偏移要把卡片内边距算进去。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, IconButton, IconGlyph, JustifySpec, LengthSpec, RangeChanged, RangeField, RadiusTier,
    ScrollAxes, ScrollView, SemanticColorRole, Stack, Text, TextHorizontalAlignment,
};
use nana_ui::Icon;

use super::super::super::player::{PlayerMessage, QueueItem};
use super::super::super::ShellMessage;
use super::super::paint::{HoverSurface, SliderTrack};
use super::{display_title, BarProps, DISABLED_OPACITY};

/// 卡片内边距：上 12、左右 16。浮层的绝对定位从内容盒算起。
const CARD_PADDING_TOP: f32 = 12.0;
const CARD_PADDING_X: f32 = 16.0;
/// 浮层上限高度，和 Vue `max-height: 320px` 一致。
const QUEUE_MAX_HEIGHT: f32 = 320.0;
/// 浮层内边距和行距。
const QUEUE_PADDING: f32 = 10.0;
const QUEUE_HEAD: f32 = 20.15;

/// 单行文字：给定字号、字重、颜色和行高，超长省略。
pub(super) fn line(label: impl Into<String>, size: f32, weight: u16, role: SemanticColorRole, line_height: f32) -> Text {
    let mut text = Text::new(label.into()).color(role).font_size(size).font_weight(weight).line_height(line_height).truncating();
    let layout = std::sync::Arc::make_mut(&mut text.style.layout);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Fill);
    text
}

/// 固定像素的图标字形。
pub(super) fn icon(icon: Icon, size: f32, role: SemanticColorRole) -> IconGlyph {
    IconGlyph::new(icon).size(size).role(role)
}

/// 没有缩略图时的类型方块：bg 底、10px 圆角、11px 粗体弱色字居中。
pub(super) fn type_cover(label: &str, size: f32, key: &'static str) -> AnyView {
    let mut text = Text::new(label).color(SemanticColorRole::Muted).font_size(11.0).font_weight(700).line_height(17.05);
    text.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    widget(
        Stack::column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Px(size))
            .height(LengthSpec::Px(size))
            .min_width(LengthSpec::Px(size))
            .min_height(LengthSpec::Px(size))
            .grow(0.0)
            .shrink(0.0)
            .surface(SemanticColorRole::Background)
            .radius(RadiusTier::Lg),
    )
    .children((widget(text),))
    .key(key)
    .into_any()
}

/// 盖满父级的透明可点区域。名字只给无障碍；悬停、按下时铺底色，内容由兄弟节点画在上面。
/// 要放在兄弟节点前面，绘制顺序才在下层。绝对定位从父级内容盒算起，`inset` 是父级内边距，
/// 往外扩回去才盖住整块。
pub(crate) fn hit_area(label: &str, disabled: bool, inset: f32, radius: f32) -> IconButton {
    let mut button = IconButton::new(super::icons::PLAY, label).colors_from_style().disabled(disabled);
    button.style.background = None;
    button.style.square = None;
    button.style.control_padding_x = None;
    button.style.painter = Some(HoverSurface { radius }.into());
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.position = nana_ui_core::PositionSpec::Absolute;
    layout.offset_left = Some(LengthSpec::Px(-inset));
    layout.offset_top = Some(LengthSpec::Px(-inset));
    layout.width = Some(LengthSpec::CalcPercentOffset { percent: 100.0, offset_px: inset * 2.0 });
    layout.height = Some(LengthSpec::CalcPercentOffset { percent: 100.0, offset_px: inset * 2.0 });
    button
}

/// 细滑杆：宽 `width`，压缩时最窄 `min_width`，高 20px。禁用时整条 0.45 不透明。
#[allow(clippy::too_many_arguments)]
pub(super) fn slider(value: f64, minimum: f64, maximum: f64, step: f64, label: &str, disabled: bool, width: f32, min_width: f32) -> RangeField {
    let mut range = RangeField::new(value, minimum, maximum, step).label(label).show_label(false).show_value(false).disabled(disabled);
    let ratio = range.ratio();
    range.style.painter = Some(SliderTrack { ratio }.into());
    range.style.background = None;
    range.style.border = None;
    let layout = std::sync::Arc::make_mut(&mut range.style.layout);
    layout.width = Some(LengthSpec::Px(width));
    layout.min_width = Some(LengthSpec::Px(min_width));
    layout.height = Some(LengthSpec::Px(20.0));
    layout.min_height = Some(LengthSpec::Px(20.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(1.0);
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    layout.opacity = disabled.then_some(DISABLED_OPACITY);
    range
}

/// 图片、视频的设置行：图片多一条停留时长，两类都有适应 / 填充。宽窗靠右，窄窗两端对齐并可折行。
pub(super) fn settings_row(props: &BarProps, narrow: bool) -> AnyView {
    let mut children: Vec<AnyView> = Vec::new();
    if props.file_class == "image" {
        let seconds = props.image_seconds.clamp(2, 30);
        let duration = slider(f64::from(seconds), 2.0, 30.0, 1.0, "图片停留时长", false, 120.0, 120.0);
        children.push(
            widget(Stack::row(8.0).align(AlignSpec::Center))
                .children((
                    widget(Text::new(format!("停留 {seconds} 秒")).color(SemanticColorRole::Muted).font_size(12.0).line_height(18.6))
                        .key("player-duration-label"),
                    widget(duration).key("player-duration").on_cx(|_, event: &RangeChanged, cx| {
                        let seconds = event.value.round().clamp(2.0, 30.0) as u32;
                        cx.dispatch_program(player_message(PlayerMessage::SetImageDuration(seconds * 1000)));
                    }),
                ))
                .into_any(),
        );
    }
    children.push(fit_switch(props.cover));
    let justify = if narrow { JustifySpec::SpaceBetween } else { JustifySpec::End };
    widget(Stack::bar(12.0).justify(justify).align(AlignSpec::Center).min_height(LengthSpec::Px(34.0)).wrap(narrow))
        .children(children)
        .key("player-settings")
        .into_any()
}

/// 适应 / 填充两档。外框是 bg 底、border-soft 细边、8px 圆角；当前档铺 accent-soft、字用 accent。
fn fit_switch(cover: bool) -> AnyView {
    widget(
        Stack::row(2.0)
            .align(AlignSpec::Center)
            .padding(2.0)
            .surface(SemanticColorRole::Background)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((
        widget(fit_option("适应", !cover)).key("player-fit-contain").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(player_message(PlayerMessage::SetObjectFit { cover: false }));
        }),
        widget(fit_option("填充", cover)).key("player-fit-cover").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(player_message(PlayerMessage::SetObjectFit { cover: true }));
        }),
    ))
    .key("player-fit")
    .into_any()
}

fn fit_option(label: &str, active: bool) -> Button {
    let mut button = Button::new(label);
    button.style.background = active.then_some(SemanticColorRole::AccentSoft);
    button.style.foreground = Some(if active { SemanticColorRole::Accent } else { SemanticColorRole::Muted });
    button.style.border = None;
    button.style.radius = Some(RadiusTier::Sm);
    button.style.control_height = None;
    button.style.control_padding_x = None;
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.height = Some(LengthSpec::Px(32.0));
    layout.min_height = Some(LengthSpec::Px(26.0));
    layout.padding_left = Some(LengthSpec::Px(8.0));
    layout.padding_right = Some(LengthSpec::Px(8.0));
    layout.padding_top = Some(LengthSpec::Px(4.0));
    layout.padding_bottom = Some(LengthSpec::Px(4.0));
    layout.font_size = Some(12.0);
    layout.font_weight = Some(500);
    let style = button.style.clone();
    Button::new(label).style(style)
}

/// 当前队列浮层。每一项是封面加两行字，当前项铺 accent-soft；不可播放的项变淡、不可点。
pub(super) fn queue_popover(props: &BarProps) -> AnyView {
    let media = props.media_label();
    let rows = props
        .queue
        .iter()
        .map(|item| queue_row(item, media, props.current_id.as_deref() == Some(item.id.as_str())))
        .collect::<Vec<_>>();
    let head = widget(Stack::bar(10.0).justify(JustifySpec::SpaceBetween).align(AlignSpec::Center))
        .children((
            widget(Text::new("当前队列").color(SemanticColorRole::Text).font_size(13.0).font_weight(700).line_height(QUEUE_HEAD))
                .key("player-queue-title"),
            widget(Text::new(format!("{} 项", props.queue.len())).color(SemanticColorRole::Muted).font_size(12.0).line_height(18.6))
                .key("player-queue-count"),
        ))
        .into_any();
    let list_limit = QUEUE_MAX_HEIGHT - QUEUE_PADDING * 2.0 - 2.0 - QUEUE_HEAD - 8.0;
    let list = widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.width = Some(LengthSpec::Fill);
        layout.max_height = Some(LengthSpec::Px(list_limit));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(1.0);
    }))
    .children((widget(Stack::column(6.0)).children(rows),))
    .key("player-queue-list")
    .into_any();
    widget(
        Stack::column(8.0)
            .padding(QUEUE_PADDING)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderStrong, 1.0)
            .radius(RadiusTier::Xl)
            .with_layout(|layout| {
                layout.position = nana_ui_core::PositionSpec::Absolute;
                // Vue 相对卡片内边距盒：右 14、底边在卡片上沿再往上 10。这里换算到内容盒。
                layout.offset_right = Some(LengthSpec::Px(14.0 - CARD_PADDING_X));
                layout.offset_bottom = Some(LengthSpec::CalcPercentOffset { percent: 100.0, offset_px: 10.0 + CARD_PADDING_TOP });
                layout.width = Some(LengthSpec::Px(360.0));
                layout.max_width = Some(LengthSpec::CalcViewportOffset {
                    axis: nana_ui_core::ViewportAxis::Width,
                    value: 100.0,
                    offset_px: -48.0,
                });
                layout.max_height = Some(LengthSpec::Px(QUEUE_MAX_HEIGHT));
                layout.paint.box_shadows = vec![nana_ui_core::BoxShadowSpec {
                    offset_x: 0.0,
                    offset_y: 14.0,
                    blur_radius: 34.0,
                    spread_radius: -18.0,
                    color: [0.0, 0.0, 0.0, 0.6],
                    inset: false,
                }];
            }),
    )
    .children((head, list))
    .key("player-queue-popover")
    .into_any()
}

fn queue_row(item: &QueueItem, media: &'static str, active: bool) -> AnyView {
    let ready = item.status == "ready";
    let title = display_title(&item.filename, &item.extension);
    let detail = if ready { item.path.clone() } else { item.status_reason.clone().unwrap_or_else(|| item.status.clone()) };
    let id = item.id.clone();
    let hit = widget(hit_area(&title, !ready, 8.0, 10.0)).key(format!("player-queue-{}", item.id)).on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(player_message(PlayerMessage::PlayItem { item_id: id.clone() }));
    });
    let title_role = if active { SemanticColorRole::Accent } else { SemanticColorRole::Text };
    let meta = widget(Stack::column(2.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
        widget(line(title, 14.0, 700, title_role, 21.7)),
        widget(line(detail, 11.667, 500, SemanticColorRole::Muted, 18.08)),
    ));
    let surface = if active { SemanticColorRole::AccentSoft } else { SemanticColorRole::Background };
    widget(
        Stack::row(10.0)
            .align(AlignSpec::Start)
            .padding(8.0)
            .min_height(LengthSpec::Px(56.0))
            .width(LengthSpec::Fill)
            .surface(surface)
            .radius(RadiusTier::Lg)
            .with_layout(|layout| layout.opacity = (!ready).then_some(0.52)),
    )
    .children((hit, type_cover_small(media), meta))
    .into_any()
}

fn type_cover_small(label: &'static str) -> AnyView {
    type_cover(label, 40.0, "player-queue-cover")
}

fn player_message(message: PlayerMessage) -> ShellMessage {
    ShellMessage::Player(message)
}
