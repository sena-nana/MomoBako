//! 底部播放条，对齐 Vue `WorkspacePlayerBar`。
//!
//! 卡片里自上而下是 8px 进度轨、「媒体 / 传输 / 时间与音量」三栏，以及图片、视频才有的
//! 停留时长和适应 / 填充设置。窗口不宽于 1120px 时三栏拆成三行。当前队列浮在卡片右上方。
//! 文件页、预览页和播放集页共用这一块。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, IconButton, JustifySpec, LengthSpec, RangeChanged, RangeField, SemanticColorRole, Stack, Text,
    Thumbnail,
};
use nana_ui::{ContentFit, Icon};

use super::super::player::{PlaybackMode, PlayerMessage, QueueItem};
use super::super::{ShellMessage, ShellViewModel};
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
    /// 已经上传纹理的缩略图槽位路径。
    pub thumbnail: Option<String>,
}

impl BarProps {
    pub(crate) fn from_model(model: &ShellViewModel) -> Self {
        let player = &model.player;
        let item = player.current_item().cloned();
        let definition = item.as_ref().and_then(|item| definition(model, &item.player_type_id));
        let session = &player.session;
        let error = session.error.clone().filter(|error| !error.trim().is_empty());
        Self {
            file_class: definition.as_ref().map(|value| value.file_class.clone()).unwrap_or_default(),
            supports_seek: definition.as_ref().is_some_and(|value| value.supports_seek),
            supports_volume: definition.as_ref().is_some_and(|value| value.supports_volume),
            can_play: player.can_play,
            // 图标跟实际状态走：失败的会话不显示暂停。
            playing: session.status == "playing",
            mode: player.mode,
            current_ms: session.current_time_ms,
            duration_ms: session.duration_ms.unwrap_or(0),
            volume: session.volume.clamp(0.0, 1.0),
            image_seconds: (player.settings.image_duration_ms as f32 / 1000.0).round() as u32,
            cover: player.settings.object_fit_cover,
            error: error.filter(|_| item.is_some()),
            queue_open: player.queue_open && item.is_some(),
            queue: player.queue.clone(),
            queue_key: format!(
                "player-queue-list-{}-{}",
                super::key_part(player.repo_id.as_deref().unwrap_or_default()),
                super::key_part(player.listed.as_ref().map(|detail| detail.playlist.playlist_id.as_str()).unwrap_or("transient"))
            ),
            current_id: player.current_id.clone(),
            thumbnail: thumbnail_slot(model, item.as_ref()),
            item,
        }
    }

    /// 粗体标题：有条目时是「正在播放」加去掉扩展名的文件名。
    pub(crate) fn title(&self) -> String {
        match &self.item {
            Some(item) if !item.filename.trim().is_empty() => {
                format!("正在播放 {}", display_title(&item.filename, &item.extension))
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
            Some(item) if !item.player_label.trim().is_empty() => format!("{} · {}", item.player_label, item.path),
            Some(item) => item.path.clone(),
            None => "选择播放集后可开始播放".into(),
        }
    }

    /// 封面里的类型字，按播放器类别取。
    pub(crate) fn media_label(&self) -> &'static str {
        media_type_label(&self.file_class)
    }

    pub(crate) fn time_text(&self) -> String {
        format!("{} / {}", format_time(self.current_ms), format_time(self.duration_ms))
    }

    pub(crate) fn progress_ratio(&self) -> f32 {
        if self.duration_ms == 0 {
            return 0.0;
        }
        (self.current_ms as f32 / self.duration_ms as f32).clamp(0.0, 1.0)
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
    if let Some(contribution) = model.player.contributions.iter().find(|item| item.player_type_id == player_type_id) {
        return Some(Definition {
            file_class: contribution.file_class.clone(),
            supports_seek: contribution.supports_seek,
            supports_volume: contribution.supports_volume,
        });
    }
    model.player.candidates.iter().find(|item| item.player_type_id == player_type_id).map(|candidate| Definition {
        file_class: candidate.file_class.clone(),
        supports_seek: candidate.supports_seek,
        supports_volume: candidate.supports_volume,
    })
}

/// 当前条目有已上传的缩略图时返回它的路径。没有纹理就画类型字，不拿文件名冒充画面。
fn thumbnail_slot(model: &ShellViewModel, item: Option<&QueueItem>) -> Option<String> {
    let path = item.and_then(|item| item.thumbnail_path.clone())?;
    let ready = model.files.rows.iter().any(|row| row.texture_ready && row.thumbnail_path.as_deref() == Some(path.as_str()));
    ready.then_some(path)
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
    if name.len() > suffix.len() && name.to_ascii_lowercase().ends_with(&suffix.to_ascii_lowercase()) {
        name[..name.len() - suffix.len()].to_string()
    } else {
        name.to_string()
    }
}

fn format_time(ms: u64) -> String {
    let total = ms / 1000;
    format!("{}:{:02}", total / 60, total % 60)
}

/// Vue 禁用按钮的整体不透明度。
pub(crate) const DISABLED_OPACITY: f32 = 0.45;

/// 播放条。`narrow` 对应 Vue 的 `max-width: 1120px` 断点。
pub(crate) fn player_bar(model: &ShellViewModel) -> AnyView {
    let props = BarProps::from_model(model);
    let narrow = model.narrow_viewport();
    let radius = crate::theme_map::radius_2xl(model.admin.corner_radius as f32);
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
        .radius_px(radius)
        .min_height(LengthSpec::Px(124.0))
        .with_layout(|layout| {
            layout.padding_top = Some(LengthSpec::Px(12.0));
            layout.padding_right = Some(LengthSpec::Px(16.0));
            layout.padding_bottom = Some(LengthSpec::Px(16.0));
            layout.padding_left = Some(LengthSpec::Px(16.0));
            layout.paint.box_shadows = vec![nana_ui_core::BoxShadowSpec {
                offset_x: 0.0,
                offset_y: 14.0,
                blur_radius: 34.0,
                spread_radius: -24.0,
                color: [0.0, 0.0, 0.0, 0.45],
                inset: false,
            }];
        });
    widget(card).children(rows).key("player-card").into_any()
}

/// 8px 进度轨。上面盖一层透明拖动区，和 Vue 的透明 range 一样高 22px。
fn progress(props: &BarProps) -> AnyView {
    let seekable = props.has_item() && props.duration_ms > 0 && props.supports_seek;
    let duration = props.duration_ms.max(1) as f64;
    let mut range = RangeField::new(props.current_ms.min(props.duration_ms.max(1)) as f64, 0.0, duration, 100.0)
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
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Px(8.0))
            .min_height(LengthSpec::Px(8.0))
            .painter(ProgressTrack { ratio: props.progress_ratio() }),
    )
    .children((widget(range).key("player-seek").on_cx(|_, event: &RangeChanged, cx| {
        cx.dispatch_program_all(player_message(PlayerMessage::Seek(event.value.max(0.0) as u64)));
    }),))
    .key("player-progress")
    .into_any()
}

/// 三栏：媒体、传输、时间和音量。窄窗拆成三行，传输居中，时间和音量两端对齐。
fn body(props: &BarProps, narrow: bool) -> AnyView {
    let media = media(props);
    let transport = transport(props);
    if narrow {
        let side = widget(Stack::bar(10.0).justify(JustifySpec::SpaceBetween).align(AlignSpec::Center))
            .children((time(props), volume(props)))
            .key("player-side")
            .into_any();
        return widget(Stack::column(18.0))
            .children((media, widget(Stack::bar(0.0).justify(JustifySpec::Center)).children((transport,)).into_any(), side))
            .key("player-body")
            .into_any();
    }
    // 宽窗：媒体占 1 份、最少 220；时间和音量占 0.72 份、最少 170；传输按内容宽。
    // 列太窄时先压媒体，不把音量挤出卡片（Vue 的网格在窄列里会溢出）。
    let media = widget(
        Stack::row(0.0)
            .grow(1.0)
            .shrink(1.0)
            .min_width(LengthSpec::Px(0.0))
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
    widget(Stack::bar(18.0).align(AlignSpec::Center).min_height(LengthSpec::Px(72.0)))
        .children((media, transport, side))
        .key("player-body")
        .into_any()
}

/// 封面和两行字是同一个可点区域：点它打开当前条目的预览。没有条目时整块变淡、不可点。
fn media(props: &BarProps) -> AnyView {
    let title = props.title();
    // 没有条目时整块按 Vue 的 0.45 淡化，在卡片底色上混出来。
    let dim = (!props.has_item()).then_some((SemanticColorRole::Surface, DISABLED_OPACITY));
    let hit = widget(parts::hit_area(&title, !props.has_item(), 0.0, 6.0))
        .key("player-media")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(player_message(PlayerMessage::OpenPreview { item_id: None })));
    let meta = widget(
        Stack::column(3.0)
            .width(LengthSpec::Shrink)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0),
    )
    .children((
        widget(parts::dim_text(parts::line(title, 14.0, 700, SemanticColorRole::Text, 21.7), dim)).key("player-title"),
        widget(parts::dim_text(parts::line(props.subtitle(), 11.667, 500, SemanticColorRole::Muted, 18.08), dim))
            .key("player-subtitle"),
    ));
    widget(Stack::row(12.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).min_height(LengthSpec::Px(58.0)))
        .children((hit, cover(props, dim), meta))
    .key("player-media-row")
    .into_any()
}

/// 58px 封面。有纹理画缩略图，没有就写类型字。
fn cover(props: &BarProps, dim: Option<(SemanticColorRole, f32)>) -> AnyView {
    if let Some(path) = props.thumbnail.as_deref() {
        let mut thumb = Thumbnail::new(super::super::thumbs::thumbnail_slot(path)).fit(ContentFit::Cover);
        thumb.style.radius = Some(nana_ui_core::RadiusTier::Lg);
        let layout = std::sync::Arc::make_mut(&mut thumb.style.layout);
        layout.width = Some(LengthSpec::Px(58.0));
        layout.height = Some(LengthSpec::Px(58.0));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        return widget(thumb).key("player-cover").into_any();
    }
    parts::type_cover(props.media_label(), 58.0, "player-cover", dim)
}

/// 循环模式、上一首、播放 / 暂停、下一首、当前队列。
fn transport(props: &BarProps) -> AnyView {
    let has_item = props.has_item();
    let skip_disabled = !has_item || !props.can_play;
    let (mode_icon, mode_label) = match props.mode {
        PlaybackMode::Shuffle => (icons::SHUFFLE, "随机播放"),
        PlaybackMode::SingleLoop => (icons::REPEAT_1, "单曲循环"),
        PlaybackMode::ListLoop => (icons::REPEAT, "列表循环"),
    };
    let playing = props.playing;
    let (play_icon, play_label) = if playing { (icons::PAUSE, "暂停") } else { (icons::PLAY, "播放") };
    widget(Stack::row(6.0).grow(0.0).shrink(0.0).align(AlignSpec::Center))
        .children((
            widget(glyph_button(mode_icon, mode_label, 15.0, 32.0, !has_item))
                .key("player-cycle-mode")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(player_message(PlayerMessage::CycleMode))),
            widget(glyph_button(icons::SKIP_BACK, "上一首", 16.0, 32.0, skip_disabled))
                .key("player-previous")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(player_message(PlayerMessage::PlayPrevious))),
            widget(glyph_button(play_icon, play_label, 17.0, 36.0, skip_disabled))
                .key("player-play")
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(player_message(PlayerMessage::SetPlaying(!playing)))),
            widget(glyph_button(icons::SKIP_FORWARD, "下一首", 16.0, 32.0, skip_disabled))
                .key("player-next")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(player_message(PlayerMessage::PlayNext { natural_end: false }));
                }),
            widget(glyph_button(icons::LIST_MUSIC, "当前队列", 16.0, 32.0, !has_item))
                .key("player-queue")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(player_message(PlayerMessage::ToggleQueue))),
        ))
        .key("player-transport")
        .into_any()
}

/// 只有图标的按钮。可读名字给无障碍，图标按 Vue 的像素大小画，颜色是正文色。
pub(crate) fn glyph_button(icon: Icon, label: &str, icon_size: f32, box_size: f32, disabled: bool) -> IconButton {
    let mut button = IconButton::new(icon, label).colors_from_style().disabled(disabled);
    button.style.foreground = Some(SemanticColorRole::Text);
    button.style.background = None;
    button.style.square = None;
    button.style.control_padding_x = None;
    button.style.painter = Some(GlyphButton { icon, size: icon_size, color: SemanticColorRole::Text, dim: disabled }.into());
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(box_size));
    layout.height = Some(LengthSpec::Px(box_size));
    layout.min_width = Some(LengthSpec::Px(box_size));
    layout.min_height = Some(LengthSpec::Px(box_size));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    button
}

/// 12px 弱色时间。窄列里可以在「/」后面折行。
fn time(props: &BarProps) -> AnyView {
    widget(
        Text::new(props.time_text())
            .color(SemanticColorRole::Muted)
            .font_size(12.0)
            .line_height(18.6)
            .with_min_width_zero(),
    )
    .key("player-time")
    .into_any()
}

/// 音量图标和 120px 滑杆。滑杆最窄 72px；没有条目或播放器不支持音量时变淡。
fn volume(props: &BarProps) -> AnyView {
    let disabled = !props.has_item() || !props.supports_volume;
    let slider = parts::slider(f64::from(props.volume), 0.0, 1.0, 0.01, "音量", disabled, 120.0, 72.0);
    widget(Stack::row(8.0).align(AlignSpec::Center).shrink(1.0).min_width(LengthSpec::Px(0.0)))
        .children((
            widget(parts::icon(icons::VOLUME_2, 15.0, SemanticColorRole::Muted)),
            widget(slider).key("player-volume").on_cx(|_, event: &RangeChanged, cx| {
                cx.dispatch_program_all(player_message(PlayerMessage::SetVolume(event.value.clamp(0.0, 1.0) as f32)));
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
