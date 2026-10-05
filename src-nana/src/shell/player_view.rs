//! 播放列表和底部播放条。
//!
//! 排序用 `ReorderList`。播放、暂停、跳转和音量走共用的 `MediaTransportBar`。
//! 验收场景不挂这块表面，旧的上移下移按钮保持原样。

use nana_ui::icons_tabler::{EYE, LIST, PLAYER_SKIP_BACK, PLAYER_SKIP_FORWARD, REPEAT};
use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, MediaTransportBar, MediaTransportEvent, MediaTransportPlacement, ReorderItem, ReorderList, ReorderListEvent,
    Stack, TextChanged, TextInput,
};

use super::player::PlayerMessage;
use super::{ShellMessage, ShellViewModel};

/// 文件页和播放列表页共用的播放表面。
pub(super) fn player_surface(model: &ShellViewModel) -> AnyView {
    let mut rows = Vec::new();
    if model.workspace.panel == super::workspace::WorkspacePanel::Playlist {
        rows.push(playlist_page(model));
    }
    rows.push(transport(model));
    if model.player.queue_open {
        rows.push(queue_list(model));
    }
    if let Some(membership) = membership(model) {
        rows.push(membership);
    }
    widget(Stack::column(8.0)).children(rows).key("player-surface").into_any()
}

fn playlist_page(model: &ShellViewModel) -> AnyView {
    let mut rows = vec![text("播放集").key("playlist-page-eyebrow").into_any()];
    let Some(detail) = model.player.listed.as_ref() else {
        rows.push(text("选择一个播放集").key("playlist-page-empty").into_any());
        rows.push(text("在左侧播放集区选择要查看或播放的列表。").key("playlist-page-empty-hint").into_any());
        return widget(Stack::column(6.0)).children(rows).into_any();
    };
    rows.push(text(detail.playlist.name.clone()).key("playlist-page-title").into_any());
    let mut subline = format!("{} · {} 项", detail.playlist.player_label, detail.items.len());
    if model.player.notice.is_empty() && model.player.candidates.iter().all(|candidate| candidate.player_type_id != detail.playlist.player_type_id)
        && model.player.contributions.iter().all(|player| player.player_type_id != detail.playlist.player_type_id)
    {
        subline.push_str(" · 缺少对应播放插件");
    }
    rows.push(text(subline).key("playlist-page-status").into_any());
    if detail.items.is_empty() {
        rows.push(text("播放集还是空的").key("playlist-page-no-items").into_any());
        rows.push(text("在文件浏览区右键文件，使用“加入播放列表”把内容加入这里。").key("playlist-page-no-items-hint").into_any());
    } else {
        rows.push(reorder_list(model));
        rows.push(
            button("播放")
                .key("playlist-play")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(player_message(PlayerMessage::PlayListed { item_id: None })))
                .into_any(),
        );
    }
    if !model.player.notice.is_empty() {
        rows.push(text(model.player.notice.clone()).key("playlist-player-notice").into_any());
    }
    widget(Stack::column(6.0)).children(rows).into_any()
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

fn transport(model: &ShellViewModel) -> AnyView {
    let session = &model.player.session;
    let playing_now = session.status == "playing";
    let failed = session.status == "failed" || session.status == "ended" || session.status == "idle";
    let duration = session.duration_ms.unwrap_or(0) as f64 / 1000.0;
    let mut bar = MediaTransportBar::new();
    bar.playing = session.status == "playing";
    bar.disabled = failed || model.player.current_id.is_none();
    bar.seekable = session.can_seek && session.status != "failed";
    bar.position = session.current_time_ms as f64 / 1000.0;
    bar.duration = duration;
    bar.volume = f64::from(session.volume.clamp(0.0, 1.0) * 100.0);
    bar.placement = MediaTransportPlacement::Inline;
    bar.show_fullscreen = Some(false);
    let title = model.player.current_item().map(|item| format!("正在播放 {}", item.filename)).unwrap_or_else(|| "未选择播放内容".into());
    let queue_label = if model.player.queue_open { "关闭队列" } else { "当前队列" };
    let mut rows = vec![
        widget(Stack::bar(8.0)).children((
            text(title).key("player-title"),
            text(model.player.time_text()).key("player-time"),
        )).into_any(),
        widget(bar).key("player-transport").on_cx(move |_, event: &MediaTransportEvent, cx| {
            let message = match event {
                MediaTransportEvent::PlayPause => Some(PlayerMessage::SetPlaying(!playing_now)),
                MediaTransportEvent::Seek(seconds) => Some(PlayerMessage::Seek((*seconds * 1000.0).max(0.0) as u64)),
                MediaTransportEvent::Volume(volume) => Some(PlayerMessage::SetVolume((*volume / 100.0) as f32)),
                _ => None,
            };
            if let Some(message) = message {
                cx.dispatch_program(player_message(message));
            }
        }).into_any(),
        widget(Stack::bar(4.0)).children((
            widget(super::title_bar::shell_icon(REPEAT, model.player.mode_text(), false))
                .key("player-cycle-mode")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(player_message(PlayerMessage::CycleMode));
                }),
            widget(super::title_bar::shell_icon(PLAYER_SKIP_BACK, "上一首", false))
                .key("player-previous")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(player_message(PlayerMessage::PlayPrevious));
                }),
            widget(super::title_bar::shell_icon(PLAYER_SKIP_FORWARD, "下一首", false))
                .key("player-next")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(player_message(PlayerMessage::PlayNext { natural_end: false }));
                }),
            widget(super::title_bar::shell_icon(LIST, queue_label, false))
                .key("player-queue")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(player_message(PlayerMessage::ToggleQueue));
                }),
            widget(super::title_bar::shell_icon(EYE, "打开预览", false))
                .key("player-open-preview")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(player_message(PlayerMessage::OpenPreview));
                }),
        )).into_any(),
    ];
    if !model.player.activity.is_empty() {
        rows.push(text(model.player.activity.clone()).key("player-activity").into_any());
    }
    let download = model.player.download_text();
    if !download.is_empty() {
        rows.push(text(download).key("player-download").into_any());
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
