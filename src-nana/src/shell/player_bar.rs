//! 底部播放条，对齐 Vue `WorkspacePlayerBar`。
//!
//! 卡片里自上而下是 8px 进度轨、「媒体 / 传输 / 时间与音量」三栏，以及图片、视频才有的
//! 停留时长和适应 / 填充设置。只有窗口不宽于 1120px（`shell.css` 的 `@media (max-width: 1120px)`）
//! 才把三栏拆成三行。窗口更宽、卡片却窄于一行时，仍是这一行：音量滑杆按列的下限伸出卡片，
//! 不把整条改成三行。当前队列浮在卡片右上方（`player_bar_parts.rs`）。文件页、预览页和播放集页共用这一块。

use nana_ui::Icon;
use nana_ui::runtime::view::{AnyView, IntoView, fields, widget};
use nana_ui::runtime::{
    Activate, AlignSpec, IconButton, JustifySpec, LengthSpec, RadiusTier, RangeField, RangeInput,
    SemanticColorRole, SizeChanged, Stack, Text,
};

use super::super::player::{PlaybackMode, PlayerMessage, QueueItem};
use super::super::{ShellMessage, ShellViewModel, hot};
use super::icons;
use super::paint::{GlyphButton, Invisible, ProgressTrack};

#[path = "player_bar_parts.rs"]
mod parts;
pub(super) use parts::hit_area;

/// 播放条要显示的全部状态。从壳层模型推导一次，视图只读这里。
pub(crate) struct BarProps {
    /// 当前条目。预览音视频时是插在当前项后面的临时条目。
    pub item: Option<QueueItem>,
    /// 当前播放器登记的文件类别：image、video、audio，没有播放器时为空。
    pub file_class: String,
    pub supports_seek: bool,
    pub supports_volume: bool,
    pub can_play: bool,
    /// 当前项的播放源准备失败过：「播放」可点，点了重新准备（上一首、下一首照旧不可点）。
    pub retry: bool,
    pub playing: bool,
    pub mode: PlaybackMode,
    pub current_ms: u64,
    pub duration_ms: u64,
    pub volume: f32,
    pub image_seconds: u32,
    pub cover: bool,
    pub error: Option<String>,
    pub queue_open: bool,
    pub queue: Vec<QueueItem>,
    /// 队列浮层滚动区的键：仓库和播放集，换播放集时列表回到顶部。
    pub queue_key: String,
    pub current_id: Option<String>,
    /// 窗口逻辑宽。音量滑杆宽是 `min(120px, 18vw)`。
    viewport_width: f32,
}

impl BarProps {
    pub(crate) fn from_model(model: &ShellViewModel) -> Self {
        let player = &model.player;
        let item = player.current_item().cloned();
        // 没有当前项时仍按正在放的播放集的播放器类型取类别，封面才写「音频」而不是「媒体」。
        let definition = player
            .bar_player_type_id()
            .and_then(|id| definition(model, id));
        let session = &player.session;
        let error = session
            .error
            .clone()
            .filter(|error| !error.trim().is_empty());
        Self {
            file_class: definition
                .as_ref()
                .map(|value| value.file_class.clone())
                .unwrap_or_default(),
            supports_seek: definition.as_ref().is_some_and(|value| value.supports_seek),
            supports_volume: definition
                .as_ref()
                .is_some_and(|value| value.supports_volume),
            can_play: player.can_play,
            retry: player.retry_pending(),
            // 图标跟实际状态走：失败的会话不显示暂停。
            playing: session.status == "playing",
            mode: player.mode,
            current_ms: session.current_time_ms,
            duration_ms: session.duration_ms.unwrap_or(0),
            volume: session.volume.clamp(0.0, 1.0),
            image_seconds: (player.settings.image_duration_ms as f32 / 1000.0).round() as u32,
            cover: player.settings.object_fit_cover,
            // Vue `WorkspacePlayerBar.vue` 有错误就写在次行，没有条目时也写。
            error,
            // 队列跟 `queueOpen` 走，不因为当前项没了就收起（Vue `v-if="queueOpen"`）。
            queue_open: player.queue_open,
            queue: player.queue.clone(),
            queue_key: format!(
                "player-queue-list-{}-{}",
                super::key_part(player.repo_id.as_deref().unwrap_or_default()),
                super::key_part(
                    player
                        .listed
                        .as_ref()
                        .map(|detail| detail.playlist.playlist_id.as_str())
                        .unwrap_or("transient")
                )
            ),
            current_id: player.current_id.clone(),
            viewport_width: model.viewport_width,
            item,
        }
    }

    /// 粗体标题：有条目时是「正在播放」加去掉扩展名的文件名。
    pub(crate) fn title(&self) -> String {
        match &self.item {
            Some(item) if !item.filename.trim().is_empty() => {
                format!(
                    "正在播放 {}",
                    display_title(&item.filename, &item.extension)
                )
            }
            _ => "未选择播放内容".into(),
        }
    }

    /// 次行：错误优先，其次是「播放器 · 路径」，没有条目时提示先选播放集。
    pub(crate) fn subtitle(&self) -> String {
        if let Some(error) = &self.error {
            return error.clone();
        }
        match &self.item {
            Some(item) if !item.player_label.trim().is_empty() => {
                format!("{} · {}", item.player_label, item.path)
            }
            Some(item) => item.path.clone(),
            None => "选择播放集后可开始播放".into(),
        }
    }

    /// 封面里的类型字，按播放器类别取。
    pub(crate) fn media_label(&self) -> &'static str {
        media_type_label(&self.file_class)
    }

    pub(crate) fn time_text(&self) -> String {
        clock_parts(self.current_ms, self.duration_ms).1
    }

    pub(crate) fn has_item(&self) -> bool {
        self.item.is_some()
    }

    /// 图片和视频才有停留时长或画面适配。
    pub(crate) fn shows_settings(&self) -> bool {
        self.has_item() && matches!(self.file_class.as_str(), "image" | "video")
    }
}

/// 播放器定义：插件登记的贡献优先，其次是内置候选。
struct Definition {
    file_class: String,
    supports_seek: bool,
    supports_volume: bool,
}

fn definition(model: &ShellViewModel, player_type_id: &str) -> Option<Definition> {
    if let Some(contribution) = model
        .player
        .contributions
        .iter()
        .find(|item| item.player_type_id == player_type_id)
    {
        return Some(Definition {
            file_class: contribution.file_class.clone(),
            supports_seek: contribution.supports_seek,
            supports_volume: contribution.supports_volume,
        });
    }
    model
        .player
        .candidates
        .iter()
        .find(|item| item.player_type_id == player_type_id)
        .map(|candidate| Definition {
            file_class: candidate.file_class.clone(),
            supports_seek: candidate.supports_seek,
            supports_volume: candidate.supports_volume,
        })
}

pub(crate) fn media_type_label(class: &str) -> &'static str {
    match class {
        "image" => "图片",
        "video" => "视频",
        "audio" => "音频",
        _ => "媒体",
    }
}

/// 去掉扩展名后的标题，和 Vue `displayNameWithoutExtension` 一致。
pub(crate) fn display_title(filename: &str, extension: &str) -> String {
    let name = filename.trim();
    let extension = extension.trim().trim_start_matches('.');
    if name.is_empty() || extension.is_empty() {
        return name.to_string();
    }
    let suffix = format!(".{extension}");
    if name.len() > suffix.len()
        && name
            .to_ascii_lowercase()
            .ends_with(&suffix.to_ascii_lowercase())
    {
        name[..name.len() - suffix.len()].to_string()
    } else {
        name.to_string()
    }
}

fn format_time(ms: u64) -> String {
    let total = ms / 1000;
    format!("{}:{:02}", total / 60, total % 60)
}

/// 随播放时钟变的三个值：进度轨比例、「当前 / 总长」和拖动条位置。没有时长时比例为 0，
/// 拖动条按 1 毫秒的总长夹住。挂载和逐帧同步共用这一份算法。
pub(crate) fn clock_parts(current_ms: u64, duration_ms: u64) -> (f32, String, f64) {
    let ratio = if duration_ms == 0 {
        0.0
    } else {
        (current_ms as f32 / duration_ms as f32).clamp(0.0, 1.0)
    };
    let time = format!("{} / {}", format_time(current_ms), format_time(duration_ms));
    (ratio, time, current_ms.min(duration_ms.max(1)) as f64)
}

/// 宽排版里媒体栏的下限，对照 `.workspace-player__body` 的 `minmax(220px, 1fr)`。
/// 卡片放不下一行时音量伸出卡片，不因此改成三行。
const MEDIA_MIN_WIDTH: f32 = 220.0;

/// 三行只跟 `shell.css` 的 `@media (max-width: 1120px)` 走。
///
/// 窗口更宽时没有容器查询。`.workspace-player__body` 仍是
/// `grid-template-columns: minmax(220px, 1fr) max-content minmax(170px, 0.72fr)`：
/// 卡片放不下一行时列停在下限，音量从卡片右边溢出去。媒体栏下限是 220px，
/// 1200×800 标准文件页（卡片约 558）也会把音量挤出右内边距，传输仍和媒体同一行。
fn narrow_layout(model: &ShellViewModel) -> bool {
    model.narrow_viewport()
}

/// 播放条。窗口不宽于 1120px 时按 [`narrow_layout`] 拆成三行。卡片是 `--radius-2xl`，跟着主题变。
/// 卡片不裁切：窗口仍宽、卡片放不下一行时，音量可以伸出右边，和 `.workspace-player` 一样。
pub(crate) fn player_bar(model: &ShellViewModel) -> AnyView {
    let props = BarProps::from_model(model);
    let narrow = narrow_layout(model);
    let mut rows: Vec<AnyView> = vec![progress(&props), body(&props, narrow)];
    if props.shows_settings() {
        rows.push(parts::settings_row(&props, narrow));
    }
    if props.queue_open {
        rows.push(parts::queue_popover(&props));
    }
    let card = Stack::column(10.0)
        .surface(SemanticColorRole::Surface)
        .outline(SemanticColorRole::BorderStrong, 1.0)
        .radius(RadiusTier::Xxl)
        .min_height(LengthSpec::Px(124.0))
        .with_layout(|layout| {
            layout.padding_top = Some(LengthSpec::Px(12.0));
            layout.padding_right = Some(LengthSpec::Px(16.0));
            layout.padding_bottom = Some(LengthSpec::Px(16.0));
            layout.padding_left = Some(LengthSpec::Px(16.0));
            layout.overflow_x = nana_ui_core::OverflowSpec::Visible;
            layout.overflow_y = nana_ui_core::OverflowSpec::Visible;
            layout.paint.box_shadows = vec![nana_ui_core::BoxShadowSpec {
                paint_color: None,
                offset_x: 0.0,
                offset_y: 14.0,
                blur_radius: 34.0,
                spread_radius: -24.0,
                color: [0.0, 0.0, 0.0, 0.45],
                inset: false,
            }];
        });
    widget(card)
        .children(rows)
        .key("player-card")
        .on_cx(|_, event: &SizeChanged, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::BarResized(event.width)))
        })
        .into_any()
}

/// 8px 进度轨。上面盖一层透明拖动区，和 Vue 的透明 range 一样高 22px。
fn progress(props: &BarProps) -> AnyView {
    let seekable = props.has_item() && props.duration_ms > 0 && props.supports_seek;
    let duration = props.duration_ms.max(1) as f64;
    let (ratio, _, position) = clock_parts(props.current_ms, props.duration_ms);
    let mut range = RangeField::new(position, 0.0, duration, 100.0)
        .label("播放进度")
        .show_label(false)
        .show_value(false)
        .disabled(!seekable);
    range.style.painter = Some(Invisible.into());
    range.style.background = None;
    range.style.border = None;
    let layout = std::sync::Arc::make_mut(&mut range.style.layout);
    layout.position = nana_ui_core::PositionSpec::Absolute;
    layout.offset_left = Some(LengthSpec::Px(0.0));
    layout.offset_top = Some(LengthSpec::Px(-7.0));
    layout.width = Some(LengthSpec::Percent(100.0));
    layout.height = Some(LengthSpec::Px(22.0));
    layout.min_height = Some(LengthSpec::Px(22.0));
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    // 进度轨比例和拖动条位置随播放时钟逐帧走，绑在热信号上，播放推进不重挂播放条。
    // Vue 进度条听的是 range 的 `input`：拖动时每一格都跳转，不等松手。
    // Nana 拖动中的取值是 `RangeInput`；`RangeChanged` 要等松手才提交。键盘步进也会先发出 `RangeInput`。
    let seek = widget(range)
        .prop::<f64, fields::slider::value>(hot::prop(|signals| signals.position, position))
        .key("player-seek")
        .on_cx(|_, event: &RangeInput, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::Seek(
                event.value.max(0.0) as u64
            )));
        });
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Px(8.0))
            .min_height(LengthSpec::Px(8.0))
            .painter(ProgressTrack { ratio }),
    )
    .prop::<f32, hot::TrackRatioField>(hot::prop(|signals| signals.progress, ratio))
    .children((seek,))
    .key("player-progress")
    .into_any()
}

/// 三栏：媒体、传输、时间和音量。窗口不宽于 1120px 时拆成三行，传输居中，时间和音量两端对齐。
fn body(props: &BarProps, narrow: bool) -> AnyView {
    let media = media(props);
    let transport = transport(props);
    if narrow {
        let side = widget(
            Stack::bar(10.0)
                .justify(JustifySpec::SpaceBetween)
                .align(AlignSpec::Center),
        )
        .children((time(props), volume(props)))
        .key("player-side")
        .into_any();
        return widget(Stack::column(18.0))
            .children((
                media,
                widget(Stack::bar(0.0).justify(JustifySpec::Center))
                    .children((transport,))
                    .into_any(),
                side,
            ))
            .key("player-body")
            .into_any();
    }
    // 宽排版对照 `.workspace-player__body`：媒体 1fr（至少 220）、传输 max-content、
    // 时间音量 0.72fr（至少 170）。卡片比这些下限还窄时列不再缩短，音量从右边挤出卡片。
    let media = widget(
        Stack::row(0.0)
            .grow(1.0)
            .shrink(1.0)
            .min_width(LengthSpec::Px(MEDIA_MIN_WIDTH))
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((media,))
    .into_any();
    let side = widget(
        Stack::row(10.0)
            .justify(JustifySpec::End)
            .align(AlignSpec::Center)
            .grow(0.72)
            .shrink(1.0)
            .min_width(LengthSpec::Px(170.0))
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((time(props), volume(props)))
    .key("player-side")
    .into_any();
    widget(
        Stack::bar(18.0)
            .align(AlignSpec::Center)
            .min_height(LengthSpec::Px(72.0))
            .with_layout(|layout| {
                layout.overflow_x = nana_ui_core::OverflowSpec::Visible;
                layout.overflow_y = nana_ui_core::OverflowSpec::Visible;
            }),
    )
    .children((media, transport, side))
    .key("player-body")
    .into_any()
}

/// 封面和两行字是同一个可点区域：点它打开当前条目的预览。没有条目时整块变淡、不可点。
fn media(props: &BarProps) -> AnyView {
    let title = props.title();
    // 没有条目时整块按 Vue 的 0.45 淡化，在卡片底色上混出来。
    let dim = (!props.has_item()).then_some((
        SemanticColorRole::Surface,
        crate::theme_map::DISABLED_OPACITY,
    ));
    let hit = widget(parts::hit_area(&title, !props.has_item(), 0.0, 6.0))
        .key("player-media")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::OpenPreview { item_id: None }))
        });
    // 两行字的宽度来自剩下的空间（基准 0、可伸缩），超出时省略号收尾，和 Vue 的 nowrap + ellipsis 一致。
    let meta = widget(
        Stack::column(3.0)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0)
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((
        widget(parts::dim_text(
            parts::line(title, 14.0, 700, SemanticColorRole::Text, 21.7),
            dim,
        ))
        .key("player-title"),
        widget(parts::dim_text(
            parts::line(
                props.subtitle(),
                11.667,
                500,
                SemanticColorRole::Muted,
                18.08,
            ),
            dim,
        ))
        .key("player-subtitle"),
    ));
    widget(
        Stack::row(12.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(58.0)),
    )
    .children((hit, cover(props, dim), meta))
    .key("player-media-row")
    .into_any()
}

/// 58px 封面，对应 Vue `.workspace-player__thumb` 和 `resolveThumbnailSrc(item.thumbnailPath)`。
///
/// 当前条目的路径指向可读本地图片时按 `object-fit: cover` 画进去。路径空、只是 http(s)、
/// 读失败、不是图片或编不成图时仍写类型字。不下载，也不等文件列表把纹理传上来。
/// 没有条目时类型字随整行按 0.45 淡一次；有图时这一格不另淡，点击穿透，仍打开预览。
fn cover(props: &BarProps, dim: Option<(SemanticColorRole, f32)>) -> AnyView {
    let path = props
        .item
        .as_ref()
        .and_then(|item| item.thumbnail_path.as_deref());
    if let Some(view) = parts::player_still(path, 58.0, "player-cover".into()) {
        return view;
    }
    parts::type_cover(props.media_label(), 58.0, "player-cover", dim)
}

/// 循环模式、上一首、播放 / 暂停、下一首、当前队列。
///
/// 提示照 Vue：只有循环模式写了 `title`（当前模式名）。Vue 另外四个按钮只有 `aria-hidden` 的图标，读屏
/// 读不出名字；这里都给无障碍名，不加提示。
fn transport(props: &BarProps) -> AnyView {
    let has_item = props.has_item();
    let skip_disabled = !has_item || !props.can_play;
    let (mode_icon, mode_label) = match props.mode {
        PlaybackMode::Shuffle => (icons::SHUFFLE, "随机播放"),
        PlaybackMode::SingleLoop => (icons::REPEAT_1, "单曲循环"),
        PlaybackMode::ListLoop => (icons::REPEAT, "列表循环"),
    };
    let playing = props.playing;
    let (play_icon, play_label) = if playing {
        (icons::PAUSE, "暂停")
    } else {
        (icons::PLAY, "播放")
    };
    widget(
        Stack::row(6.0)
            .grow(0.0)
            .shrink(0.0)
            .align(AlignSpec::Center),
    )
    .children((
        widget(glyph_button(mode_icon, mode_label, 15.0, 32.0, !has_item).with_tooltip(mode_label))
            .key("player-cycle-mode")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(player_message(PlayerMessage::CycleMode))
            }),
        widget(glyph_button(
            icons::SKIP_BACK,
            "上一首",
            16.0,
            32.0,
            skip_disabled,
        ))
        .key("player-previous")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::PlayPrevious))
        }),
        widget(glyph_button(
            play_icon,
            play_label,
            17.0,
            36.0,
            skip_disabled && !props.retry,
        ))
        .key("player-play")
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::SetPlaying(!playing)))
        }),
        widget(glyph_button(
            icons::SKIP_FORWARD,
            "下一首",
            16.0,
            32.0,
            skip_disabled,
        ))
        .key("player-next")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::PlayNext {
                natural_end: false,
            }));
        }),
        widget(glyph_button(
            icons::LIST_MUSIC,
            "当前队列",
            16.0,
            32.0,
            !has_item,
        ))
        .key("player-queue")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::ToggleQueue))
        }),
    ))
    .key("player-transport")
    .into_any()
}

/// 只有图标的传输按钮（Vue `.ui-icon-button`）：`label` 是无障碍名，图标按 Vue 的像素大小画，颜色是正文色，
/// 外框 `box_size` 见方。
///
/// 外观全由 [`GlyphButton`] 画：它不调 `draw_default`，`IconButton` 的内建图标、悬停底和焦点底都不画；
/// 停用时它按 `PaintState::disabled` 照 Vue 的 0.45 在 sRGB 里和卡片底色混。颜色照写的来
/// （`colors_from_style`），NanaUI 不再把停用的按钮整枚淡化，只淡自绘这一次。
pub(crate) fn glyph_button(
    icon: Icon,
    label: &str,
    icon_size: f32,
    box_size: f32,
    disabled: bool,
) -> IconButton {
    let mut button = IconButton::new(icon, label)
        .colors_from_style()
        .disabled(disabled);
    button.style.foreground = Some(SemanticColorRole::Text);
    button.style.background = None;
    button.style.control_padding_x = None;
    button.style.painter = Some(
        GlyphButton {
            icon,
            size: icon_size,
            color: SemanticColorRole::Text,
        }
        .into(),
    );
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(box_size));
    layout.height = Some(LengthSpec::Px(box_size));
    layout.min_width = Some(LengthSpec::Px(box_size));
    layout.min_height = Some(LengthSpec::Px(box_size));
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    button
}

/// 12px 弱色时间。窄列里可以在「/」后面折行。
///
/// 数字用等宽字形，对应 `.workspace-player__time { font-variant-numeric: tabular-nums }`。
/// 没有在播曲目时仍是会话里的 `0:00 / 0:00`，不另写进度。
fn time(props: &BarProps) -> AnyView {
    let text = props.time_text();
    let mut node = Text::new(text.clone())
        .color(SemanticColorRole::Muted)
        .font_size(12.0)
        .line_height(18.6)
        .with_min_width_zero();
    // `tnum` 是 tabular-nums 的 OpenType 特性，分秒进位时字宽不跳。
    std::sync::Arc::make_mut(&mut node.style.layout).font_features =
        Some(vec![nana_ui_core::FontFeatureSetting::new(*b"tnum", 1)]);
    widget(node)
        .prop::<String, fields::text::value>(hot::prop(|signals| signals.time, text))
        .key("player-time")
        .into_any()
}

/// 音量滑杆的首选宽度：`.workspace-player__volume input { width: min(120px, 18vw); min-width: 72px }`。
/// 窗口宽过约 667px 时仍是 120；再窄才按 18vw 缩短，最短 72。没有曲目时滑杆照样在，用会话里的音量。
pub(super) fn volume_slider_width(viewport_width: f32) -> f32 {
    const PREFERRED: f32 = 120.0;
    const FLOOR: f32 = 72.0;
    let from_viewport = if viewport_width.is_finite() && viewport_width > 0.0 {
        viewport_width * 0.18
    } else {
        PREFERRED
    };
    from_viewport.min(PREFERRED).max(FLOOR)
}

/// 音量图标和滑杆。滑杆宽见 [`volume_slider_width`]，压缩时最窄 72px；没有条目或播放器不支持音量时变淡。
fn volume(props: &BarProps) -> AnyView {
    let disabled = !props.has_item() || !props.supports_volume;
    let width = volume_slider_width(props.viewport_width);
    let slider = parts::slider(
        f64::from(props.volume),
        0.0,
        1.0,
        0.01,
        "音量",
        disabled,
        width,
        72.0,
    );
    widget(
        Stack::row(8.0)
            .align(AlignSpec::Center)
            .shrink(1.0)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children((
        widget(parts::icon(icons::VOLUME_2, 15.0, SemanticColorRole::Muted)),
        // Vue 音量 range 听 `@input`：拖动中每一格都改音量。只听 `RangeInput`。
        // 松手的 `RangeChanged` 是同一格的提交，再听一次会把这次拖动写两遍。键盘步进也会先发出 `RangeInput`。
        widget(slider)
            .key("player-volume")
            .on_cx(|_, event: &RangeInput, cx| {
                cx.dispatch_program_all(player_message(PlayerMessage::SetVolume(
                    event.value.clamp(0.0, 1.0) as f32,
                )));
            }),
    ))
    .key("player-volume-row")
    .into_any()
}

fn player_message(message: PlayerMessage) -> ShellMessage {
    ShellMessage::Player(message)
}

/// `Text` 默认最小宽度是内容宽，三栏压缩时要能缩到 0 再折行。
trait MinWidthZero {
    fn with_min_width_zero(self) -> Self;
}

impl MinWidthZero for Text {
    fn with_min_width_zero(mut self) -> Self {
        let layout = std::sync::Arc::make_mut(&mut self.style.layout);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.flex_shrink = Some(1.0);
        self
    }
}

#[cfg(test)]
pub(crate) fn props_for_test(model: &ShellViewModel) -> BarProps {
    BarProps::from_model(model)
}

#[cfg(test)]
mod volume_width_tests {
    use super::volume_slider_width;

    /// 空会话也画这条滑杆。窗口不窄于 `120 / 0.18` 时仍是 120；再窄按 18vw，最短 72。
    #[test]
    fn volume_slider_shrinks_with_the_viewport_down_to_72() {
        assert_eq!(volume_slider_width(1200.0), 120.0);
        assert_eq!(volume_slider_width(1120.0), 120.0);
        assert!((volume_slider_width(600.0) - 108.0).abs() < 0.01);
        assert_eq!(volume_slider_width(300.0), 72.0);
        assert_eq!(volume_slider_width(f32::NAN), 120.0);
    }
}

#[cfg(test)]
mod narrow_card_layout_tests {
    use crate::shell::ShellViewModel;
    use crate::shell::view_harness::ShellHarness;

    fn scene(name: &str) -> ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(scene, _)| *scene == name)
            .unwrap_or_else(|| panic!("没有场景 {name}"))
            .1
    }

    fn settle(harness: &mut ShellHarness) {
        for _ in 0..6 {
            harness.flush();
            for message in harness.take_messages() {
                harness.apply(message);
            }
        }
        harness.flush();
    }

    fn bounds(harness: &ShellHarness, key: &str) -> (f32, f32, f32, f32) {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        let bounds = harness
            .document()
            .context()
            .world()
            .layout_box(id)
            .unwrap_or_else(|| panic!("{key} 没有布局盒"));
        (bounds.x, bounds.y, bounds.width, bounds.height)
    }

    fn centre_y((_, y, _, height): (f32, f32, f32, f32)) -> f32 {
        y + height / 2.0
    }

    /// 窗口宽于 1120、卡片窄于一行：仍是一行，音量越过卡片右内边距。
    /// `shell.css` 只有 `@media (max-width: 1120px)` 才把 `.workspace-player__body` 改成一列。
    #[test]
    fn a_wide_window_with_a_narrow_card_pushes_the_volume_out() {
        let mut harness = ShellHarness::mount_at(scene("files-multi-select"), 1180.0, 800.0);
        settle(&mut harness);
        assert!(harness.model.viewport_width > 1120.0);
        let width = harness.model.player.bar_width;
        assert!(
            width > 0.0 && width < 554.0,
            "卡片宽 {width}，这一档才该把音量挤出去"
        );
        let card = bounds(&harness, "file-player-slot");
        let media = bounds(&harness, "player-media");
        let play = bounds(&harness, "player-play");
        let volume = bounds(&harness, "player-volume");
        assert!(
            (centre_y(media) - centre_y(play)).abs() < 8.0,
            "传输应和媒体同一行：媒体 {media:?} 播放 {play:?}"
        );
        let volume_right = volume.0 + volume.2;
        let inner_right = card.0 + card.2 - 16.0;
        assert!(
            volume_right > inner_right,
            "音量应挤出卡片右内边距：音量 {volume:?} 卡片 {card:?}"
        );
    }

    /// 1200 宽的标准文件页卡片约 558：仍是一行，音量越过卡片右内边距。
    /// 媒体栏下限 220 时，这一档的列下限之和已经超过卡片内容宽。
    #[test]
    fn the_standard_files_card_pushes_volume_past_the_padding() {
        let mut harness = ShellHarness::mount_at(scene("files-multi-select"), 1200.0, 800.0);
        settle(&mut harness);
        assert!(
            (harness.model.player.bar_width - 558.0).abs() < 1.0,
            "卡片宽 {}",
            harness.model.player.bar_width
        );
        let card = bounds(&harness, "file-player-slot");
        let media = bounds(&harness, "player-media");
        let play = bounds(&harness, "player-play");
        let volume = bounds(&harness, "player-volume");
        assert!(
            (centre_y(media) - centre_y(play)).abs() < 8.0,
            "标准文件页应留在一行：媒体 {media:?} 播放 {play:?}"
        );
        let volume_right = volume.0 + volume.2;
        let inner_right = card.0 + card.2 - 16.0;
        assert!(
            volume_right > inner_right,
            "音量应挤出卡片右内边距：音量 {volume:?} 卡片 {card:?}"
        );
    }

    /// `@media (max-width: 1120px)` 仍是三行，传输在媒体下面。
    #[test]
    fn the_viewport_media_query_still_stacks_three_rows() {
        let mut harness = ShellHarness::mount_at(scene("files-multi-select"), 1120.0, 800.0);
        settle(&mut harness);
        let media = bounds(&harness, "player-media");
        let play = bounds(&harness, "player-play");
        assert!(
            play.1 > media.1 + media.3 - 1.0,
            "1120 宽应是三行：媒体 {media:?} 播放 {play:?}"
        );
    }
}

#[cfg(test)]
mod seek_drag_tests {
    use nana_ui::InputModifiers;

    use super::super::super::player::{PlayerMessage, QueueItem};
    use crate::shell::ShellMessage;
    use crate::shell::view_harness::ShellHarness;

    fn scene() -> crate::shell::ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(name, _)| *name == "files-multi-select")
            .expect("没有 files-multi-select")
            .1
    }

    fn settle(harness: &mut ShellHarness) {
        for _ in 0..6 {
            harness.flush();
            for message in harness.take_messages() {
                harness.apply(message);
            }
        }
        harness.flush();
    }

    fn bounds(harness: &ShellHarness, key: &str) -> (f32, f32, f32, f32) {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        let box_ = harness
            .document()
            .context()
            .world()
            .layout_box(id)
            .unwrap_or_else(|| panic!("{key} 没有布局盒"));
        (box_.x, box_.y, box_.width, box_.height)
    }

    fn seek_positions(messages: &[ShellMessage]) -> Vec<u64> {
        messages
            .iter()
            .filter_map(|message| match message {
                ShellMessage::Player(PlayerMessage::Seek(position_ms)) => Some(*position_ms),
                _ => None,
            })
            .collect()
    }

    /// 拖进度条还没松手就要跳转。Vue `WorkspacePlayerBar` 的进度 range 听 `@input`，拖动中每一格都 seek。
    #[test]
    fn dragging_the_seek_bar_seeks_before_release() {
        let mut harness = ShellHarness::mount(scene());
        settle(&mut harness);
        harness.model.player.queue = vec![QueueItem {
            id: "tone".into(),
            playlist_id: "list".into(),
            asset_id: "asset".into(),
            path: "music/tone.wav".into(),
            filename: "tone.wav".into(),
            extension: "wav".into(),
            status: "ready".into(),
            status_reason: None,
            transient: true,
            player_type_id: "momobako.playlist.wav".into(),
            player_label: "WAV".into(),
            file_class: "audio".into(),
            thumbnail_path: None,
            is_virtual: false,
            local_absolute_path: None,
        }];
        harness.model.player.current_id = Some("tone".into());
        harness.model.player.session.duration_ms = Some(10_000);
        harness.model.player.session.status = "paused".into();
        harness.sync();
        harness.flush();

        let seek = bounds(&harness, "player-seek");
        assert!(seek.2 > 40.0, "进度条宽 {}", seek.2);
        let y = seek.1 + seek.3 / 2.0;
        let plain = InputModifiers::default();
        harness.pointer(nana_ui::PointerPhase::Down, seek.0 + 4.0, y, plain);
        harness.pointer(nana_ui::PointerPhase::Move, seek.0 + seek.2 - 4.0, y, plain);
        let during = seek_positions(&harness.take_messages());
        assert!(
            during.iter().any(|position_ms| *position_ms > 0),
            "拖动中应跳转，实际 {during:?}"
        );
        harness.pointer(nana_ui::PointerPhase::Up, seek.0 + seek.2 - 4.0, y, plain);
    }

    fn volume_levels(messages: &[ShellMessage]) -> Vec<f32> {
        messages
            .iter()
            .filter_map(|message| match message {
                ShellMessage::Player(PlayerMessage::SetVolume(volume)) => Some(*volume),
                _ => None,
            })
            .collect()
    }

    /// 拖音量滑杆还没松手就要写入。Vue 音量 range 听 `@input`。松手不再把同一格写第二次。
    #[test]
    fn dragging_the_volume_slider_sets_volume_before_release() {
        // 1200 宽时音量滑杆伸出文件卡片，右端盖在详情上，点不到。放宽窗口让整条滑杆留在卡片里。
        let mut harness = ShellHarness::mount_at(scene(), 1800.0, 800.0);
        settle(&mut harness);
        harness.model.player.queue = vec![QueueItem {
            id: "tone".into(),
            playlist_id: "list".into(),
            asset_id: "asset".into(),
            path: "music/tone.wav".into(),
            filename: "tone.wav".into(),
            extension: "wav".into(),
            status: "ready".into(),
            status_reason: None,
            transient: true,
            player_type_id: "momobako.playlist.wav".into(),
            player_label: "WAV".into(),
            file_class: "audio".into(),
            thumbnail_path: None,
            is_virtual: false,
            local_absolute_path: None,
        }];
        harness.model.player.current_id = Some("tone".into());
        harness.model.player.session.volume = 1.0;
        harness.model.player.session.status = "paused".into();
        harness.sync();
        harness.flush();

        let slider = bounds(&harness, "player-volume");
        let card = bounds(&harness, "file-player-slot");
        assert!(slider.2 > 40.0, "音量滑杆宽 {}", slider.2);
        assert!(
            slider.0 >= card.0 && slider.0 + slider.2 <= card.0 + card.2 + 1.0,
            "音量应整段落在卡片里：滑杆 {slider:?} 卡片 {card:?}"
        );
        let y = slider.1 + slider.3 / 2.0;
        let plain = InputModifiers::default();
        harness.pointer(
            nana_ui::PointerPhase::Down,
            slider.0 + slider.2 - 6.0,
            y,
            plain,
        );
        harness.pointer(nana_ui::PointerPhase::Move, slider.0 + 6.0, y, plain);
        let during = volume_levels(&harness.take_messages());
        let last = *during
            .last()
            .unwrap_or_else(|| panic!("拖动中应写入音量，实际 {during:?}"));
        assert!(last < 0.99, "应从满音量往下调，实际 {during:?}");
        harness.pointer(nana_ui::PointerPhase::Up, slider.0 + 6.0, y, plain);
        let released = volume_levels(&harness.take_messages());
        assert!(
            released
                .iter()
                .all(|volume| (*volume - last).abs() > 0.0001),
            "松手不应再写一次 {last}，实际 {released:?}"
        );
    }

    fn duration_ms(messages: &[ShellMessage]) -> Vec<u32> {
        messages
            .iter()
            .filter_map(|message| match message {
                ShellMessage::Player(PlayerMessage::SetImageDuration(ms)) => Some(*ms),
                _ => None,
            })
            .collect()
    }

    /// 拖图片停留时长还没松手就要写入。Vue 停留 range 听 `@input`。松手不再把同一格写第二次。
    #[test]
    fn dragging_the_duration_slider_sets_duration_before_release() {
        let mut harness = ShellHarness::mount_at(scene(), 1800.0, 800.0);
        settle(&mut harness);
        harness.model.player.contributions.push(
            crate::backend::services::repository::PlaylistPlayerContribution {
                player_type_id: "momobako.playlist.image-slideshow".into(),
                label: "图片幻灯片".into(),
                file_class: "image".into(),
                supported_extensions: vec!["png".into()],
                supports_seek: false,
                supports_volume: false,
                supports_preview_navigation: true,
                description: None,
            },
        );
        harness.model.player.queue = vec![QueueItem {
            id: "still".into(),
            playlist_id: "list".into(),
            asset_id: "asset".into(),
            path: "media/cover.png".into(),
            filename: "cover.png".into(),
            extension: "png".into(),
            status: "ready".into(),
            status_reason: None,
            transient: true,
            player_type_id: "momobako.playlist.image-slideshow".into(),
            player_label: "图片幻灯片".into(),
            file_class: "image".into(),
            thumbnail_path: None,
            is_virtual: false,
            local_absolute_path: None,
        }];
        harness.model.player.current_id = Some("still".into());
        harness.model.player.settings.image_duration_ms = 2_000;
        harness.sync();
        harness.flush();

        let slider = bounds(&harness, "player-duration");
        let card = bounds(&harness, "file-player-slot");
        assert!(slider.2 > 40.0, "停留时长滑杆宽 {}", slider.2);
        assert!(
            slider.0 >= card.0 && slider.0 + slider.2 <= card.0 + card.2 + 1.0,
            "停留时长应落在卡片里：滑杆 {slider:?} 卡片 {card:?}"
        );
        let y = slider.1 + slider.3 / 2.0;
        let plain = InputModifiers::default();
        harness.pointer(
            nana_ui::PointerPhase::Down,
            slider.0 + slider.2 - 6.0,
            y,
            plain,
        );
        let during = duration_ms(&harness.take_messages());
        let last = *during
            .last()
            .unwrap_or_else(|| panic!("拖动中应写入停留时长，实际 {during:?}"));
        assert!(last > 2_000, "应从 2 秒往上调，实际 {during:?}");
        harness.pointer(
            nana_ui::PointerPhase::Up,
            slider.0 + slider.2 - 6.0,
            y,
            plain,
        );
        let released = duration_ms(&harness.take_messages());
        assert!(
            released.iter().all(|ms| *ms != last),
            "松手不应再写一次 {last}，实际 {released:?}"
        );
    }
}
