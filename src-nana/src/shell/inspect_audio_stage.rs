//! 音频预览的播放器挂载，对齐 player-audio 运行时的 `media-preview--audio` 外壳。
//!
//! 左边是唱片舞台（唱片、封面圆、标题和「音频」标签），右边是歌词面板。播放、暂停、跳转和音量
//! 都在底部播放条上，这里没有控件。唱片占满标题以外的空间，按宽高里较小的一边画正圆，最大 380px；
//! Vue 按视口定死尺寸，预览框小时唱片被裁成方块，这里不照抄。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{AlignSpec, JustifySpec, LengthSpec, RadiusTier, SemanticColorRole, Stack, TextHorizontalAlignment};

use super::super::ShellViewModel;
use super::frame::text;
use super::preview_paint::VinylRecord;

/// 唱片最大直径，对应 `--audio-record-size` 的上限。
const RECORD_MAX: f32 = 380.0;
/// Vue `@media (max-width: 820px)`：舞台和歌词改成上下排。
const STACK_BREAKPOINT: f32 = 820.0;

/// 音频舞台。没有封面时唱片中间写「音频」，副标题写「通用音频」。
pub(super) fn audio_stage(model: &ShellViewModel) -> AnyView {
    let viewport = model.viewport_width.max(1.0);
    let stacked = viewport <= STACK_BREAKPOINT;
    let path = model.inspect.target_path.clone().unwrap_or_default();
    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
    let extension = name.rsplit_once('.').map(|(_, ext)| ext.to_string()).unwrap_or_default();
    let title = super::super::player_view::bar::display_title(&name, &extension);
    // h2 字号 clamp(17px, 1.8vw, 22px)。
    let title_size = (viewport * 0.018).clamp(17.0, 22.0);
    let mut heading = text(&title, title_size, 700, SemanticColorRole::Text, title_size * 1.24).truncating();
    heading.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let mut secondary = text("通用音频", 12.0, 400, SemanticColorRole::Muted, 18.6).truncating();
    secondary.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let caption = widget(Stack::column(8.0).align(AlignSpec::Center).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((
            widget(heading).key("inspect-audio-title"),
            widget(secondary).key("inspect-audio-artist"),
            widget(Stack::row(8.0).justify(JustifySpec::Center)).children((pill("音频", false, "inspect-audio-kind"),)),
        ))
        .key("inspect-audio-caption")
        .into_any();
    let stage = widget(
        Stack::column(18.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .padding(if stacked { 0.0 } else { 24.0 })
            .with_layout(move |layout| {
                layout.flex_grow = Some(0.92);
                layout.flex_shrink = Some(1.0);
                layout.flex_basis = Some(LengthSpec::Px(0.0));
                layout.height = Some(LengthSpec::Fill);
            }),
    )
    .children((record(), caption))
    .key("inspect-audio-stage")
    .into_any();
    let lyrics = widget(
        Stack::fill_column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .min_width(LengthSpec::Px(0.0))
            .padding(18.0)
            .with_layout(|layout| {
                layout.flex_grow = Some(1.08);
                layout.flex_basis = Some(LengthSpec::Px(0.0));
            }),
    )
    .children((widget(text("暂无歌词", 14.0, 400, SemanticColorRole::Muted, 21.7)).key("inspect-audio-lyrics-empty"),))
    .key("inspect-audio-lyrics")
    .into_any();
    // 栏距 clamp(24px, 4vw, 58px)。
    let gap = if stacked { 14.0 } else { (viewport * 0.04).clamp(24.0, 58.0) };
    let layout = if stacked { Stack::fill_column(gap) } else { Stack::fill_row(gap).align(AlignSpec::Stretch) };
    let shell = widget(
        Stack::fill_column(0.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Xl)
            .padding(if stacked { 18.0 } else { 22.0 })
            .min_height(LengthSpec::Px(0.0))
            .max_width(920.0),
    )
    .children((widget(layout.min_height(LengthSpec::Px(0.0))).children((stage, lyrics)).key("inspect-audio-layout"),))
    .key("inspect-audio-shell")
    .into_any();
    widget(Stack::fill_column(0.0).align(AlignSpec::Center).justify(JustifySpec::Center).surface(SemanticColorRole::Background))
        .children((shell,))
        .key("inspect-player-mount")
        .into_any()
}

/// 唱片：占满舞台里标题以外的空间（最大 380×380），自绘按较小的一边画正圆，
/// 中间 58% 的封面圆也由同一个自绘画出；「音频」标签居中。
fn record() -> AnyView {
    widget(
        Stack::column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Fill)
            .max_width(RECORD_MAX)
            .painter(VinylRecord)
            .grow(1.0)
            .shrink(1.0)
            .with_layout(|layout| {
                layout.flex_basis = Some(LengthSpec::Px(0.0));
                layout.min_height = Some(LengthSpec::Px(0.0));
                layout.max_height = Some(LengthSpec::Px(RECORD_MAX));
            }),
    )
    .children((pill("音频", true, "inspect-audio-chip"),))
    .key("inspect-audio-record")
    .into_any()
}

/// 胶囊标签：26px 高，12px 半粗弱色字。`strong` 时用 border，否则 border-soft。
fn pill(label: &str, strong: bool, key: &'static str) -> AnyView {
    let border = if strong { SemanticColorRole::Border } else { SemanticColorRole::BorderSoft };
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .min_height(LengthSpec::Px(26.0))
            .padding_xy(8.0, 4.0)
            .surface(SemanticColorRole::Surface)
            .outline(border, 1.0)
            .radius_px(999.0)
            .grow(0.0)
            .shrink(0.0),
    )
    .children((widget(text(label, 12.0, 600, SemanticColorRole::Muted, 18.6)),))
    .key(key)
    .into_any()
}

/// 准备卡：读取或解码时居中显示文件名、「准备音频」和一条进度。
pub(super) fn loading_card(model: &ShellViewModel) -> AnyView {
    let path = model.inspect.target_path.clone().unwrap_or_default();
    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
    let name = if name.is_empty() { "准备播放".to_string() } else { name };
    let card = widget(
        Stack::column(10.0)
            .padding(14.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md)
            .max_width(360.0)
            .with_layout(|layout| {
                layout.width = Some(LengthSpec::CalcPercentOffset { percent: 100.0, offset_px: -32.0 });
            }),
    )
    .children((
        widget(text(&name, 13.0, 700, SemanticColorRole::Text, 20.15).truncating()).key("inspect-media-loading-name"),
        widget(text("准备音频", 12.0, 400, SemanticColorRole::Muted, 18.6).truncating()).key("inspect-media-loading-detail"),
        progress_track(0.0, "inspect-media-loading-progress"),
    ))
    .key("inspect-media-loading")
    .into_any();
    widget(Stack::fill_column(0.0).align(AlignSpec::Center).justify(JustifySpec::Center).surface(SemanticColorRole::Background))
        .children((card,))
        .key("inspect-player-mount")
        .into_any()
}

/// 6px 进度轨：border-soft 58% 的底，accent 填充一段表示正在准备。`width` 为 0 时铺满父级。
pub(super) fn progress_track(width: f32, key: &'static str) -> AnyView {
    let mut track = Stack::row(0.0)
        .height(LengthSpec::Px(6.0))
        .min_height(LengthSpec::Px(6.0))
        .radius_px(999.0)
        .grow(0.0)
        .shrink(0.0)
        .with_layout(|layout| {
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        });
    track = if width > 0.0 { track.width(LengthSpec::Px(width)) } else { track.width(LengthSpec::Fill) };
    let style = track
        .node_style()
        .surface_mix(nana_ui_core::SemanticColorMix::alpha(SemanticColorRole::BorderSoft, 0.58));
    let fill = widget(Stack::row(0.0).width(LengthSpec::Percent(42.0)).height(LengthSpec::Fill).surface(SemanticColorRole::Accent).radius_px(999.0));
    widget(track.style(style)).children((fill,)).key(key).into_any()
}
