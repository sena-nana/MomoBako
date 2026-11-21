//! 底部播放条。
//!
//! 卡片里上面是媒体行（封面、标题和次行，点了打开预览），下面是 NanaUI 的 `MediaTransportBar`：
//! 前导槽放循环模式、上一首、播放 / 暂停、下一首和当前队列，条自己画进度、「当前 / 总长」和音量弹层，
//! 图片、视频才有的停留时长和适应 / 填充放在条的第二行。窗口不宽于 1120px 时传输条换成 `Stacked`，
//! 进度单独占一行。当前队列是挂在队列按钮上的 `Popover`（`shell/player_view/bar/parts.rs`）。
//! 文件页、预览页和播放集页共用这一块。

use nana_ui::runtime::view::{AnyView, FieldWrite, IntoView, widget};
use nana_ui::runtime::{
    Activate, AlignSpec, IconButton, LengthSpec, ListItem, MediaTransportBar,
    MediaTransportDensity, MediaTransportEvent, MediaTransportIcons, MediaTransportPlacement,
    RadiusTier, SemanticColorRole, SizeChanged, Stack,
};
use nana_ui::{ControlSize, Icon};
use nana_ui_core::{TextWeight, TypeRole};

use super::super::player::{PlaybackMode, PlayerMessage, QueueItem};
use super::super::{ShellMessage, ShellViewModel, hot};
use super::icons;

mod parts;

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
            // 有错误就写在次行，没有条目时也写。
            error,
            // 队列跟 `queue_open` 走，不因为当前项没了就收起。
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

    /// 「当前 / 总长」。条上的读数由传输条自己按同一格式写，这里只给测试核对会话。
    #[cfg(test)]
    pub(crate) fn time_text(&self) -> String {
        let clock = |ms: u64| format!("{}:{:02}", ms / 1000 / 60, ms / 1000 % 60);
        format!("{} / {}", clock(self.current_ms), clock(self.duration_ms))
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

/// 去掉扩展名后的标题。
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

/// 传输条上的当前位置（毫秒），按 1 毫秒的总长夹住。挂载和逐帧同步（热信号）共用这一份算法。
pub(crate) fn position_ms(current_ms: u64, duration_ms: u64) -> f64 {
    current_ms.min(duration_ms.max(1)) as f64
}

/// 播放条。卡片里上面是媒体行，下面是 NanaUI 的 `MediaTransportBar`；窗口不宽于 1120px 时传输条
/// 换成 `Stacked`，进度单独占一行，整张卡片是媒体、进度、按钮三行。卡片是 `--radius-2xl`，跟着主题变。
pub(crate) fn player_bar(model: &ShellViewModel) -> AnyView {
    let props = BarProps::from_model(model);
    let narrow = model.narrow_viewport();
    let card = Stack::column(10.0)
        .surface(SemanticColorRole::Surface)
        .border(SemanticColorRole::BorderStrong, 1.0)
        .radius(RadiusTier::Xxl)
        .with_layout(|layout| {
            layout.padding_top = Some(LengthSpec::Px(12.0));
            layout.padding_right = Some(LengthSpec::Px(16.0));
            layout.padding_bottom = Some(LengthSpec::Px(12.0));
            layout.padding_left = Some(LengthSpec::Px(16.0));
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
        .children((media(&props), transport_bar(&props, narrow)))
        .key("player-card")
        .on_cx(|_, event: &SizeChanged, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::BarResized(event.width)))
        })
        .into_any()
}

/// 传输条：进度、「当前 / 总长」和音量弹层由 `MediaTransportBar` 自己画，前导槽放循环模式、上一首、
/// 播放 / 暂停、下一首和当前队列，第二行放图片、视频才有的设置。条自己的播放钮、设置菜单和全屏都藏起来。
///
/// 进度松手（或键盘每一步）才跳转，拖动中只预览读数；当前时间随播放时钟走热信号，播放推进不重挂播放条。
/// 音量拖动中每一格都写入。没有条目、或播放器既不能跳转也不能调音量时，进度和音量一起停用。
fn transport_bar(props: &BarProps, narrow: bool) -> AnyView {
    let has_item = props.has_item();
    let position_ms = position_ms(props.current_ms, props.duration_ms);
    let mut bar = MediaTransportBar::new()
        .placement(MediaTransportPlacement::Inline)
        .density(if narrow {
            MediaTransportDensity::Stacked
        } else {
            MediaTransportDensity::Regular
        })
        .show_play(false)
        .show_settings(false)
        .show_fullscreen(false)
        .seekable(has_item && props.duration_ms > 0 && props.supports_seek)
        .icons(MediaTransportIcons {
            play: icons::PLAY,
            pause: icons::PAUSE,
            volume: icons::VOLUME_2,
            volume_muted: icons::VOLUME_X,
            ..MediaTransportIcons::default()
        });
    bar.playing = props.playing;
    bar.duration = props.duration_ms as f64 / 1000.0;
    bar.position = position_ms / 1000.0;
    bar.volume = f64::from(props.volume) * 100.0;
    bar.disabled = !has_item || (!props.supports_seek && !props.supports_volume);
    bar.volume_label = Some("音量".into());
    let bar = widget(bar)
        .prop::<f64, PositionMs>(hot::prop(|signals| signals.position, position_ms))
        .leading(transport(props))
        .key("player-transport-bar")
        .on_cx(|_, event: &MediaTransportEvent, cx| match *event {
            MediaTransportEvent::Seek(seconds) => {
                cx.dispatch_program_all(player_message(PlayerMessage::Seek(
                    (seconds.max(0.0) * 1000.0).round() as u64,
                )));
            }
            MediaTransportEvent::Volume(volume) => {
                cx.dispatch_program_all(player_message(PlayerMessage::SetVolume(
                    (volume / 100.0).clamp(0.0, 1.0) as f32,
                )));
            }
            _ => {}
        });
    if props.shows_settings() {
        bar.secondary(parts::settings_row(props, narrow)).into_any()
    } else {
        bar.into_any()
    }
}

/// 传输条的当前时间。热信号给的是毫秒，条上按秒。
struct PositionMs;

impl FieldWrite<MediaTransportBar, f64> for PositionMs {
    const FIELD: &'static str = "MediaTransportBar.position(ms)";

    fn write(target: &mut MediaTransportBar, position_ms: f64) {
        target.position = position_ms / 1000.0;
    }

    fn differs(target: &MediaTransportBar, position_ms: &f64) -> bool {
        target.position != position_ms / 1000.0
    }
}

/// 媒体行：NanaUI 的 `ListItem`，行首 58px 封面，内容是粗体标题和次行。点它打开当前条目的预览；
/// 没有条目时整行停用，字和类型方块按 0.45 在卡片底色上淡化。
fn media(props: &BarProps) -> AnyView {
    let title = props.title();
    let dim = (!props.has_item()).then_some((
        SemanticColorRole::Surface,
        crate::theme_map::DISABLED_OPACITY,
    ));
    // 两行字的宽度来自剩下的空间，不换行，超出时省略号收尾。
    let meta = widget(
        Stack::column(3.0)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0)
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((
        widget(parts::dim_text(
            parts::line(
                title.clone(),
                TypeRole::Section,
                TextWeight::Bold,
                SemanticColorRole::Text,
                21.7,
            ),
            dim,
        ))
        .key("player-title"),
        widget(parts::dim_text(
            parts::line(
                props.subtitle(),
                11.667,
                TextWeight::Medium,
                SemanticColorRole::Muted,
                18.08,
            ),
            dim,
        ))
        .key("player-subtitle"),
    ));
    let mut row = ListItem::new(title)
        .auto_height(true)
        .gap(12.0)
        .disabled(!props.has_item());
    let layout = std::sync::Arc::make_mut(&mut row.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    widget(row)
        .leading(cover(props, dim))
        .content(meta)
        .key("player-media")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::OpenPreview { item_id: None }))
        })
        .into_any()
}

/// 58px 封面。
///
/// 当前条目的路径指向可读本地图片时按 `object-fit: cover` 画进去。路径空、只是 http(s)、
/// 读失败、不是图片或编不成图时仍写类型字。不下载，也不等文件列表把纹理传上来。
/// 没有条目时类型字随整行按 0.45 淡一次；有图时这一格不另淡，点击穿透到媒体行，仍打开预览。
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

/// 前导槽里的循环模式、上一首、播放 / 暂停、下一首和当前队列，都是 NanaUI 的 `IconButton`
/// （队列是 `Popover` 的图标触发器）。
///
/// 只有循环模式有悬停提示（当前模式名）。每个按钮都有无障碍名，读屏读得出名字。
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
        Stack::row(4.0)
            .grow(0.0)
            .shrink(0.0)
            .align(AlignSpec::Center),
    )
    .children((
        widget(transport_button(mode_icon, mode_label, !has_item).with_tooltip(mode_label))
            .key("player-cycle-mode")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(player_message(PlayerMessage::CycleMode))
            }),
        widget(transport_button(icons::SKIP_BACK, "上一首", skip_disabled))
            .key("player-previous")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(player_message(PlayerMessage::PlayPrevious))
            }),
        widget(transport_button(
            play_icon,
            play_label,
            skip_disabled && !props.retry,
        ))
        .key("player-play")
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::SetPlaying(!playing)))
        }),
        widget(transport_button(
            icons::SKIP_FORWARD,
            "下一首",
            skip_disabled,
        ))
        .key("player-next")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::PlayNext {
                natural_end: false,
            }));
        }),
        parts::queue_popover(props),
    ))
    .key("player-transport")
    .into_any()
}

/// 传输按钮：小号 `IconButton`，外观按主题配方。`label` 是无障碍名。
fn transport_button(icon: Icon, label: &str, disabled: bool) -> IconButton {
    IconButton::new(icon, label)
        .size(ControlSize::Small)
        .disabled(disabled)
}

fn player_message(message: PlayerMessage) -> ShellMessage {
    ShellMessage::Player(message)
}

#[cfg(test)]
pub(crate) fn props_for_test(model: &ShellViewModel) -> BarProps {
    BarProps::from_model(model)
}

#[cfg(test)]
mod narrow_card_layout_tests;

#[cfg(test)]
mod seek_drag_tests;
