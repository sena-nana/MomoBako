//! 音频预览的播放器挂载：唱片舞台和歌词面板。
//! 标题和封面按元数据取。歌词居中，当前行放大并带投影；有时间的行可点，焦点画下沿。
//! 每一行按 16px 字号的行盒折行，长句不收成省略号。

use std::collections::BTreeMap;

use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, Entity, JustifySpec, LengthSpec, RadiusTier, RuntimeDocument,
    ScrollAxes, ScrollOffset, ScrollView, ScrollViewportChanged, SemanticColorRole, StableNodeId,
    Stack, TextHorizontalAlignment,
};
use nana_ui_core::{ScrollEdgeFade, SpacingStep, TextWeight};
use serde_json::Value;

use super::super::ShellMessage;
use super::super::files_view::style as files_style;
use super::super::player::PlayerMessage;
use super::frame::text;
use super::preview_paint::VinylRecord;
use crate::theme_map::tiers::{self, Hairline};

mod diameter;
mod lyric_box;
pub(crate) mod lyric_focus;
pub(crate) mod lyric_lift;
mod lyric_pad;
mod pills;
mod split;
mod stage_pad;
mod title_width;
use lyric_box::{GAP as LYRIC_GAP, GAP_STEP as LYRIC_GAP_STEP, LINE as LYRIC_LINE};
/// 窗口不宽于 820px 时，舞台和歌词改成上下排。
const STACK_BREAKPOINT: f32 = 820.0;
/// 歌词行：正文先和 `--bg` 按 0.42 混，再盖不透明度。背景是 `--bg` 时收成一次混合。
const LYRIC_INK: f32 = 0.42;
const LYRIC_AHEAD_MIX: f32 = LYRIC_INK * 0.46;
const LYRIC_PASSED_MIX: f32 = LYRIC_INK * 0.62;
const LYRIC_HOVER_MIX: f32 = 0.88 * 0.82;
/// 歌词上下留白的初值。视口量出来以前，上下留白先用它。
const LYRIC_INSET_INITIAL: f32 = 104.0;
/// 歌词视口顶上渐隐带占视口高度的比例。
const LYRIC_FADE_TOP: f32 = 0.12;
/// 歌词视口底下渐隐带占视口高度的比例。
const LYRIC_FADE_BOTTOM: f32 = 0.14;
/// 窄窗口里舞台和歌词之间的间距档。
const STACKED_GAP: SpacingStep = SpacingStep::Xxl;
/// 舞台卡片最宽 920、准备卡最宽 360：阅读宽度，不是间距，照写像素。
const STAGE_MAX_WIDTH: f32 = 920.0;
const LOADING_MAX_WIDTH: f32 = 360.0;
/// 唱片舞台上的三行字：标题、副标题，以及非空时画在「音频」旁边的专辑。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AudioCaption {
    pub title: String,
    pub subtitle: String,
    pub album: String,
}

/// 预览条目上的元数据。列表行和搜索命中是映射，播放集条目是 JSON 对象。
#[derive(Clone, Copy)]
pub(super) enum AudioMeta<'a> {
    Map(&'a BTreeMap<String, Value>),
    Object(&'a serde_json::Map<String, Value>),
    Empty,
}

impl<'a> AudioMeta<'a> {
    fn get(self, key: &str) -> Option<&'a Value> {
        match self {
            Self::Map(map) => map.get(key),
            Self::Object(object) => object.get(key),
            Self::Empty => None,
        }
    }
}

/// 舞台上的标题、副标题、专辑胶囊。
///
/// 标题用去掉空白的 `title`，没有就用去掉扩展名的文件名。`artists` 是数组时只用其中的非空字符串，
/// 用「 / 」连起来，不再看单个 `artist`；不是数组时才用 `artist`。副标题优先艺人，否则专辑，都没有写「通用音频」。
/// 专辑另有一枚胶囊，和副标题是否已经写了专辑无关。
pub(super) fn audio_caption(name: &str, extension: &str, metadata: AudioMeta<'_>) -> AudioCaption {
    let title = trimmed_text(metadata.get("title"))
        .unwrap_or_else(|| super::super::player_view::bar::display_title(name, extension));
    let artist = match metadata.get("artists") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| trimmed_text(Some(item)))
            .collect::<Vec<_>>()
            .join(" / "),
        _ => trimmed_text(metadata.get("artist")).unwrap_or_default(),
    };
    let album = trimmed_text(metadata.get("album")).unwrap_or_default();
    let subtitle = if !artist.is_empty() {
        artist
    } else if !album.is_empty() {
        album.clone()
    } else {
        "通用音频".to_string()
    };
    AudioCaption {
        title,
        subtitle,
        album,
    }
}

/// 字符串去掉首尾空白。不是字符串，或去掉以后是空的，都当成没有。
fn trimmed_text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// 音频舞台读的这一条：先用文件列表里同一路径（当前目录、智能文件夹或分类），
/// 列表里没有时用播放集条目，再没有才用搜索命中。封面和标题、艺人、专辑用同一条，素材详情不另覆盖。
pub(super) struct PreviewEntry<'a> {
    pub meta: AudioMeta<'a>,
    /// 这一条上已经选好的本地封面路径。没有图、或只有网址时为 `None`。
    pub cover: Option<&'a str>,
}

/// 取出音频舞台这一条的元数据和封面路径。
pub(super) fn preview_entry(model: &crate::shell::ShellViewModel) -> PreviewEntry<'_> {
    let empty = PreviewEntry {
        meta: AudioMeta::Empty,
        cover: None,
    };
    let Some(path) = model
        .inspect
        .target_path
        .as_deref()
        .filter(|path| !path.is_empty())
    else {
        return empty;
    };
    let files = &model.files;
    let ctx = super::super::files::FileContext::from_model(model);
    if let Some(row) = files
        .listed_rows(&ctx)
        .into_iter()
        .find(|row| row.path == path)
    {
        return entry_of(AudioMeta::Map(&row.metadata), row.thumbnail_path.as_deref());
    }
    if let Some(row) = files
        .rows
        .iter()
        .chain(files.virtual_rows.iter())
        .chain(files.category_rows.iter())
        .find(|row| row.path == path)
    {
        return entry_of(AudioMeta::Map(&row.metadata), row.thumbnail_path.as_deref());
    }
    if let Some(item) = model
        .player
        .listed
        .as_ref()
        .and_then(|detail| detail.items.iter().find(|item| item.path == path))
    {
        let meta = match item.metadata.as_ref() {
            Some(value) => match value.as_object() {
                Some(object) => AudioMeta::Object(object),
                None if value.is_null() => AudioMeta::Empty,
                None => {
                    eprintln!("Nana 播放集条目的元数据不是对象，音频舞台不采用：{path}");
                    AudioMeta::Empty
                }
            },
            None => AudioMeta::Empty,
        };
        return entry_of(meta, item.thumbnail_path.as_deref());
    }
    if let Some(hit) = model.inspect.results.iter().find(|hit| hit.path == path) {
        return entry_of(AudioMeta::Map(&hit.metadata), None);
    }
    empty
}

/// 封面路径和元数据配成一条。`thumbnail` 是条目上的 `thumbnailPath`，自定义缩略图也在这里。
fn entry_of<'a>(meta: AudioMeta<'a>, thumbnail: Option<&'a str>) -> PreviewEntry<'a> {
    PreviewEntry {
        cover: cover_file(thumbnail, meta),
        meta,
    }
}

/// 封面先用 `thumbnailPath`，没有再用 `coverArt`。去掉空白后是空的不算有图。
/// `http` / `https` 还没落地成文件，这里不下载，交回空，唱片保持封面圆。
pub(super) fn cover_file<'a>(
    thumbnail: Option<&'a str>,
    metadata: AudioMeta<'a>,
) -> Option<&'a str> {
    let path = present(thumbnail).or_else(|| {
        metadata
            .get("coverArt")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
    })?;
    if is_remote(path) { None } else { Some(path) }
}

/// 本地封面文件的长度和修改时间。文件不在时为 `None`，调用方保持封面圆。
pub(super) fn cover_stamp(path: &str) -> Option<(u64, u128)> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    Some((meta.len(), modified))
}

/// 用现有缩略图解码把本地图片收成封面 data URL。读失败、不是图片或编不成图时记日志并返回空，不填假像素。
pub(super) fn cover_data_url(path: &str) -> Option<String> {
    let frame = match super::super::decode_thumbnail_file(path) {
        Ok(frame) => frame,
        Err(error) => {
            eprintln!("Nana 音频封面读不出来，唱片保持封面圆：{path}：{error}");
            return None;
        }
    };
    match super::super::inspect::rgba_png_data_url(frame.width, frame.height, &frame.rgba) {
        Some(url) => Some(url),
        None => {
            eprintln!("Nana 音频封面编码失败，唱片保持封面圆：{path}");
            None
        }
    }
}

/// 标题下的「音频」和专辑。
///
/// 胶囊是可折行的横排，间距 `Md`，在标题宽度里居中。
/// 两枚放不进这一宽时，专辑整枚换到下一行，字不收成省略号。
/// 宽度和标题同一条；视口还没量到时这条长度解析不成像素，不另填 220 或 380。
fn meta_row(chips: Vec<AnyView>, stacked: bool) -> AnyView {
    widget(
        files_style::row(SpacingStep::Md)
            .wrap(true)
            .justify(JustifySpec::Center)
            .align(AlignSpec::Center)
            .with_layout(move |layout| {
                title_width::pin_width(layout, stacked);
            }),
    )
    .children(chips)
    .key("inspect-audio-meta")
    .into_any()
}

/// 去掉首尾空白。空的当成没有。
fn present(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|text| !text.is_empty())
}

/// `http://` 或 `https://`。这种地址还不是本地文件。
fn is_remote(path: &str) -> bool {
    (path.len() >= 7 && path[..7].eq_ignore_ascii_case("http://"))
        || (path.len() >= 8 && path[..8].eq_ignore_ascii_case("https://"))
}

/// 音频舞台。`viewport` 是窗口宽。`caption` 是标题、副标题和专辑。
/// `lines` 是已经解析的歌词；`active` 是播放头走到的那一行。
/// `pending` 时歌词文件还在读，空面板写「读取歌词...」。
/// `art` 是封面的 data URL；没有时唱片中间写「音频」。
pub(super) fn audio_stage(
    viewport: f32,
    lines: &[super::super::inspect_lyrics::LyricLine],
    active: Option<usize>,
    caption: &AudioCaption,
    art: Option<&str>,
    pending: bool,
) -> AnyView {
    let measured = viewport.is_finite() && viewport > 0.0;
    if !measured {
        eprintln!("Nana 音频舞台视口还没量到，格子不按 minmax 分成像素：{viewport}");
    }
    let viewport = viewport.max(1.0);
    let stacked = viewport <= STACK_BREAKPOINT;
    // h2 字号 clamp(17px, 1.8vw, 22px)。
    let title_size = (viewport * 0.018).clamp(17.0, 22.0);
    let mut heading = text(
        &caption.title,
        title_size,
        TextWeight::Bold,
        SemanticColorRole::Text,
        title_size * 1.24,
    )
    .truncating();
    // 标题盒子跟唱片直径走。文案仍是 title / 艺人 / 专辑那套，这里只卡宽度。
    title_width::pin_width(std::sync::Arc::make_mut(&mut heading.style.layout), stacked);
    heading.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let mut secondary = text(
        &caption.subtitle,
        12.0,
        TextWeight::Regular,
        SemanticColorRole::Muted,
        18.6,
    )
    .truncating();
    secondary.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let mut chips = vec![pills::pill("音频", false, "inspect-audio-kind")];
    if !caption.album.is_empty() {
        chips.push(pills::pill(&caption.album, false, "inspect-audio-album"));
    }
    let meta = meta_row(chips, stacked);
    let heading_block = widget(
        files_style::column(SpacingStep::Md)
            .align(AlignSpec::Center)
            .with_layout(move |layout| {
                title_width::pin_width(layout, stacked);
            }),
    )
    .children((
        widget(heading).key("inspect-audio-title"),
        widget(secondary).key("inspect-audio-artist"),
        meta,
    ))
    .key("inspect-audio-caption")
    .into_any();
    // 栏距：宽窗口 `clamp(24px, 4vw, 58px)`（下限 24 是 `Page` 档，其上跟着窗口宽连续变），窄窗口 `Xxl`。
    // 列和行的 minmax 用同一段间距。
    let gap = if stacked {
        tiers::space_px(STACKED_GAP)
    } else {
        (viewport * 0.04).clamp(24.0, 58.0)
    };
    let stage = stage_pad::stage_view(stacked, gap, measured, record(art, stacked), heading_block);
    let lyrics = widget(
        Stack::fill_column(0.0)
            .align(if lines.is_empty() {
                AlignSpec::Center
            } else {
                AlignSpec::Stretch
            })
            .justify(if lines.is_empty() {
                JustifySpec::Center
            } else {
                JustifySpec::Start
            })
            .min_width(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                if lines.is_empty() {
                    tiers::padding(layout, &tiers::Edge::ALL, SpacingStep::PageTight);
                }
                layout.flex_grow = Some(1.0);
                layout.flex_shrink = Some(1.0);
                layout.flex_basis = Some(LengthSpec::Px(0.0));
                layout.min_height = Some(LengthSpec::Px(0.0));
            }),
    )
    .children((lyric_body(lines, active, pending),))
    .key("inspect-audio-lyrics")
    .into_any();
    // 歌词列：宽窗口上下留白，空态的 `PageTight` 仍在歌词面板上。
    let info = lyric_pad::info_view(stacked, gap, measured, lyrics);
    let layout = if stacked {
        Stack::fill_column(0.0).gap_theme(STACKED_GAP)
    } else {
        // 下限是 `Page` 档，宽窗口按 4vw 多出来的一段加在档位上：主题换间距刻度时下限跟着变。
        let wide = (gap - tiers::space_px(SpacingStep::Page)).max(0.0);
        Stack::fill_row(0.0)
            .align(AlignSpec::Stretch)
            .with_layout(move |layout| {
                tiers::gap_by(layout, tiers::space_expr(SpacingStep::Page).offset(wide));
            })
    };
    let shell_pad = if stacked {
        SpacingStep::PageTight
    } else {
        SpacingStep::Page
    };
    let shell = widget(
        crate::theme_map::tiers::pad_xy(
            Hairline::hairline(
                Stack::fill_column(0.0).surface(SemanticColorRole::Surface),
                SemanticColorRole::BorderSoft,
            ),
            shell_pad,
            shell_pad,
        )
        .radius(RadiusTier::Xl)
        .min_height(LengthSpec::Px(0.0))
        .max_width(STAGE_MAX_WIDTH)
        .painter(super::preview_paint::AudioCardFace),
    )
    .children((widget(
        layout
            .min_height(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                // 舞台 `overflow: hidden`：下限放不下时裁掉溢出，不把列撑出舞台。
                layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
                layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
            }),
    )
    .children((stage, info))
    .key("inspect-audio-layout"),))
    .key("inspect-audio-shell")
    .into_any();
    widget(
        Stack::fill_column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .surface(SemanticColorRole::Background),
    )
    .children((shell,))
    .key("inspect-player-mount")
    .into_any()
}

/// 歌词面板的内容。没有行时是「暂无歌词」；有行时放进纵向滚动区。
/// 有时间的行是按钮，点一下把播放头跳到这一行；没有时间的行只是文字。
/// 上下留白：视口量出来以后改成 `max(96, floor(高 / 2))`，
/// 上沿再减 1。当前行在这次布局里滚到正中。
fn lyric_body(
    lines: &[super::super::inspect_lyrics::LyricLine],
    active: Option<usize>,
    pending: bool,
) -> AnyView {
    if lines.is_empty() {
        // 占位文案：`loading` 写「读取歌词...」，读完没有正文才写「暂无歌词」。
        let (label, key) = if pending {
            ("读取歌词...", "inspect-audio-lyrics-loading")
        } else {
            ("暂无歌词", "inspect-audio-lyrics-empty")
        };
        return widget(text(
            label,
            14.0,
            TextWeight::Regular,
            SemanticColorRole::Muted,
            21.7,
        ))
        .key(key)
        .into_any();
    }
    let inset = signal(LYRIC_INSET_INITIAL);
    let fade = signal(ScrollEdgeFade::default());
    let rows = lines
        .iter()
        .enumerate()
        .map(|(index, line)| lyric_line(index, line, active))
        .collect::<Vec<_>>();
    let list = widget(
        files_style::column(LYRIC_GAP_STEP)
            .width(LengthSpec::Fill)
            .align(AlignSpec::Stretch)
            .grow(0.0)
            .shrink(0.0),
    )
    .children(rows)
    .key("inspect-audio-lyric-list")
    .into_any();
    let top = inset;
    let bottom = inset;
    let track = widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .align(AlignSpec::Stretch)
            .grow(0.0)
            .shrink(0.0),
    )
    .children((
        dynamic(top, |value: &f32| {
            lyric_pad((*value - 1.0).max(0.0), "inspect-audio-lyric-pad-top")
        }),
        list,
        dynamic(bottom, |value: &f32| {
            lyric_pad(*value, "inspect-audio-lyric-pad-bottom")
        }),
    ))
    .key("inspect-audio-lyric-track")
    .into_any();
    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Fill);
    }))
    .children((track,))
    .key("inspect-audio-lyric-scroll")
    .bind(move |view: &mut ScrollView| set_lyric_edge_fade(view, fade.get()))
    .on_cx(move |_view, event: &ScrollViewportChanged, cx| {
        center_lyric_viewport(event, active, inset, fade, cx)
    })
    .into_any()
}

/// 视口高度有了以后收成按视口算的留白和上下渐隐带，并按行号先滚一次。
/// 实测盒子由 [`follow_active_lyric`] 收口。
fn center_lyric_viewport(
    event: &ScrollViewportChanged,
    active: Option<usize>,
    inset: nana_ui::runtime::view::Signal<f32>,
    fade: nana_ui::runtime::view::Signal<ScrollEdgeFade>,
    cx: &mut nana_ui::runtime::ViewContext<'_, ScrollView>,
) {
    if !event.height.is_finite() {
        eprintln!("Nana 歌词视口高度无效，当前行不滚：{}", event.height);
        return;
    }
    if event.height <= 0.0 {
        return;
    }
    fade.set_if_changed(lyric_edge_fade(event.height));
    let next = lyric_inset(event.height);
    if inset.with_untracked(|current| (*current - next).abs() >= 0.5) {
        inset.set(next);
    }
    let Some(index) = active else {
        return;
    };
    let y = lyric_scroll_top(index, event.height, next);
    let id = cx.entity().stable_id();
    cx.mutations()
        .set_scroll_offset(id, ScrollOffset { x: 0.0, y });
}

/// 把渐隐带写进滚动区样式。滚动区连同歌词合成一层淡出，每一行原来的明暗、放大和投影都不动。
/// 视口量出来以前两条带都是 0，不做合成。带长没变时不改样式。
fn set_lyric_edge_fade(view: &mut ScrollView, fade: ScrollEdgeFade) {
    let next = fade.is_active().then_some(fade);
    if view.style.layout.paint.scroll_edge_fade == next {
        return;
    }
    std::sync::Arc::make_mut(&mut view.style.layout)
        .paint
        .scroll_edge_fade = next;
}

/// 视口高度对应的上下渐隐带：顶上 12%，底下 14%，按整像素取，免得布局的亚像素抖动来回改样式。
fn lyric_edge_fade(viewport: f32) -> ScrollEdgeFade {
    ScrollEdgeFade::new(
        (viewport * LYRIC_FADE_TOP).round(),
        (viewport * LYRIC_FADE_BOTTOM).round(),
    )
}

/// 歌词上下留白：至少 96，否则是视口高度的一半，向下取整。
fn lyric_inset(viewport: f32) -> f32 {
    (viewport / 2.0).floor().max(96.0)
}
/// `scrollTop = max(0, offsetTop - clientHeight / 2 + 行高 / 2)`。`offsetTop` 含上留白 `inset - 1` 和前面的行。
/// 这里按单行盒子估算。折行以后的真实行高由 [`follow_active_lyric`] 用布局盒收口。
fn lyric_scroll_top(index: usize, viewport: f32, inset: f32) -> f32 {
    let offset_top = (inset - 1.0).max(0.0) + index as f32 * (LYRIC_LINE + LYRIC_GAP);
    (offset_top - viewport / 2.0 + LYRIC_LINE / 2.0).max(0.0)
}
/// 一行留白。高度是像素，不参与伸展，避免把歌词行距撑开。
fn lyric_pad(height: f32, key: &'static str) -> AnyView {
    let height = height.max(0.0);
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Px(height))
            .min_height(LengthSpec::Px(height))
            .grow(0.0)
            .shrink(0.0),
    )
    .key(key)
    .into_any()
}
/// 上次把哪一行、在多高的视口里滚到了中间。同一行、同一高度不再滚，免得把读者自己滚开的位置拉回去。
pub(crate) struct LyricFollow {
    scroll: StableNodeId,
    active: Option<StableNodeId>,
    viewport: u32,
}

/// 当前行滚到视口正中。没有滚动区或当前行就不改偏移，视口高或当前行变了才滚。
/// 滚动偏移变了返回 `true`，调用方据此再画一帧。
pub(crate) fn follow_active_lyric(
    document: &mut RuntimeDocument,
    followed: &mut Option<LyricFollow>,
) -> bool {
    if followed
        .as_ref()
        .is_some_and(|state| lyric_follow_holds(document, state))
    {
        return false;
    }
    let Some(scroll) = keyed(document, "inspect-audio-lyric-scroll") else {
        *followed = None;
        return false;
    };
    let active = keyed(document, "inspect-audio-lyric-active");
    let ready = {
        let world = document.context().world();
        let metrics = world.scroll_metrics(scroll);
        let viewport = metrics.map(|item| item.viewport_height);
        (viewport, metrics.and_then(|_| world.layout_box(scroll)))
    };
    let (Some(viewport), Some(view)) = ready else {
        return false;
    };
    if !(viewport > 0.0) {
        return false;
    }
    let viewport_bits = viewport.to_bits();
    if followed.as_ref().is_some_and(|state| {
        state.scroll == scroll && state.active == active && state.viewport == viewport_bits
    }) {
        return false;
    }
    let Some(active_id) = active else {
        *followed = Some(LyricFollow {
            scroll,
            active: None,
            viewport: viewport_bits,
        });
        return false;
    };
    let Some(item) = document.context().world().layout_box(active_id) else {
        eprintln!("Nana 歌词当前行还没有布局，先不滚到中间");
        return false;
    };
    let target = item.y - view.y - view.height / 2.0 + item.height / 2.0;
    if !target.is_finite() {
        eprintln!(
            "Nana 歌词居中偏移算不出来：行 {:.1} 视口 {:.1}",
            item.y, view.height
        );
        return false;
    }
    let moved = match document.context_mut().scroll_to(
        Entity::<ScrollView>::from_stable_id(scroll),
        ScrollOffset {
            x: 0.0,
            y: target.max(0.0),
        },
    ) {
        Ok(moved) => moved,
        Err(error) => {
            eprintln!("Nana 歌词滚到当前行失败：{error}");
            return false;
        }
    };
    *followed = Some(LyricFollow {
        scroll,
        active: Some(active_id),
        viewport: viewport_bits,
    });
    moved
}

/// 记下的那一行和视口高度都还在。同一行播放中不再整棵去找。
fn lyric_follow_holds(document: &RuntimeDocument, state: &LyricFollow) -> bool {
    let Some(active) = state.active else {
        return false;
    };
    let context = document.context();
    if !key_is(&context, state.scroll, "inspect-audio-lyric-scroll")
        || !key_is(&context, active, "inspect-audio-lyric-active")
    {
        return false;
    }
    context
        .world()
        .scroll_metrics(state.scroll)
        .is_some_and(|metrics| {
            metrics.viewport_height > 0.0 && metrics.viewport_height.to_bits() == state.viewport
        })
}

/// 节点的键路径最后一段是不是 `key`。
fn key_is(context: &nana_ui::runtime::AppContext, id: StableNodeId, key: &str) -> bool {
    context
        .assembly_path(id)
        .is_some_and(|path| path.rsplit('/').next() == Some(key))
}

/// 键路径最后一段是 `key` 的节点。
fn keyed(document: &RuntimeDocument, key: &str) -> Option<StableNodeId> {
    let context = document.context();
    context
        .world()
        .document_order(document.document())
        .into_iter()
        .find(|id| {
            context
                .assembly_path(*id)
                .is_some_and(|path| path.rsplit('/').next() == Some(key))
        })
}

/// 一行歌词。有时间时整行可点，跳到这一行的毫秒；没有时间时不可点。
/// 点击把 `currentTime` 设成 `timeMs / 1000`，按钮在 `timeMs == null` 时禁用。
/// 当前项还没装好时，播放器的跳转自己拒绝并记日志，不把会话标成失败。
fn lyric_line(
    index: usize,
    line: &super::super::inspect_lyrics::LyricLine,
    active: Option<usize>,
) -> AnyView {
    let mix = lyric_tone(index, line.time_ms.is_none(), active);
    let key = if active == Some(index) {
        "inspect-audio-lyric-active".to_string()
    } else {
        format!("inspect-audio-lyric-{index}")
    };
    let Some(time_ms) = line.time_ms else {
        let mut line = text(
            &line.text,
            lyric_box::FONT,
            TextWeight::Regular,
            SemanticColorRole::Text,
            lyric_box::LEADING,
        );
        paint_lyric_text(&mut line, mix);
        lyric_box::apply(std::sync::Arc::make_mut(&mut line.style.layout));
        return widget(line).key(key).into_any();
    };
    widget(lyric_button(&line.text, mix))
        .key(key)
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Player(PlayerMessage::Seek(time_ms)));
        })
        .into_any()
}

/// 有时间的歌词行：居中、按淡法混字色，悬停和键盘焦点再亮一档。不铺按钮底，避免画成芯片。
fn lyric_button(label: &str, mix: Option<f32>) -> Button {
    let mut button = Button::new(label);
    button.style.foreground = Some(SemanticColorRole::Text);
    button.style.background = None;
    button.style.border = None;
    button.style.radius = None;
    button.style.control_height = None;
    button.style.control_padding_x = None;
    button.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    button.style.interaction.base.foreground_mix = lyric_mix(mix);
    button.style.interaction.hovered.foreground_mix = lyric_mix(Some(LYRIC_HOVER_MIX));
    button.style.interaction.hovered.background = None;
    button.style.interaction.pressed.background = None;
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    lyric_box::apply(layout);
    if mix.is_none() {
        lyric_lift::lift_active_line(&mut button);
    }
    lyric_focus::mark(&mut button);
    let style = button.style.clone();
    Button::new(label)
        .content_align(TextHorizontalAlignment::Center)
        .style(style)
}

/// 没有时间的歌词不是按钮，字色和居中跟有时间的行同一套。
fn paint_lyric_text(node: &mut nana_ui::runtime::Text, mix: Option<f32>) {
    node.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    node.style.interaction.base.foreground_mix = lyric_mix(mix);
}

fn lyric_mix(ratio: Option<f32>) -> Option<nana_ui_core::SemanticColorMix> {
    ratio.map(|ratio| {
        nana_ui_core::SemanticColorMix::new(
            SemanticColorRole::Text,
            SemanticColorRole::Background,
            ratio,
        )
    })
}

/// 当前行不淡。已经唱过的略实。还没唱到的、以及排在后面的没有时间的行，用更淡的那一档。
fn lyric_tone(index: usize, untimed: bool, active: Option<usize>) -> Option<f32> {
    if active == Some(index) {
        None
    } else if !untimed && active.is_some_and(|current| index < current) {
        Some(LYRIC_PASSED_MIX)
    } else {
        Some(LYRIC_AHEAD_MIX)
    }
}

/// 唱片：窗口宽于 820px 时边长是 `clamp(220px, min(32vw, 48vh), 380px)`，
/// 不宽于 820px 时是 `clamp(140px, min(34vh, 56vw), 230px)`。自绘按较小的一边画正圆。
/// `art` 有值时铺进中间 58% 的圆，不再写「音频」；没有图时中间仍是封面圆，标签居中。
fn record(art: Option<&str>, narrow: bool) -> AnyView {
    let chip: Vec<AnyView> = if art.is_none() {
        vec![pills::pill("音频", true, "inspect-audio-chip")]
    } else {
        Vec::new()
    };
    let size = diameter::record_size(narrow);
    widget(
        Stack::column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(size)
            .height(size)
            .min_width(size)
            .min_height(size)
            .grow(0.0)
            .shrink(0.0)
            .painter(VinylRecord::with_art(art)),
    )
    .children(chip)
    .key("inspect-audio-record")
    .into_any()
}

/// 准备卡：读取或解码时居中。落影取主题的表面高度档，进度轨是不定进度。
/// 最宽 360，窄时左右各让出 `Xxxl`。
pub(super) fn loading_card(name: &str, video: bool) -> AnyView {
    let name = if name.is_empty() {
        "准备播放".to_string()
    } else {
        name.to_string()
    };
    let detail = if video {
        "准备媒体"
    } else {
        "准备音频"
    };
    let card = widget(
        crate::theme_map::tiers::pad_xy(
            Hairline::hairline(
                files_style::column(SpacingStep::Lg).surface(SemanticColorRole::Surface),
                SemanticColorRole::BorderSoft,
            ),
            SpacingStep::Xxl,
            SpacingStep::Xxl,
        )
        .radius(RadiusTier::Md)
        .max_width(LOADING_MAX_WIDTH)
        .painter(super::preview_paint::LoadingCardShadow)
        .with_layout(|layout| {
            let margin = tiers::space_px(SpacingStep::Xxxl);
            layout.width = Some(LengthSpec::percent_offset(100.0, -margin * 2.0));
        }),
    )
    .children((
        widget(
            text(
                &name,
                13.0,
                TextWeight::Bold,
                SemanticColorRole::Text,
                20.15,
            )
            .truncating(),
        )
        .key("inspect-media-loading-name"),
        widget(
            text(
                detail,
                12.0,
                TextWeight::Regular,
                SemanticColorRole::Muted,
                18.6,
            )
            .truncating(),
        )
        .key("inspect-media-loading-detail"),
        super::preview_paint::loading_progress(),
    ))
    .key("inspect-media-loading")
    .into_any();
    widget(
        Stack::fill_column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .surface(SemanticColorRole::Background),
    )
    .children((card,))
    .key("inspect-player-mount")
    .into_any()
}

#[cfg(test)]
mod tests;
