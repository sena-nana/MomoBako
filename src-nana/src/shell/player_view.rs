//! 播放列表和底部播放条。
//!
//! 排序用 `ReorderList`。播放、暂停、跳转和音量走共用的 `MediaTransportBar`。
//! 验收场景和产品窗口共用这块表面。排序用重排列表。行上播放带图标，移除是危险幽灵按钮。

use nana_ui::icons_tabler::{GRIP_VERTICAL, LIST, PLAYER_PAUSE, PLAYER_PLAY, PLAYER_SKIP_BACK, PLAYER_SKIP_FORWARD, REPEAT, TRASH, VOLUME};
use nana_ui::ContentFit;
use nana_ui::{ButtonKind, Icon};
use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, EmptyState, GpuTextureView, IconButton, IconGlyph, JustifySpec, LengthSpec, List, ListItem,
    RadiusTier, RangeChanged, RangeField, ReorderItem, ReorderList, ReorderListEvent, SemanticColorRole, SettingsCard, Stack,
    TextChanged, TextHorizontalAlignment, TextInput, Thumbnail,
};

use super::player::PlayerMessage;
use super::{ShellMessage, ShellViewModel};

/// 文件页和播放列表页共用的播放表面。
pub(super) fn player_surface(model: &ShellViewModel) -> AnyView {
    let mut rows = Vec::new();
    if model.player.still.is_some() {
        rows.push(still_stage(model));
        rows.push(transport(model));
    } else {
        if let Some(card) = outside_gap(model) {
            rows.push(card);
        }
        if model.workspace.panel == super::workspace::WorkspacePanel::Playlist {
            rows.push(playlist_page(model));
        } else {
            rows.push(transport(model));
        }
    }
    if model.player.queue_open {
        rows.push(queue_list(model));
    }
    if let Some(membership) = membership(model) {
        rows.push(membership);
    }
    widget(Stack::column(8.0)).children(rows).key("player-surface").into_any()
}

/// 幻灯片主区只放画面或空画面。随机、上一首、下一首和循环留在播放条。
fn still_stage(model: &ShellViewModel) -> AnyView {
    let Some(still) = model.player.still.as_ref() else {
        return widget(EmptyState::new("没有可绘制的画面").message("图片幻灯片需要路径上的 RGBA 帧。"))
            .key("player-still-empty")
            .into_any();
    };
    let body = if still.frame.is_some() {
        let fit = if model.player.settings.object_fit_cover { ContentFit::Cover } else { ContentFit::Contain };
        let caption = format!("{} · {}×{}", still.extension, still.width, still.height);
        widget(Stack::column(8.0).align(AlignSpec::Center))
            .children((
                widget(GpuTextureView::new("player-still").fit(fit)).key("player-still-image"),
                widget(super::workbench::meta(caption)).key("player-still-size"),
            ))
            .into_any()
    } else {
        let message = still.missing.clone().unwrap_or_else(|| "图片幻灯片需要路径上的 RGBA 帧。".into());
        widget(EmptyState::new("没有可绘制的画面").message(message)).key("player-still-empty").into_any()
    };
    widget(
        Stack::column(0.0)
            .height(LengthSpec::Px(320.0))
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center),
    )
    .children((body,))
    .key("player-still-stage")
    .into_any()
}

/// 扩展名不在音视频会话里、也不是图片幻灯片的项。
fn outside_gap(model: &ShellViewModel) -> Option<AnyView> {
    let note = model.player.outside_note.as_ref()?;
    let rows = vec![
        widget(EmptyState::new("没有可绘制的画面").message(note.clone()))
            .key("player-outside-empty")
            .into_any(),
    ];
    Some(widget(SettingsCard::new("播放")).children(rows).key("player-outside-card").into_any())
}

fn playlist_page(model: &ShellViewModel) -> AnyView {
    let Some(detail) = model.player.listed.as_ref() else {
        return super::workbench::dashed_empty(
            "选择一个播放集",
            "在左侧播放集区选择要查看或播放的列表。",
            "playlist-page-empty",
            "playlist-page-empty-hint",
            true,
            false,
        );
    };
    let missing_player = playlist_plugin_missing(model, &detail.playlist.player_type_id);
    let mut subline = format!("{} · {} 项", detail.playlist.player_label, detail.items.len());
    if missing_player {
        subline.push_str(" · 缺少对应播放插件");
    }
    let can_play = !detail.items.is_empty() && !missing_player;
    let mut body = vec![super::workbench::header(
        "播放集",
        "playlist-page-eyebrow",
        &detail.playlist.name,
        "playlist-page-title",
        &subline,
        "playlist-page-status",
        vec![widget(wash_disabled_play(Button::new("播放").disabled(!can_play).icon(PLAYER_PLAY)))
            .key("playlist-play")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program(player_message(PlayerMessage::PlayListed { item_id: None })))
            .into_any()],
    )];
    if detail.items.is_empty() {
        body.push(super::workbench::dashed_empty(
            "播放集还是空的",
            "在文件浏览区右键文件，使用“加入播放列表”把内容加入这里。",
            "playlist-page-no-items",
            "playlist-page-no-items-hint",
            true,
            true,
        ));
    } else {
        body.push(playlist_items(model));
    }
    if !model.player.notice.is_empty() {
        body.push(text(model.player.notice.clone()).key("playlist-player-notice").into_any());
    }
    body.push(transport(model));
    super::workbench::panel(body)
}

/// 一条播放集占一行。标题和扩展名块打开预览；播放带图标，移除是危险幽灵按钮。
fn playlist_items(model: &ShellViewModel) -> AnyView {
    let playlist_id = model
        .selected_playlist_id
        .clone()
        .or_else(|| model.player.listed.as_ref().map(|detail| detail.playlist.playlist_id.clone()))
        .unwrap_or_default();
    let listed = model.player.listed.as_ref();
    let can_play_list = listed.is_some_and(|detail| !playlist_plugin_missing(model, &detail.playlist.player_type_id));
    let mut reorder = Vec::new();
    let mut rows = Vec::new();
    for (id, label) in model.playlist_item_ids.iter().zip(model.playlist_item_entries.iter()) {
        let item = listed.and_then(|detail| detail.items.iter().find(|item| item.playlist_item_id == *id));
        let title = item
            .map(|item| item.filename.clone())
            .unwrap_or_else(|| label.split(" · ").next().unwrap_or(label).to_string());
        let path = item
            .map(|item| {
                if item.status == "ready" {
                    item.path.clone()
                } else {
                    item.status_reason.clone().unwrap_or_else(|| item.status.clone())
                }
            })
            .unwrap_or_default();
        let ready = item.is_none_or(|item| item.status == "ready");
        let extension = item.map(|item| item.extension.clone()).unwrap_or_default();
        reorder.push(ReorderItem::new(id.clone(), title.clone()).selected(model.player.current_id.as_deref() == Some(id.as_str())));
        rows.push(playlist_item_row(id, &playlist_id, &title, &path, &extension, ready && can_play_list));
    }
    widget(ReorderList::new(reorder).live_rows(true).spacing(8.0).label("播放集条目"))
        .key("playlist-reorder")
        .on_cx(|_, event: &ReorderListEvent, cx| {
            if let ReorderListEvent::Reorder { source, before } = event {
                cx.dispatch_program(player_message(PlayerMessage::Reorder {
                    source: source.to_string(),
                    before: before.as_ref().map(|value| value.to_string()),
                }));
            }
        })
        .children(rows)
        .into_any()
}

/// 没有对应播放插件时，页眉和行上的播放都不可点。
pub(super) fn playlist_plugin_missing(model: &ShellViewModel, player_type_id: &str) -> bool {
    let ready = model.player.candidates.iter().any(|candidate| candidate.player_type_id == player_type_id)
        || model.player.contributions.iter().any(|player| player.player_type_id == player_type_id);
    let named = model.player.notice.contains("缺少对应播放插件")
        || model.player.session.error.as_deref().is_some_and(|text| text.contains("缺少对应播放插件"));
    !ready || named
}

/// 底部播放条在缺少插件时不能播放。没有播放集时，只认会话里写明的那句。
fn transport_plugin_missing(model: &ShellViewModel) -> bool {
    if let Some(detail) = model.player.listed.as_ref() {
        return playlist_plugin_missing(model, &detail.playlist.player_type_id);
    }
    model.player.notice.contains("缺少对应播放插件")
        || model.player.session.error.as_deref().is_some_and(|text| text.contains("缺少对应播放插件"))
}

fn playlist_item_row(id: &str, playlist_id: &str, title: &str, path: &str, extension: &str, can_play: bool) -> AnyView {
    let playlist_id = playlist_id.to_string();
    let item_id = id.to_string();
    let play_id = item_id.clone();
    let path_row = (!path.is_empty()).then(|| widget(super::workbench::meta(path)).key(format!("playlist-item-path-{id}")).into_any());
    widget(Stack::bar(8.0).align(AlignSpec::Center).min_height(LengthSpec::Px(44.0)))
        .children((
            drag_handle(id),
            extension_mark(id, extension),
            widget(Stack::column(2.0).grow(1.0).shrink(1.0).min_width(LengthSpec::Px(0.0))).children((
                item_title(id, title),
                path_row,
            )),
            widget(wash_disabled_play(inline_button("播放").icon(PLAYER_PLAY).disabled(!can_play)))
                .key(format!("playlist-item-play-{id}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(player_message(PlayerMessage::PlayListed { item_id: Some(play_id.clone()) }));
                }),
            widget(inline_button("移除").kind(ButtonKind::Danger).icon(TRASH)).key(format!("playlist-remove-{id}")).on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(ShellMessage::RemovePlaylistItem {
                    playlist_id: playlist_id.clone(),
                    item_id: item_id.clone(),
                });
            }),
        ))
        .into_any()
}

/// 拖动排序的手柄。用图标集里的竖握把，行本身仍由重排列表拖动。
fn drag_handle(id: &str) -> AnyView {
    widget(
        Stack::column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Px(32.0))
            .height(LengthSpec::Px(32.0))
            .grow(0.0)
            .shrink(0.0),
    )
    .children((widget(IconGlyph::new(GRIP_VERTICAL).size(16.0)),))
    .key(format!("playlist-item-drag-{id}"))
    .into_any()
}

/// 没有缩略图时显示扩展名。点这块和点标题一样，走已有的打开预览消息。
fn extension_mark(id: &str, extension: &str) -> AnyView {
    let label = extension.trim().to_ascii_uppercase();
    let shown = if label.is_empty() { " ".to_string() } else { label };
    let item_id = id.to_string();
    let mut button = Button::new(shown.clone());
    button.style.background = Some(SemanticColorRole::Subtle);
    button.style.foreground = Some(SemanticColorRole::Muted);
    button.style.border = None;
    button.style.radius = Some(RadiusTier::Md);
    button.style.control_height = None;
    button.style.control_padding_x = None;
    button.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(58.0));
    layout.height = Some(LengthSpec::Px(58.0));
    layout.min_width = Some(LengthSpec::Px(58.0));
    layout.min_height = Some(LengthSpec::Px(58.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.font_size = Some(11.0);
    layout.font_weight = Some(700);
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    let style = button.style.clone();
    widget(Button::new(shown).style(style))
        .key(format!("playlist-item-ext-{id}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(player_message(PlayerMessage::OpenPreview { item_id: Some(item_id.clone()) }));
        })
        .into_any()
}

/// 条目标题。点它打开这一条的预览，不开始播放。
fn item_title(id: &str, title: &str) -> AnyView {
    let item_id = id.to_string();
    let mut button = Button::new(title).kind(ButtonKind::Ghost);
    button.style.control_height = None;
    button.style.control_padding_x = None;
    button.style.text_horizontal_alignment = TextHorizontalAlignment::Start;
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.font_size = Some(14.0);
    layout.font_weight = Some(600);
    layout.width = Some(LengthSpec::Shrink);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    widget(button).key(format!("playlist-item-title-{id}")).on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(player_message(PlayerMessage::OpenPreview { item_id: Some(item_id.clone()) }));
    }).into_any()
}

/// 浅色底上，禁用按钮的 Faint 字色和可点的「移除」贴得太近。
/// 不透明度叠在 Nana 自己的 disabled 绘制上，把整颗按钮再洗淡，文字仍是「播放」。
const DISABLED_PLAY_OPACITY: f32 = 0.4;

/// 缺少播放插件时走按钮的 disabled 绘制，并降低不透明度。
pub(super) fn wash_disabled_play(mut button: Button) -> Button {
    if button.disabled {
        let layout = std::sync::Arc::make_mut(&mut button.style.layout);
        layout.opacity = Some(DISABLED_PLAY_OPACITY);
    }
    button
}

/// 侧栏播放图标和行上的「播放」同一档禁用。文字不画出来，名字只给无障碍。
pub(super) fn wash_disabled_icon(mut button: IconButton) -> IconButton {
    if button.disabled {
        let layout = std::sync::Arc::make_mut(&mut button.style.layout);
        layout.opacity = Some(DISABLED_PLAY_OPACITY);
    }
    button
}

/// 行内次要按钮。宽度跟文字走，不拉成整行标题。
fn inline_button(label: &str) -> Button {
    let mut button = super::workbench::ghost_button(label);
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.width = Some(LengthSpec::Shrink);
    button
}

fn media_type_label(class: &str) -> &'static str {
    match class {
        "image" => "图片",
        "video" => "视频",
        "audio" => "音频",
        _ => "媒体",
    }
}

/// 去掉扩展名后的标题，和 Vue 播放条同一规则。
fn display_title(filename: &str, extension: &str) -> String {
    let name = filename.trim();
    let extension = extension.trim();
    if name.is_empty() || extension.is_empty() {
        return name.to_string();
    }
    let suffix = format!(".{extension}");
    if name.to_ascii_lowercase().ends_with(&suffix.to_ascii_lowercase()) {
        name[..name.len() - suffix.len()].to_string()
    } else {
        name.to_string()
    }
}

fn item_subtitle(item: &super::player::QueueItem) -> String {
    if item.player_label.trim().is_empty() {
        item.path.clone()
    } else {
        format!("{} · {}", item.player_label, item.path)
    }
}

/// 封面优先用已经解码的缩略图。没有纹理时仍留类型字，不拿文件名冒充画面。
/// 点封面走已有的打开预览消息，传输条上不再另放眼睛。
fn media_cover(model: &ShellViewModel, label: &'static str) -> AnyView {
    let path = model
        .player
        .current_item()
        .and_then(|item| item.thumbnail_path.clone())
        .or_else(|| model.inspect.facts.thumbnail_path.clone());
    let ready = path.as_deref().is_some_and(|path| {
        model.files.rows.iter().any(|row| row.texture_ready && row.thumbnail_path.as_deref() == Some(path))
    });
    if let Some(path) = path.filter(|_| ready) {
        let mut thumb = Thumbnail::new(super::thumbs::thumbnail_slot(&path)).fit(ContentFit::Cover);
        let layout = std::sync::Arc::make_mut(&mut thumb.style.layout);
        layout.width = Some(LengthSpec::Px(58.0));
        layout.height = Some(LengthSpec::Px(58.0));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        thumb.style.radius = Some(RadiusTier::Lg);
        return widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(58.0))
                .height(LengthSpec::Px(58.0))
                .min_width(LengthSpec::Px(58.0))
                .min_height(LengthSpec::Px(58.0))
                .grow(0.0)
                .shrink(0.0),
        )
        .children((widget(thumb), cover_hit()))
        .key("player-cover")
        .into_any();
    }
    widget(type_cover(label))
        .key("player-cover")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(player_message(PlayerMessage::OpenPreview { item_id: None }));
        })
        .into_any()
}

/// 没有缩略图时的类型字。方块本身可点，打开当前项预览。
fn type_cover(label: &str) -> Button {
    let mut button = plain_button(label);
    button.style.background = Some(SemanticColorRole::Background);
    button.style.foreground = Some(SemanticColorRole::Muted);
    button.style.radius = Some(RadiusTier::Lg);
    button.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(58.0));
    layout.height = Some(LengthSpec::Px(58.0));
    layout.min_width = Some(LengthSpec::Px(58.0));
    layout.min_height = Some(LengthSpec::Px(58.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.font_size = Some(11.0);
    layout.font_weight = Some(700);
    button
}

/// 缩略图不接收点击。这层透明按钮盖在同一块 58 像素上。
fn cover_hit() -> AnyView {
    let mut button = plain_button("");
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.position = nana_ui_core::PositionSpec::Absolute;
    layout.offset_top = Some(LengthSpec::Px(0.0));
    layout.offset_left = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Px(58.0));
    layout.height = Some(LengthSpec::Px(58.0));
    widget(button).key("player-cover-hit").on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program(player_message(PlayerMessage::OpenPreview { item_id: None }));
    }).into_any()
}

/// 去掉控件高度、内边距和描边，避免把标题画成一颗按钮。
fn plain_button(label: impl Into<String>) -> Button {
    let mut button = Button::new(label);
    button.style.background = None;
    button.style.border = None;
    button.style.radius = None;
    button.style.foreground = Some(SemanticColorRole::Text);
    button.style.control_height = None;
    button.style.control_padding_x = None;
    button.style.control_padding_y = None;
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.border_width = Some(0.0);
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    layout.padding_top = Some(LengthSpec::Px(0.0));
    layout.padding_bottom = Some(LengthSpec::Px(0.0));
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.min_height = Some(LengthSpec::Px(0.0));
    let style = button.style.clone();
    Button::new(button.label).style(style)
}

/// 播放条粗体。有曲名时带「正在播放」，点它打开预览。
fn transport_title(title: String) -> AnyView {
    let mut button = plain_button(title);
    button.style.text_horizontal_alignment = TextHorizontalAlignment::Start;
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.font_size = Some(14.0);
    layout.font_weight = Some(600);
    layout.width = Some(LengthSpec::Shrink);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    widget(button).key("player-title").on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program(player_message(PlayerMessage::OpenPreview { item_id: None }));
    }).into_any()
}

fn play_button(icon: Icon, disabled: bool, playing_now: bool) -> AnyView {
    let mut button = super::title_bar::shell_icon(icon, "播放", disabled);
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(36.0));
    layout.height = Some(LengthSpec::Px(36.0));
    layout.min_width = Some(LengthSpec::Px(36.0));
    layout.min_height = Some(LengthSpec::Px(36.0));
    widget(button).key("player-play").on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program(player_message(PlayerMessage::SetPlaying(!playing_now)));
    }).into_any()
}

/// 空状态或会话外说明已经写过的那句，不再画到播放条和条下。
fn show_activity(model: &ShellViewModel) -> bool {
    if model.player.activity.is_empty() {
        return false;
    }
    let activity = model.player.activity.as_str();
    if model.player.outside_note.as_deref() == Some(activity) {
        return false;
    }
    if model.player.still.as_ref().and_then(|item| item.missing.as_deref()) == Some(activity) {
        return false;
    }
    let gap = model.player.still.as_ref().is_some_and(|item| item.frame.is_none()) || model.player.outside_note.is_some();
    !(gap && model.player.session.error.as_deref() == Some(activity))
}

fn transport(model: &ShellViewModel) -> AnyView {
    let session = &model.player.session;
    let playing_now = session.status == "playing";
    let has_item = model.player.current_id.is_some();
    let preview_audio = model.player.preview_audio_armed();
    let controllable = has_item || preview_audio;
    let duration_ms = session.duration_ms.unwrap_or(0).max(1) as f64;
    let slideshow = model.player.still.is_some()
        || model.player.current_item().is_some_and(|item| item.file_class == "image" && !session.can_seek && !session.can_volume);
    let seekable = !slideshow && controllable && session.can_seek && session.status != "failed" && session.duration_ms.unwrap_or(0) > 0;
    let show_volume = !slideshow;
    let plugin_blocks_play = transport_plugin_missing(model);
    let gap_owns_error = model.player.still.as_ref().is_some_and(|item| item.frame.is_none()) || model.player.outside_note.is_some();
    let title = model.player.current_item().map(|item| {
        let name = display_title(&item.filename, &item.extension);
        if name.is_empty() { "未选择播放内容".into() } else { format!("正在播放 {name}") }
    }).unwrap_or_else(|| {
        if preview_audio { format!("正在预览 {}", display_title(model.player.preview_path(), "")) } else { "未选择播放内容".into() }
    });
    let subtitle = if plugin_blocks_play {
        "缺少对应播放插件".into()
    } else if gap_owns_error {
        model.player.current_item().map(|item| item_subtitle(item)).unwrap_or_else(|| "选择播放集后可开始播放".into())
    } else {
        model.player.session.error.clone().filter(|error| !error.is_empty()).unwrap_or_else(|| {
            model.player.current_item().map(|item| item_subtitle(item)).unwrap_or_else(|| {
                if preview_audio { model.player.preview_path().to_string() } else { "选择播放集后可开始播放".into() }
            })
        })
    };
    let media = model.player.current_item().map(|item| media_type_label(&item.file_class)).unwrap_or("媒体");
    let queue_label = if model.player.queue_open { "关闭队列" } else { "当前队列" };
    let playing_now = playing_now && !plugin_blocks_play;
    let play_icon = if playing_now { PLAYER_PAUSE } else { PLAYER_PLAY };
    let play_disabled = !controllable || plugin_blocks_play;
    let skip_disabled = !has_item || plugin_blocks_play;
    let seek = (!slideshow).then(|| {
        widget(
            RangeField::new(session.current_time_ms as f64, 0.0, duration_ms, 100.0)
                .label("播放进度")
                .show_label(false)
                .show_value(false)
                .disabled(!seekable),
        )
        .key("player-seek")
        .on_cx(|_, event: &RangeChanged, cx| {
            cx.dispatch_program(player_message(PlayerMessage::Seek(event.value.max(0.0) as u64)));
        })
        .into_any()
    });
    let volume = show_volume.then(|| {
        widget(Stack::row(8.0).grow(0.0).shrink(1.0)).children((
            widget(super::title_bar::shell_icon(VOLUME, "音量", !controllable || !session.can_volume)),
            {
                let mut volume = RangeField::new(f64::from(session.volume.clamp(0.0, 1.0)), 0.0, 1.0, 0.01)
                    .label("音量")
                    .show_label(false)
                    .show_value(false)
                    .disabled(!controllable || !session.can_volume);
                let layout = std::sync::Arc::make_mut(&mut volume.style.layout);
                layout.width = Some(LengthSpec::Px(96.0));
                layout.flex_grow = Some(0.0);
                layout.flex_shrink = Some(1.0);
                widget(volume)
                    .key("player-volume")
                    .on_cx(|_, event: &RangeChanged, cx| {
                        cx.dispatch_program(player_message(PlayerMessage::SetVolume(event.value.clamp(0.0, 1.0) as f32)));
                    })
            },
        )).into_any()
    });
    let media_row = widget(Stack::row(12.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0))
        .children((
            media_cover(model, media),
            widget(Stack::column(3.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
                transport_title(title),
                widget(super::workbench::meta(subtitle)).key("player-subtitle"),
            )),
        ))
        .into_any();
    let controls = widget(Stack::row(6.0).grow(0.0).shrink(0.0))
        .children((
            widget(super::title_bar::shell_icon(REPEAT, model.player.mode_text(), !has_item))
                .key("player-cycle-mode")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(player_message(PlayerMessage::CycleMode))),
            widget(super::title_bar::shell_icon(PLAYER_SKIP_BACK, "上一首", skip_disabled))
                .key("player-previous")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(player_message(PlayerMessage::PlayPrevious))),
            play_button(play_icon, play_disabled, playing_now),
            widget(super::title_bar::shell_icon(PLAYER_SKIP_FORWARD, "下一首", skip_disabled))
                .key("player-next")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(player_message(PlayerMessage::PlayNext { natural_end: false }));
                }),
            widget(super::title_bar::shell_icon(LIST, queue_label, !has_item))
                .key("player-queue")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(player_message(PlayerMessage::ToggleQueue))),
        ))
        .into_any();
    let time = text(model.player.time_text()).key("player-time").into_any();
    let body = if model.narrow_viewport() {
        // Vue 窄屏把三列拆成三行：媒体、居中的控件、两端对齐的时间和音量。
        widget(Stack::column(18.0)).children((
            media_row,
            widget(Stack::bar(0.0).justify(JustifySpec::Center)).children((controls,)),
            widget(Stack::bar(10.0)).children((time, widget(Stack::spacer()), volume)),
        ))
        .into_any()
    } else {
        widget(Stack::bar(18.0).align(AlignSpec::Center)).children((
            media_row,
            controls,
            widget(Stack::row(10.0).grow(0.0).shrink(1.0)).children((time, volume)),
        ))
        .into_any()
    };
    let mut rows = vec![
        widget(
            Stack::column(10.0)
                .surface(SemanticColorRole::Surface)
                .outline(SemanticColorRole::BorderStrong, 1.0)
                .radius(RadiusTier::Xl)
                .padding_xy(16.0, 12.0)
                .min_height(LengthSpec::Px(124.0)),
        )
        .children((seek, body))
        .key("player-card")
        .into_any(),
    ];
    if show_activity(model) {
        rows.push(widget(super::workbench::meta(model.player.activity.clone())).key("player-activity").into_any());
    }
    let download = model.player.download_text();
    if !download.is_empty() {
        rows.push(widget(super::workbench::meta(download)).key("player-download").into_any());
    }
    if model.motion.sweep_on() {
        let mut sweep = nana_ui::runtime::Text::new("扫光");
        let layout = std::sync::Arc::make_mut(&mut sweep.style.layout);
        layout.transform = Some(super::motion::shift_x(model.motion.sweep_percent()));
        rows.push(widget(sweep).key("download-sweep").into_any());
    }
    if model.player.current_item().is_some_and(|item| item.file_class == "image" || item.file_class == "video") {
        rows.push(fit_controls(model));
    }
    widget(Stack::column(6.0)).children(rows).key("player-bar").into_any()
}

fn fit_controls(model: &ShellViewModel) -> AnyView {
    let image = model.player.current_item().is_some_and(|item| item.file_class == "image");
    let duration = model.player.settings.image_duration_ms.to_string();
    let duration_row = if image {
        Some(widget(TextInput::new(duration).label("图片停留毫秒")).on_cx(|_, event: &TextChanged, cx| {
            if let Ok(value) = event.value.parse::<u32>() {
                cx.dispatch_program(player_message(PlayerMessage::SetImageDuration(value)));
            }
        }))
    } else {
        None
    };
    widget(Stack::fill_row(8.0)).children((
        duration_row,
        widget(super::workbench::ghost_button("适应")).key("player-fit-contain").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(player_message(PlayerMessage::SetObjectFit { cover: false }));
        }),
        widget(super::workbench::ghost_button("填充")).key("player-fit-cover").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(player_message(PlayerMessage::SetObjectFit { cover: true }));
        }),
        text(model.player.object_fit_text()).key("player-fit-label"),
    )).into_any()
}

/// 队列是播放条下面的次级列表。传输控件留在上面的播放条里。
fn queue_list(model: &ShellViewModel) -> AnyView {
    let mut rows = Vec::new();
    for item in &model.player.queue {
        let id = item.id.clone();
        let detail = if item.status == "ready" {
            String::new()
        } else {
            item.status_reason.clone().unwrap_or_else(|| item.status.clone())
        };
        rows.push(
            widget(ListItem::new(item.filename.clone()).detail(detail).disabled(item.status != "ready"))
                .key(format!("player-queue-{id}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(player_message(PlayerMessage::PlayItem { item_id: id.clone() }));
                })
                .into_any(),
        );
    }
    widget(Stack::column(4.0))
        .children((
            widget(super::workbench::eyebrow(format!("当前队列 {} 项", model.player.queue.len()))).key("player-queue-count"),
            widget(List::new()).children(rows),
        ))
        .into_any()
}

fn membership(model: &ShellViewModel) -> Option<AnyView> {
    let path = model.selected_path.clone()?;
    let entry = model.browser_entries.iter().find(|entry| entry.path == path)?.clone();
    let actions = model.player.membership_actions(
        &entry.kind,
        entry.extension.as_deref().unwrap_or(""),
        entry.asset_id.as_deref().unwrap_or(""),
        entry.is_virtual,
    );
    if actions.is_empty() {
        return None;
    }
    let mut rows = vec![widget(super::workbench::eyebrow("加入播放列表")).key("player-membership-title").into_any()];
    for action in actions {
        let playlist_id = action.playlist_id.clone();
        let kind = entry.kind.clone();
        let extension = entry.extension.clone().unwrap_or_default();
        let asset_id = entry.asset_id.clone().unwrap_or_default();
        let is_virtual = entry.is_virtual;
        let path = entry.path.clone();
        rows.push(
            widget(super::workbench::ghost_button(action.label))
                .key(format!("player-membership-{playlist_id}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(player_message(PlayerMessage::ToggleMembership {
                        playlist_id: playlist_id.clone(),
                        kind: kind.clone(),
                        extension: extension.clone(),
                        asset_id: asset_id.clone(),
                        is_virtual,
                        path: path.clone(),
                    }));
                })
                .into_any(),
        );
    }
    Some(widget(Stack::column(4.0)).children(rows).into_any())
}

fn player_message(message: PlayerMessage) -> ShellMessage {
    ShellMessage::Player(message)
}
