//! 播放列表和底部播放条。
//!
//! 排序用 `ReorderList`。播放、暂停、跳转和音量走共用的 `MediaTransportBar`。
//! 验收场景不挂这块表面，旧的上移下移按钮保持原样。

use nana_ui::icons_tabler::{EYE, LIST, PLAYER_PAUSE, PLAYER_PLAY, PLAYER_SKIP_BACK, PLAYER_SKIP_FORWARD, REPEAT, VOLUME};
use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, JustifySpec, LengthSpec, RadiusTier, RangeChanged, RangeField, ReorderItem, ReorderList,
    ReorderListEvent, SemanticColorRole, Stack, Text, TextChanged, TextHorizontalAlignment, TextInput,
};

use super::player::PlayerMessage;
use super::{ShellMessage, ShellViewModel};

/// 文件页和播放列表页共用的播放表面。
pub(super) fn player_surface(model: &ShellViewModel) -> AnyView {
    let mut rows = Vec::new();
    if model.workspace.panel == super::workspace::WorkspacePanel::Playlist {
        rows.push(playlist_page(model));
    } else {
        rows.push(transport(model));
    }
    if model.player.queue_open {
        rows.push(queue_list(model));
    }
    if let Some(membership) = membership(model) {
        rows.push(membership);
    }
    widget(Stack::column(8.0)).children(rows).key("player-surface").into_any()
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
    let missing_player = model.player.notice.is_empty()
        && model.player.candidates.iter().all(|candidate| candidate.player_type_id != detail.playlist.player_type_id)
        && model.player.contributions.iter().all(|player| player.player_type_id != detail.playlist.player_type_id);
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
        vec![widget(Button::new("播放").disabled(!can_play).icon(PLAYER_PLAY))
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
        body.push(reorder_list(model));
    }
    if !model.player.notice.is_empty() {
        body.push(text(model.player.notice.clone()).key("playlist-player-notice").into_any());
    }
    body.push(transport(model));
    super::workbench::panel(body)
}

fn reorder_list(model: &ShellViewModel) -> AnyView {
    let items = model.playlist_item_ids.iter().zip(model.playlist_item_entries.iter()).map(|(id, label)| {
        ReorderItem::new(id.clone(), label.clone()).selected(model.player.current_id.as_deref() == Some(id))
    });
    widget(ReorderList::new(items).label("播放集条目"))
        .key("playlist-reorder")
        .on_cx(|_, event: &ReorderListEvent, cx| {
            if let ReorderListEvent::Reorder { source, before } = event {
                cx.dispatch_program(player_message(PlayerMessage::Reorder {
                    source: source.to_string(),
                    before: before.as_ref().map(|value| value.to_string()),
                }));
            }
        })
        .into_any()
}

fn media_type_label(class: &str) -> &'static str {
    match class {
        "image" => "图片",
        "video" => "视频",
        "audio" => "音频",
        _ => "媒体",
    }
}

fn media_thumb(label: &'static str) -> AnyView {
    let mut caption = Text::new(label).color(SemanticColorRole::Muted).font_size(11.0).font_weight(700);
    caption.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(58.0))
            .height(LengthSpec::Px(58.0))
            .min_width(LengthSpec::Px(58.0))
            .min_height(LengthSpec::Px(58.0))
            .grow(0.0)
            .shrink(0.0)
            .surface(SemanticColorRole::Background)
            .radius(RadiusTier::Lg)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center),
    )
    .children((widget(caption),))
    .into_any()
}

fn play_button(icon: nana_ui::Icon, disabled: bool, playing_now: bool) -> AnyView {
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

fn transport(model: &ShellViewModel) -> AnyView {
    let session = &model.player.session;
    let playing_now = session.status == "playing";
    let has_item = model.player.current_id.is_some();
    let duration_ms = session.duration_ms.unwrap_or(0).max(1) as f64;
    let seekable = has_item && session.can_seek && session.status != "failed" && session.duration_ms.unwrap_or(0) > 0;
    let title = model.player.current_item().map(|item| format!("正在播放 {}", item.filename)).unwrap_or_else(|| "未选择播放内容".into());
    let subtitle = model.player.current_item().map(|item| item.path.clone()).unwrap_or_else(|| "选择播放集后可开始播放".into());
    let media = model.player.current_item().map(|item| media_type_label(&item.file_class)).unwrap_or("媒体");
    let queue_label = if model.player.queue_open { "关闭队列" } else { "当前队列" };
    let play_icon = if playing_now { PLAYER_PAUSE } else { PLAYER_PLAY };
    let mut rows = vec![
        widget(
            Stack::column(10.0)
                .surface(SemanticColorRole::Surface)
                .outline(SemanticColorRole::BorderStrong, 1.0)
                .radius(RadiusTier::Xl)
                .padding_xy(16.0, 12.0)
                .min_height(LengthSpec::Px(124.0)),
        )
        .children((
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
            }),
            widget(Stack::bar(12.0).align(AlignSpec::Center)).children((
                widget(Stack::row(12.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
                    media_thumb(media),
                    widget(Stack::column(3.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
                        text(title).key("player-title"),
                        widget(Text::new(subtitle).color(SemanticColorRole::Muted).font_size(12.0)).key("player-subtitle"),
                    )),
                )),
                widget(Stack::row(6.0).grow(0.0).shrink(0.0)).children((
                    widget(super::title_bar::shell_icon(REPEAT, model.player.mode_text(), !has_item))
                        .key("player-cycle-mode")
                        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(player_message(PlayerMessage::CycleMode))),
                    widget(super::title_bar::shell_icon(PLAYER_SKIP_BACK, "上一首", !has_item))
                        .key("player-previous")
                        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(player_message(PlayerMessage::PlayPrevious))),
                    play_button(play_icon, !has_item, playing_now),
                    widget(super::title_bar::shell_icon(PLAYER_SKIP_FORWARD, "下一首", !has_item))
                        .key("player-next")
                        .on_cx(|_, _: &Activate, cx| {
                            cx.dispatch_program(player_message(PlayerMessage::PlayNext { natural_end: false }));
                        }),
                    widget(super::title_bar::shell_icon(LIST, queue_label, !has_item))
                        .key("player-queue")
                        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(player_message(PlayerMessage::ToggleQueue))),
                    widget(super::title_bar::shell_icon(EYE, "打开预览", false))
                        .key("player-open-preview")
                        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(player_message(PlayerMessage::OpenPreview))),
                )),
                widget(Stack::row(8.0).grow(0.0).shrink(1.0)).children((
                    text(model.player.time_text()).key("player-time"),
                    widget(super::title_bar::shell_icon(VOLUME, "音量", !has_item || !session.can_volume)),
                    {
                        let mut volume = RangeField::new(f64::from(session.volume.clamp(0.0, 1.0)), 0.0, 1.0, 0.01)
                            .label("音量")
                            .show_label(false)
                            .show_value(false)
                            .disabled(!has_item || !session.can_volume);
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
                )),
            )),
        ))
        .key("player-card")
        .into_any(),
    ];
    if !model.player.activity.is_empty() {
        rows.push(text(model.player.activity.clone()).key("player-activity").into_any());
    }
    let download = model.player.download_text();
    if !download.is_empty() {
        rows.push(text(download).key("player-download").into_any());
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
        button("适应").key("player-fit-contain").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(player_message(PlayerMessage::SetObjectFit { cover: false }));
        }),
        button("填充").key("player-fit-cover").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(player_message(PlayerMessage::SetObjectFit { cover: true }));
        }),
        text(model.player.object_fit_text()).key("player-fit-label"),
    )).into_any()
}

fn queue_list(model: &ShellViewModel) -> AnyView {
    let mut rows = vec![text(format!("当前队列 {} 项", model.player.queue.len())).key("player-queue-count").into_any()];
    for item in &model.player.queue {
        let id = item.id.clone();
        let label = if item.status == "ready" {
            item.filename.clone()
        } else {
            format!("{} · {}", item.filename, item.status_reason.clone().unwrap_or_else(|| item.status.clone()))
        };
        rows.push(
            button(label)
                .key(format!("player-queue-{id}"))
                .disabled(item.status != "ready")
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(player_message(PlayerMessage::PlayItem { item_id: id.clone() }));
                })
                .into_any(),
        );
    }
    widget(Stack::column(4.0)).children(rows).into_any()
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
    let mut rows = vec![text("加入播放列表").key("player-membership-title").into_any()];
    for action in actions {
        let playlist_id = action.playlist_id.clone();
        let kind = entry.kind.clone();
        let extension = entry.extension.clone().unwrap_or_default();
        let asset_id = entry.asset_id.clone().unwrap_or_default();
        let is_virtual = entry.is_virtual;
        let path = entry.path.clone();
        rows.push(
            button(action.label)
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
