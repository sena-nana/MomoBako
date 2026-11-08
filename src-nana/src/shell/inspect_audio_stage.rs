//! 音频预览的播放器挂载，对齐 player-audio 的唱片舞台和歌词面板。
//! 标题和封面照 `audioDisplayMetadata`。歌词居中，当前行放大并带投影；有时间的行可点，焦点画下沿。
//! 每一行按 Vue 的 16px 行盒折行，长句不收成省略号。

use std::collections::BTreeMap;

use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, Entity, JustifySpec, LengthSpec, RadiusTier, RuntimeDocument,
    ScrollAxes, ScrollOffset, ScrollView, ScrollViewportChanged, SemanticColorRole, StableNodeId,
    Stack, TextHorizontalAlignment,
};
use serde_json::Value;

use super::super::ShellMessage;
use super::super::player::PlayerMessage;
use super::frame::text;
use super::preview_paint::VinylRecord;

#[path = "inspect_audio_diameter.rs"]
mod diameter;
#[path = "inspect_audio_lyric_box.rs"]
mod lyric_box;
#[path = "inspect_audio_lyric_fade.rs"]
mod lyric_fade;
#[path = "inspect_audio_lyric_focus.rs"]
pub(crate) mod lyric_focus;
#[path = "inspect_audio_lyric_lift.rs"]
pub(crate) mod lyric_lift;
#[path = "inspect_audio_lyric_pad.rs"]
mod lyric_pad;
#[path = "inspect_audio_pills.rs"]
mod pills;
#[path = "inspect_audio_split.rs"]
mod split;
#[path = "inspect_audio_stage_pad.rs"]
mod stage_pad;
#[path = "inspect_audio_title_width.rs"]
mod title_width;
use lyric_box::{GAP as LYRIC_GAP, LINE as LYRIC_LINE};
/// Vue `@media (max-width: 820px)`：舞台和歌词改成上下排。
const STACK_BREAKPOINT: f32 = 820.0;
/// `.media-preview__audio-lyric`：正文先和 `--bg` 按 0.42 混，再盖不透明度。背景是 `--bg` 时收成一次混合。
const LYRIC_INK: f32 = 0.42;
const LYRIC_AHEAD_MIX: f32 = LYRIC_INK * 0.46;
const LYRIC_PASSED_MIX: f32 = LYRIC_INK * 0.62;
const LYRIC_HOVER_MIX: f32 = 0.88 * 0.82;
/// Vue `lyricsInset` 的初值。视口量出来以前，上下留白先用它。
const LYRIC_INSET_INITIAL: f32 = 104.0;
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

/// 对照 Vue `audioDisplayMetadata` 和舞台上的标题、副标题、专辑胶囊。
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

/// 音频舞台读的这一条，对照 Vue `previewFileEntry`：先用文件列表里同一路径（当前目录、智能文件夹或分类），
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

/// 对照 `audioPreview.js`：`thumbnailPath || coverArt`。去掉空白后是空的不算有图。
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
/// Vue `.media-preview__audio-meta` 是可折行的横排，间距 8，在标题宽度里居中。
/// 两枚放不进这一宽时，专辑整枚换到下一行，字不收成省略号。
/// 宽度和标题同一条；视口还没量到时这条长度解析不成像素，不另填 220 或 380。
fn meta_row(chips: Vec<AnyView>, stacked: bool) -> AnyView {
    widget(
        Stack::row(8.0)
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
        700,
        SemanticColorRole::Text,
        title_size * 1.24,
    )
    .truncating();
    // 标题盒子跟唱片直径走。文案仍是 title / 艺人 / 专辑那套，这里只卡宽度。
    title_width::pin_width(std::sync::Arc::make_mut(&mut heading.style.layout), stacked);
    heading.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let mut secondary =
        text(&caption.subtitle, 12.0, 400, SemanticColorRole::Muted, 18.6).truncating();
    secondary.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let mut chips = vec![pills::pill("音频", false, "inspect-audio-kind")];
    if !caption.album.is_empty() {
        chips.push(pills::pill(&caption.album, false, "inspect-audio-album"));
    }
    let meta = meta_row(chips, stacked);
    let heading_block = widget(Stack::column(8.0).align(AlignSpec::Center).with_layout(
        move |layout| {
            title_width::pin_width(layout, stacked);
        },
    ))
    .children((
        widget(heading).key("inspect-audio-title"),
        widget(secondary).key("inspect-audio-artist"),
        meta,
    ))
    .key("inspect-audio-caption")
    .into_any();
    // 栏距：宽窗口 `clamp(24px, 4vw, 58px)`，窄窗口 14px。列和行的 minmax 用同一段间距。
    let gap = if stacked {
        14.0
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
            .padding(if lines.is_empty() { 18.0 } else { 0.0 })
            .with_layout(|layout| {
                layout.flex_grow = Some(1.0);
                layout.flex_shrink = Some(1.0);
                layout.flex_basis = Some(LengthSpec::Px(0.0));
                layout.min_height = Some(LengthSpec::Px(0.0));
            }),
    )
    .children((lyric_body(lines, active, pending),))
    .key("inspect-audio-lyrics")
    .into_any();
    // `.media-preview__audio-info`：宽窗口上下留白，空态的 18px 仍在歌词面板上。
    let info = lyric_pad::info_view(stacked, gap, measured, lyrics);
    let layout = if stacked {
        Stack::fill_column(gap)
    } else {
        Stack::fill_row(gap).align(AlignSpec::Stretch)
    };
    let shell = widget(
        Stack::fill_column(0.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Xl)
            .padding(if stacked { 18.0 } else { 22.0 })
            .min_height(LengthSpec::Px(0.0))
            .max_width(920.0)
            .painter(super::preview_paint::AudioCardFace),
    )
    .children((widget(
        layout
            .min_height(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                // Vue `.media-preview__audio-layout` 的 `overflow: hidden`：下限放不下时裁掉溢出，不把列撑出舞台。
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
/// 上下留白跟 Vue `syncLyricsInset`：视口量出来以后改成 `max(96, floor(高 / 2))`，
/// 上沿再减 1。当前行在这次布局里按 `centerActiveLyric` 滚到正中。
fn lyric_body(
    lines: &[super::super::inspect_lyrics::LyricLine],
    active: Option<usize>,
    pending: bool,
) -> AnyView {
    if lines.is_empty() {
        // Vue `lyricsPlaceholder`：`loading` 写「读取歌词...」，读完没有正文才写「暂无歌词」。
        let (label, key) = if pending {
            ("读取歌词...", "inspect-audio-lyrics-loading")
        } else {
            ("暂无歌词", "inspect-audio-lyrics-empty")
        };
        return widget(text(label, 14.0, 400, SemanticColorRole::Muted, 21.7))
            .key(key)
            .into_any();
    }
    let inset = signal(LYRIC_INSET_INITIAL);
    let rows = lines
        .iter()
        .enumerate()
        .map(|(index, line)| lyric_line(index, line, active))
        .collect::<Vec<_>>();
    let list = widget(
        Stack::column(LYRIC_GAP)
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
    .on_cx(move |_view, event: &ScrollViewportChanged, cx| {
        center_lyric_viewport(event, active, inset, cx)
    })
    .into_any()
}

/// 视口高度有了以后收成 Vue 的 inset，并按行号先滚一次。实测盒子由 [`follow_active_lyric`] 收口。
fn center_lyric_viewport(
    event: &ScrollViewportChanged,
    active: Option<usize>,
    inset: nana_ui::runtime::view::Signal<f32>,
    cx: &mut nana_ui::runtime::ViewContext<'_, ScrollView>,
) {
    if !event.height.is_finite() {
        eprintln!("Nana 歌词视口高度无效，当前行不滚：{}", event.height);
        return;
    }
    if event.height <= 0.0 {
        return;
    }
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

/// Vue `syncLyricsInset`：至少 96，否则是视口高度的一半，向下取整。
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
    fade_key: Option<u64>,
}

/// 当前行滚到视口正中，再按 Vue 的上下遮罩给每行套不透明度。滚或淡出变了就再画一帧。
pub(crate) fn follow_active_lyric(
    document: &mut RuntimeDocument,
    followed: &mut Option<LyricFollow>,
) -> bool {
    let moved = sync_lyric_center(document, followed);
    lyric_fade::apply(document, followed) || moved
}

/// 没有滚动区或当前行就不改偏移。视口高或当前行变了才滚。
fn sync_lyric_center(document: &mut RuntimeDocument, followed: &mut Option<LyricFollow>) -> bool {
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
            fade_key: None,
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
        fade_key: None,
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
/// Vue `audioRuntime.js`：`currentTime = timeMs / 1000`，按钮在 `timeMs == null` 时禁用。
/// 当前项还没装好时，播放器的跳转自己拒绝并记日志，不把会话标成失败。
fn lyric_line(
    index: usize,
    line: &super::super::inspect_lyrics::LyricLine,
    active: Option<usize>,
) -> AnyView {
    let (weight, mix) = lyric_tone(index, line.time_ms.is_none(), active);
    let key = if active == Some(index) {
        "inspect-audio-lyric-active".to_string()
    } else {
        format!("inspect-audio-lyric-{index}")
    };
    let Some(time_ms) = line.time_ms else {
        let mut line = text(
            &line.text,
            lyric_box::FONT,
            weight,
            SemanticColorRole::Text,
            lyric_box::LEADING,
        );
        paint_lyric_text(&mut line, mix);
        lyric_box::apply(std::sync::Arc::make_mut(&mut line.style.layout));
        return widget(line).key(key).into_any();
    };
    widget(lyric_button(&line.text, weight, mix))
        .key(key)
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Player(PlayerMessage::Seek(time_ms)));
        })
        .into_any()
}

/// 有时间的歌词行：居中、按淡法混字色，悬停和键盘焦点再亮一档。不铺按钮底，避免画成芯片。
fn lyric_button(label: &str, weight: u16, mix: Option<f32>) -> Button {
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
    layout.font_weight = Some(weight);
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
fn lyric_tone(index: usize, untimed: bool, active: Option<usize>) -> (u16, Option<f32>) {
    if active == Some(index) {
        (700, None)
    } else if !untimed && active.is_some_and(|current| index < current) {
        (400, Some(LYRIC_PASSED_MIX))
    } else {
        (400, Some(LYRIC_AHEAD_MIX))
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

/// 准备卡：读取或解码时居中。落影是 `0 12px 32px` 的背景色 70%，进度按 1.05 秒从 -120% 扫到 250%。
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
        Stack::column(10.0)
            .padding(14.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md)
            .max_width(360.0)
            .painter(super::preview_paint::LoadingCardShadow)
            .with_layout(|layout| {
                layout.width = Some(LengthSpec::CalcPercentOffset {
                    percent: 100.0,
                    offset_px: -32.0,
                });
            }),
    )
    .children((
        widget(text(&name, 13.0, 700, SemanticColorRole::Text, 20.15).truncating())
            .key("inspect-media-loading-name"),
        widget(text(detail, 12.0, 400, SemanticColorRole::Muted, 18.6).truncating())
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
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::{AudioMeta, audio_caption};
    use crate::shell::SearchRow;
    use crate::shell::ShellViewModel;
    use crate::shell::view_harness::ShellHarness;

    /// 音频舞台按 Vue `audioDisplayMetadata` 写标题、艺人和专辑。
    /// 文件列表里有这一条时用列表上的元数据，不用搜索命中覆盖；列表里没有时才用搜索命中。
    #[test]
    fn audio_stage_shows_metadata_title_artist_and_album() {
        let empty = audio_caption("track-01.mp3", "mp3", AudioMeta::Empty);
        assert_eq!(empty.title, "track-01");
        assert_eq!(empty.subtitle, "通用音频");
        assert!(empty.album.is_empty());

        let mut fields = BTreeMap::new();
        fields.insert("title".into(), json!("  "));
        fields.insert("artist".into(), json!("某人"));
        fields.insert("artists".into(), json!([" 周杰伦 ", " ", 1, "方文山"]));
        fields.insert("album".into(), json!(" 十一月的萧邦 "));
        let full = audio_caption("track-01.mp3", "mp3", AudioMeta::Map(&fields));
        assert_eq!(full.title, "track-01", "空白标题退回文件名");
        assert_eq!(
            full.subtitle, "周杰伦 / 方文山",
            "艺人数组里的空白和非字符串丢掉，也不再看 artist"
        );
        assert_eq!(full.album, "十一月的萧邦");

        fields.insert("title".into(), json!("夜曲"));
        fields.insert("artists".into(), json!([]));
        fields.remove("album");
        let ignored = audio_caption("track-01.mp3", "mp3", AudioMeta::Map(&fields));
        assert_eq!(ignored.title, "夜曲");
        assert_eq!(
            ignored.subtitle, "通用音频",
            "空的 artists 数组不退回 artist"
        );
        assert!(ignored.album.is_empty());

        let object = json!({"artist": " 桃 ", "album": "专辑"});
        let from_object = audio_caption(
            "song.mp3",
            "mp3",
            AudioMeta::Object(object.as_object().unwrap()),
        );
        assert_eq!(from_object.title, "song");
        assert_eq!(from_object.subtitle, "桃");
        assert_eq!(from_object.album, "专辑");

        let only_album = {
            let mut fields = BTreeMap::new();
            fields.insert("album".into(), Value::String("只有专辑".into()));
            fields
        };
        let album_only = audio_caption("track-01.mp3", "mp3", AudioMeta::Map(&only_album));
        assert_eq!(album_only.subtitle, "只有专辑");
        assert_eq!(album_only.album, "只有专辑");

        let mut model = scene("preview-audio");
        let path = model.inspect.target_path.clone().expect("音频预览路径");
        set_row(
            &mut model,
            &path,
            json!({
                "title": "夜曲",
                "artists": ["周杰伦", " ", 1, "方文山"],
                "artist": "不该出现",
                "album": "十一月的萧邦",
            }),
        );
        let shown = show(&model);
        assert_eq!(node_text(&shown, "inspect-audio-title"), "夜曲");
        assert_eq!(node_text(&shown, "inspect-audio-artist"), "周杰伦 / 方文山");
        assert_eq!(node_text(&shown, "inspect-audio-album"), "十一月的萧邦");
        assert!(shown.find("通用音频").is_none(), "有艺人时不写通用音频");

        set_row(
            &mut model,
            &path,
            json!({"artists": [], "artist": "某人", "album": "只有专辑"}),
        );
        let album = show(&model);
        assert_eq!(node_text(&album, "inspect-audio-title"), "track-01");
        assert_eq!(node_text(&album, "inspect-audio-artist"), "只有专辑");
        assert_eq!(node_text(&album, "inspect-audio-album"), "只有专辑");
        assert!(album.find("通用音频").is_none());

        set_row(&mut model, &path, json!({"artists": [], "artist": "某人"}));
        let mut hit = SearchRow {
            path: path.clone(),
            filename: "track-01.mp3".into(),
            ..SearchRow::default()
        };
        hit.metadata.insert("title".into(), json!("检索标题"));
        model.inspect.results = vec![hit];
        let listed = show(&model);
        assert_eq!(node_text(&listed, "inspect-audio-artist"), "通用音频");
        assert!(
            listed.keyed("inspect-audio-album").is_none(),
            "没有专辑不画专辑胶囊"
        );
        assert_ne!(
            node_text(&listed, "inspect-audio-title"),
            "检索标题",
            "文件还在列表里时不用搜索命中的标题"
        );

        drop_rows(&mut model, &path);
        let searched = show(&model);
        assert_eq!(
            node_text(&searched, "inspect-audio-title"),
            "检索标题",
            "列表里没有这一条时用搜索命中"
        );
    }

    /// 有真实图片时唱片中间用这张图；没有可读的图时仍是封面圆，不编造像素。
    /// `thumbnailPath` 优先于 `coverArt`。自定义缩略图落地后就是 `thumbnailPath`。只有网址、读失败或不是图片时不画。
    #[test]
    fn audio_stage_draws_real_cover_or_keeps_the_disc() {
        let dir = std::env::temp_dir().join(format!("momobako-audio-cover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("临时目录");
        let red = write_png(&dir, "red.png", [200, 20, 20, 255]);
        let blue = write_png(&dir, "blue.png", [20, 20, 200, 255]);
        let note = dir.join("note.txt");
        std::fs::write(&note, b"not an image").expect("写入非图片");
        let red_url = encoded(&red, [200, 20, 20, 255]);
        let blue_url = encoded(&blue, [20, 20, 200, 255]);

        let fields = BTreeMap::from([("coverArt".into(), json!(blue))]);
        assert_eq!(
            super::cover_file(Some(&red), AudioMeta::Map(&fields)),
            Some(red.as_str()),
            "thumbnailPath 优先"
        );
        assert_eq!(
            super::cover_file(Some("  "), AudioMeta::Map(&fields)),
            Some(blue.as_str()),
            "空白缩略图改用 coverArt"
        );
        assert_eq!(
            super::cover_file(Some("https://example.com/a.png"), AudioMeta::Map(&fields)),
            None,
            "网址不落地，也不改去画 coverArt"
        );
        assert_eq!(
            super::cover_file(None, AudioMeta::Map(&fields)),
            Some(blue.as_str())
        );

        let mut model = scene("preview-audio");
        let path = model.inspect.target_path.clone().expect("音频预览路径");
        let title = node_text(&show(&model), "inspect-audio-title");
        assert!(record_image(&show(&model)).is_none(), "没有图时不画封面");
        assert!(
            show(&model).keyed("inspect-audio-chip").is_some(),
            "没有图时唱片中间仍是音频"
        );

        set_row(&mut model, &path, json!({}));
        set_thumb(&mut model, &path, Some(&red), true);
        let custom = show(&model);
        assert_eq!(
            record_image(&custom).as_deref(),
            Some(red_url.as_str()),
            "自定义缩略图落地后画这张图"
        );
        assert!(
            custom.keyed("inspect-audio-chip").is_none(),
            "有图时不再写唱片中间的音频"
        );
        assert_eq!(
            node_text(&custom, "inspect-audio-title"),
            title,
            "封面不改标题"
        );

        set_row(&mut model, &path, json!({"coverArt": blue}));
        set_thumb(&mut model, &path, Some(&red), false);
        assert_eq!(
            record_image(&show(&model)).as_deref(),
            Some(red_url.as_str()),
            "有缩略图时不用 coverArt"
        );

        set_thumb(&mut model, &path, None, false);
        assert_eq!(
            record_image(&show(&model)).as_deref(),
            Some(blue_url.as_str()),
            "没有缩略图时画 coverArt 指向的文件"
        );

        set_thumb(
            &mut model,
            &path,
            Some("https://example.com/cover.png"),
            false,
        );
        let remote = show(&model);
        assert!(record_image(&remote).is_none(), "只有网址时不画，也不编造");
        assert!(remote.keyed("inspect-audio-chip").is_some());

        set_row(&mut model, &path, json!({}));
        set_thumb(
            &mut model,
            &path,
            Some(dir.join("missing.png").to_str().unwrap()),
            false,
        );
        assert!(
            record_image(&show(&model)).is_none(),
            "文件不在时保持封面圆"
        );
        set_thumb(&mut model, &path, Some(note.to_str().unwrap()), false);
        assert!(
            record_image(&show(&model)).is_none(),
            "不是图片时保持封面圆"
        );

        set_thumb(&mut model, &path, None, false);
        let mut hit = SearchRow {
            path: path.clone(),
            filename: "track-01.mp3".into(),
            ..SearchRow::default()
        };
        hit.metadata.insert("coverArt".into(), json!(blue));
        model.inspect.results = vec![hit];
        assert!(
            record_image(&show(&model)).is_none(),
            "文件还在列表里、这一条没有图时，不用搜索命中的封面"
        );
        drop_rows(&mut model, &path);
        assert_eq!(
            record_image(&show(&model)).as_deref(),
            Some(blue_url.as_str()),
            "列表里没有这一条时用搜索命中的 coverArt"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 有时间的歌词行点一下，播放头跳到这一行；没有时间的行点不了。暂停着的会话不被点成播放。
    /// 对齐 Vue `audioRuntime.js`：按钮把 `currentTime` 设成 `timeMs / 1000`，`timeMs == null` 时禁用。
    #[test]
    fn timed_lyric_click_seeks_playback_and_untimed_lines_do_not() {
        let mut model = scene("preview-audio");
        let duration = model.player.session.duration_ms.expect("音频预览已经装好");
        assert!(duration > 0, "没有时长就分不出点了哪一行");
        assert!(model.player.session.can_seek, "装好的音频要能跳转");
        assert_eq!(model.player.session.status, "paused");
        let target = (duration / 2).max(1).min(duration);
        assert_ne!(target, 0);
        let lrc = format!("[00:00.00]开头\n{}中段\n作词：桃", lrc_tag(target));
        model.inspect.lyrics = crate::shell::inspect_lyrics::parse_lrc(&lrc);
        let mut harness = show(&model);
        assert_eq!(node_text(&harness, "inspect-audio-lyric-active"), "开头");
        assert_eq!(
            role_of(&harness, "中段"),
            nana_ui::runtime::AccessibilityRole::Button,
            "有时间的行是按钮"
        );
        assert_ne!(
            role_of(&harness, "作词：桃"),
            nana_ui::runtime::AccessibilityRole::Button,
            "没有时间的行不是按钮"
        );

        harness.click_label("中段");
        assert_eq!(
            harness.model.player.session.current_time_ms, target,
            "播放头跳到点中的那一行"
        );
        assert_eq!(
            harness.model.player.session.status, "paused",
            "点歌词只跳转，不开始播放"
        );
        assert!(
            harness.model.player.session.error.is_none(),
            "跳转不应把会话标成失败"
        );
        assert_eq!(node_text(&harness, "inspect-audio-lyric-active"), "中段");

        harness.click_label("作词：桃");
        assert_eq!(
            harness.model.player.session.current_time_ms, target,
            "没有时间的行点不了"
        );
        assert_eq!(node_text(&harness, "inspect-audio-lyric-active"), "中段");
    }

    /// 歌词三档明暗：当前行不淡，已经唱过的略实，还没唱到和没有时间的行更淡。有时间的行悬停再亮。
    #[test]
    fn lyric_lines_stay_faint_until_the_playhead_reaches_them() {
        let mut model = scene("preview-audio");
        model.inspect.lyrics =
            crate::shell::inspect_lyrics::parse_lrc("[00:00.00]开头\n[00:00.50]后来\n作词：桃");
        place_playhead(&mut model, 500);
        let harness = show(&model);
        let mix = |label: &str, hover: bool| {
            let style = harness
                .document()
                .context()
                .world()
                .node_style(harness.node(label))?;
            if hover {
                style.interaction.hovered.foreground_mix
            } else {
                style.interaction.base.foreground_mix
            }
        };
        let ink = |ratio| {
            nana_ui_core::SemanticColorMix::new(
                nana_ui::runtime::SemanticColorRole::Text,
                nana_ui::runtime::SemanticColorRole::Background,
                ratio,
            )
        };
        assert_eq!(mix("后来", false), None, "当前行是完整正文");
        assert_eq!(
            mix("开头", false),
            Some(ink(super::LYRIC_PASSED_MIX)),
            "已经唱过的略实"
        );
        assert_eq!(
            mix("作词：桃", false),
            Some(ink(super::LYRIC_AHEAD_MIX)),
            "没有时间的行和还没唱到的一样淡"
        );
        assert_eq!(
            mix("开头", true),
            Some(ink(super::LYRIC_HOVER_MIX)),
            "有时间的行悬停再亮"
        );
        let align = harness
            .document()
            .context()
            .world()
            .node_style(harness.node("后来"))
            .map(|style| style.text_horizontal_alignment);
        assert_eq!(
            align,
            Some(nana_ui::runtime::TextHorizontalAlignment::Center)
        );
    }

    /// 当前行滚到歌词视口正中。没有歌词或还没到第一行时停在顶上，短到滚不动时也不跳走。
    #[test]
    fn active_lyric_centers_in_the_viewport() {
        assert_eq!(
            super::lyric_inset(100.0),
            96.0,
            "视口不到 192 时留白至少 96"
        );
        assert_eq!(super::lyric_inset(400.0), 200.0);
        assert_eq!(super::lyric_inset(401.0), 200.0, "一半向下取整");
        assert!(
            (super::lyric_scroll_top(0, 400.0, 200.0) - (super::LYRIC_LINE / 2.0 - 1.0)).abs()
                < 0.01
        );

        let bare = show(&scene("preview-audio"));
        assert!(
            bare.keyed("inspect-audio-lyric-scroll").is_none(),
            "没有歌词不建滚动区"
        );
        assert!(bare.keyed("inspect-audio-lyrics-empty").is_some());

        let mut waiting = scene("preview-audio");
        waiting.inspect.lyrics =
            crate::shell::inspect_lyrics::parse_lrc("[00:05.00]以后\n没有时间");
        let waiting = show(&waiting);
        assert!(
            waiting.keyed("inspect-audio-lyric-active").is_none(),
            "还没到第一行时没有当前行"
        );
        assert_eq!(lyric_scroll(&waiting), 0.0, "没有当前行不滚");

        let mut model = scene("preview-audio");
        let duration = model.player.session.duration_ms.expect("音频预览已经装好");
        let mut lines = (0..30)
            .map(|index| format!("{}第{index}行", lrc_tag(index as u64)))
            .collect::<Vec<_>>();
        lines.push("没有时间".into());
        model.inspect.lyrics = crate::shell::inspect_lyrics::parse_lrc(&lines.join("\n"));
        place_playhead(&mut model, 15);
        let mut harness = show(&model);
        assert_eq!(node_text(&harness, "inspect-audio-lyric-active"), "第15行");
        assert_eq!(
            node_text(&harness, "inspect-audio-lyric-30"),
            "没有时间",
            "没有时间的行不成当前行"
        );
        assert_centered(&harness);
        assert_vue_pad(&harness);
        let middle = lyric_scroll(&harness);
        assert!(
            middle > super::LYRIC_LINE,
            "中间那一行要真正滚开顶部，现在 {middle}"
        );

        click_lyric(&mut harness, "第17行");
        assert_eq!(
            harness.model.player.session.current_time_ms, 17,
            "点歌词仍按这一行的毫秒跳"
        );
        assert_eq!(harness.model.player.session.status, "paused");
        assert_eq!(node_text(&harness, "inspect-audio-lyric-active"), "第17行");
        assert_centered(&harness);
        assert!(
            lyric_scroll(&harness) > middle,
            "新的当前行更靠下，滚动也跟着下去"
        );

        let mut one = scene("preview-audio");
        one.inspect.lyrics = crate::shell::inspect_lyrics::parse_lrc("[00:00.00]只有一行");
        let one = show(&one);
        assert_centered(&one);
        let alone = lyric_scroll(&one);
        assert!(
            alone < super::LYRIC_LINE,
            "一行歌词只为留白挪一点点，不应跳走：{alone}"
        );
        assert!(duration > 17, "夹具要长过被点的那一行");
    }

    /// 把预览会话的播放头拨到 `ms`。歌词当前行读的是这份会话。
    fn place_playhead(model: &mut ShellViewModel, ms: u64) {
        let mut session = model.inspect.media_session().cloned().expect("预览会话");
        session.current_time_ms = ms;
        model.inspect.replace_shared_media(session);
    }

    fn lyric_scroll(harness: &ShellHarness) -> f32 {
        let id = harness
            .keyed("inspect-audio-lyric-scroll")
            .expect("歌词滚动");
        harness
            .document()
            .context()
            .world()
            .scroll_offset(id)
            .unwrap_or_default()
            .y
    }

    /// 当前行的布局盒中心落在视口中心；超出可滚范围时停在滚动区的两端。
    fn assert_centered(harness: &ShellHarness) {
        let scroll = harness
            .keyed("inspect-audio-lyric-scroll")
            .expect("歌词滚动");
        let active = harness.keyed("inspect-audio-lyric-active").expect("当前行");
        let world = harness.document().context().world();
        let item = world.layout_box(active).expect("当前行布局");
        let view = world.layout_box(scroll).expect("视口布局");
        let metrics = world.scroll_metrics(scroll).expect("滚动尺寸");
        let offset = world.scroll_offset(scroll).unwrap_or_default().y;
        let raw = item.y - view.y - view.height / 2.0 + item.height / 2.0;
        let expected = raw.max(metrics.origin_y).min(metrics.max_offset().y);
        assert!(
            (offset - expected).abs() < 1.0,
            "当前行应在视口正中：偏移 {offset}，期望 {expected}，行 {:?}，视口 {:?}",
            item,
            view
        );
    }

    /// 第一行的顶到滚动内容顶的距离是上留白 `inset - 1`。
    fn assert_vue_pad(harness: &ShellHarness) {
        let scroll = harness
            .keyed("inspect-audio-lyric-scroll")
            .expect("歌词滚动");
        let first = harness.keyed("inspect-audio-lyric-0").expect("第一行");
        let world = harness.document().context().world();
        let view = world.layout_box(scroll).expect("视口布局");
        let line = world.layout_box(first).expect("第一行布局");
        let inset = super::lyric_inset(view.height);
        let pad = (line.y - view.y) - (inset - 1.0);
        assert!(
            pad.abs() < 1.0,
            "上留白应是 {inset} - 1，差了 {pad}（视口高 {}）",
            view.height
        );
    }

    /// 点歌词行的视觉位置。布局盒不含滚动，得点在滚过以后的那一处。
    fn click_lyric(harness: &mut ShellHarness, label: &str) {
        let id = harness.node(label);
        let (x, y) = harness.center(id);
        let offset = lyric_scroll(harness);
        let messages = harness.click_messages(x, y - offset);
        harness.settle(messages);
    }

    /// `[分:秒.毫秒]`，毫秒取三位数，和 LRC 解析那一头对得上。
    fn lrc_tag(ms: u64) -> String {
        let minutes = ms / 60_000;
        let seconds = (ms % 60_000) / 1000;
        let fraction = ms % 1000;
        format!("[{minutes:02}:{seconds:02}.{fraction:03}]")
    }

    fn role_of(harness: &ShellHarness, label: &str) -> nana_ui::runtime::AccessibilityRole {
        let id = harness
            .find(label)
            .unwrap_or_else(|| panic!("没有 {label}"));
        harness
            .nodes()
            .into_iter()
            .find(|node| node.id == id)
            .unwrap_or_else(|| panic!("{label} 不在无障碍树里"))
            .role
    }

    fn scene(name: &str) -> ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(scene, _)| *scene == name)
            .unwrap_or_else(|| panic!("没有场景 {name}"))
            .1
    }

    fn set_row(model: &mut ShellViewModel, path: &str, metadata: Value) {
        let row = model
            .files
            .rows
            .iter_mut()
            .find(|row| row.path == path)
            .unwrap_or_else(|| panic!("列表里没有 {path}"));
        row.metadata = metadata
            .as_object()
            .expect("元数据对象")
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
    }

    fn drop_rows(model: &mut ShellViewModel, path: &str) {
        model.files.rows.retain(|row| row.path != path);
        model.files.virtual_rows.retain(|row| row.path != path);
        model.files.category_rows.retain(|row| row.path != path);
    }

    fn show(model: &ShellViewModel) -> ShellHarness {
        ShellHarness::mount(model.clone())
    }

    fn node_text(harness: &ShellHarness, key: &str) -> String {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .world()
            .text(id)
            .unwrap_or_default()
            .to_string()
    }

    /// 写成一张纯色 PNG。颜色要能在解码结果里对上，避免舞台拿别的像素充封面。
    fn write_png(dir: &std::path::Path, name: &str, color: [u8; 4]) -> String {
        let path = dir.join(name);
        image::RgbaImage::from_pixel(8, 8, image::Rgba(color))
            .save(&path)
            .unwrap_or_else(|error| panic!("写入 {name}：{error}"));
        path.to_string_lossy().into_owned()
    }

    /// 用和舞台同一套解码、编码，把文件收成 data URL，并核对像素就是写进去的那张。
    fn encoded(path: &str, color: [u8; 4]) -> String {
        let frame = crate::shell::decode_thumbnail_file(path)
            .unwrap_or_else(|error| panic!("测试图解不开：{error}"));
        assert_eq!(
            frame.rgba.len(),
            frame.width as usize * frame.height as usize * 4,
            "像素长度对不上"
        );
        assert_eq!(&frame.rgba[..4], &color, "封面左上角不是写进文件的颜色");
        crate::shell::inspect::rgba_png_data_url(frame.width, frame.height, &frame.rgba)
            .expect("测试图能编码")
    }

    fn set_thumb(model: &mut ShellViewModel, path: &str, thumb: Option<&str>, custom: bool) {
        let mut found = false;
        for row in model
            .files
            .rows
            .iter_mut()
            .chain(model.files.virtual_rows.iter_mut())
            .chain(model.files.category_rows.iter_mut())
        {
            if row.path == path {
                row.thumbnail_path = thumb.map(str::to_string);
                row.thumbnail_custom = custom;
                found = true;
            }
        }
        assert!(found, "列表里没有 {path}");
    }

    /// 唱片自绘里的封面图。没有 `Image` 命令就是没画图。
    fn record_image(harness: &ShellHarness) -> Option<String> {
        let id = harness.keyed("inspect-audio-record").expect("唱片");
        let style = harness
            .document()
            .context()
            .world()
            .node_style(id)
            .expect("唱片样式");
        let painter = style.painter.as_ref().expect("唱片自绘").painter();
        let theme = nana_ui::theme::builtin_theme_arc(nana_ui::theme::ThemeAppearance::Dark);
        let recording = nana_ui::runtime::PaintRecording::record(
            painter,
            theme.as_ref(),
            [200.0, 200.0],
            nana_ui::runtime::PaintState::default(),
        );
        let mut found = None;
        for op in recording
            .behind_children
            .iter()
            .chain(&recording.over_children)
        {
            if let nana_ui::runtime::PaintOp::Image {
                source, fit, rect, ..
            } = op
            {
                assert_eq!(*fit, nana_ui::runtime::ImageFit::Cover);
                let side = 200.0 * 0.58;
                assert!(
                    (rect.width - side).abs() < 0.01 && (rect.height - side).abs() < 0.01,
                    "封面应落在中间 58% 的圆里"
                );
                assert!(found.replace(source.to_string()).is_none(), "只画一张封面");
            }
        }
        found
    }
}
