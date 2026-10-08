//! Windows 系统媒体传输控件。
//!
//! 播放、暂停、上一首、下一首、跳转和停止写回现有播放条消息。
//! 正式 Windows 构建才向系统注册会话。测试构建不注册，也不打开真系统会话。

use std::sync::Mutex;

use crate::backend::services::repository::PlaylistItem;

use super::player::{PlayerMessage, PlayerState};
use super::ShellViewModel;

/// 系统媒体键。和浏览器 `MediaSession` 的 play、pause、previoustrack、nexttrack、seekto、stop 对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCommand {
    Play,
    Pause,
    Previous,
    Next,
    Seek(u64),
    Stop,
}

/// 推给系统控件的当前曲目。封面路径来自播放列表缩略图，缺图时再用文件行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaNowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub thumbnail_path: Option<String>,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub enabled: bool,
}

static PENDING: Mutex<Vec<MediaCommand>> = Mutex::new(Vec::new());

/// 测试构建恒为 false。正式 Windows 构建才把系统媒体会话编进来。
pub fn session_compiled_in() -> bool {
    cfg!(all(windows, not(test)))
}

/// 把媒体键收成已有的播放条消息。停止不删除已存储的会话。
pub fn command_message(command: MediaCommand, repo_id: Option<String>) -> PlayerMessage {
    match command {
        MediaCommand::Play => PlayerMessage::SetPlaying(true),
        MediaCommand::Pause => PlayerMessage::SetPlaying(false),
        MediaCommand::Previous => PlayerMessage::PlayPrevious,
        MediaCommand::Next => PlayerMessage::PlayNext { natural_end: false },
        MediaCommand::Seek(position_ms) => PlayerMessage::Seek(position_ms),
        MediaCommand::Stop => PlayerMessage::Stop { repo_id, clear_stored: false },
    }
}

/// 从当前播放项和元数据拼出系统控件要显示的标题、艺人和专辑。
pub fn now_playing(player: &PlayerState, file_thumbnail: Option<&str>) -> MediaNowPlaying {
    let listed = listed_item(player);
    let filename = player
        .current_item()
        .map(|item| item.filename.as_str())
        .filter(|name| !name.is_empty())
        .or_else(|| player.preview_audio_armed().then(|| file_name(player.preview_path())));
    let metadata = listed.and_then(|item| item.metadata.as_ref());
    let (title, artist, album) = media_text(filename, metadata);
    let thumbnail = listed
        .and_then(|item| item.thumbnail_path.as_deref())
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .or_else(|| file_thumbnail.map(str::trim).filter(|path| !path.is_empty()).map(str::to_string));
    let enabled = player.current_item().is_some() || player.preview_audio_armed();
    MediaNowPlaying {
        title,
        artist,
        album,
        thumbnail_path: thumbnail,
        playing: player.session.status == "playing",
        position_ms: player.session.current_time_ms,
        duration_ms: player.session.duration_ms.unwrap_or(0),
        enabled,
    }
}

/// 把排队的媒体键写回播放条。队列为空时什么都不做。
pub fn poll(shell: &mut ShellViewModel) {
    let commands = match PENDING.lock() {
        Ok(mut pending) => std::mem::take(&mut *pending),
        Err(error) => {
            eprintln!("Nana 系统媒体键队列不可用：{error}");
            return;
        }
    };
    if commands.is_empty() {
        return;
    }
    for command in commands {
        let repo_id = shell.player.repo_id.clone();
        shell.reduce(super::ShellMessage::Player(command_message(command, repo_id)));
    }
    shell.surface_dirty = true;
}

/// 把当前曲目推给系统控件。测试构建直接返回。
pub fn sync(window: &nana_ui::WindowHandle, shell: &ShellViewModel) {
    #[cfg(all(windows, not(test)))]
    sync_windows(window, shell);
    #[cfg(not(all(windows, not(test))))]
    {
        let _ = (window, shell);
    }
}

fn listed_item(player: &PlayerState) -> Option<&PlaylistItem> {
    let current = player.current_id.as_deref()?;
    player.listed.as_ref()?.items.iter().find(|item| item.playlist_item_id == current)
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().filter(|name| !name.is_empty()).unwrap_or("MomoBako")
}

/// 标题、艺人、专辑。艺人优先用数组，其次用单个字符串。
fn media_text(filename: Option<&str>, metadata: Option<&serde_json::Value>) -> (String, String, String) {
    let fields = metadata.and_then(|value| value.as_object());
    let title = fields
        .and_then(|fields| fields.get("title"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .or_else(|| filename.map(str::to_string))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "MomoBako".into());
    let artist = fields
        .and_then(|fields| fields.get("artists"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items.iter().filter_map(|item| item.as_str()).map(str::trim).filter(|text| !text.is_empty()).collect::<Vec<_>>().join(", ")
        })
        .filter(|text| !text.is_empty())
        .or_else(|| {
            fields.and_then(|fields| fields.get("artist")).and_then(|value| value.as_str()).map(str::trim).filter(|text| !text.is_empty()).map(str::to_string)
        })
        .unwrap_or_default();
    let album = fields
        .and_then(|fields| fields.get("album"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or("")
        .to_string();
    (title, artist, album)
}

#[allow(dead_code)]
pub(super) fn enqueue(command: MediaCommand) {
    match PENDING.lock() {
        Ok(mut pending) => pending.push(command),
        Err(error) => eprintln!("Nana 系统媒体键无法入队：{error}"),
    }
}

#[cfg(all(windows, not(test)))]
fn sync_windows(window: &nana_ui::WindowHandle, shell: &ShellViewModel) {
    if !host::ensure(window) {
        return;
    }
    let thumb = file_thumbnail(shell);
    let display = now_playing(&shell.player, thumb.as_deref());
    if let Err(error) = host::push_display(&display) {
        eprintln!("Nana 系统媒体控件更新失败：{error}");
    }
}

#[cfg(all(windows, not(test)))]
fn file_thumbnail(shell: &ShellViewModel) -> Option<String> {
    let path = shell
        .player
        .current_item()
        .map(|item| item.path.as_str())
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .or_else(|| shell.player.preview_audio_armed().then(|| shell.player.preview_path().to_string()))?;
    shell
        .files
        .rows
        .iter()
        .chain(shell.files.virtual_rows.iter())
        .find(|row| row.path == path)
        .and_then(|row| row.thumbnail_path.clone())
}

#[cfg(all(windows, not(test)))]
mod host {
    use std::sync::Mutex;

    use raw_window_handle::RawWindowHandle;
    use windows::core::{factory, HSTRING};
    use windows::Foundation::{TimeSpan, TypedEventHandler};
    use windows::Media::{
        MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs, SystemMediaTransportControls,
        SystemMediaTransportControlsButton, SystemMediaTransportControlsButtonPressedEventArgs,
        SystemMediaTransportControlsTimelineProperties,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;

    use super::{enqueue, MediaCommand, MediaNowPlaying};

    struct Session {
        controls: SystemMediaTransportControls,
        fingerprint: String,
    }

    static SESSION: Mutex<Option<Session>> = Mutex::new(None);
    static FAILED: Mutex<bool> = Mutex::new(false);

    pub fn ensure(window: &nana_ui::WindowHandle) -> bool {
        if SESSION.lock().ok().is_some_and(|slot| slot.is_some()) {
            return true;
        }
        if FAILED.lock().ok().is_some_and(|failed| *failed) {
            return false;
        }
        let redraw = window.clone();
        let mut request = window.with_native_handle(move |handle| install(handle, redraw));
        match request.try_take() {
            Some(Ok(Ok(()))) => true,
            Some(Ok(Err(error))) => {
                eprintln!("Nana 系统媒体会话注册失败：{error}");
                if let Ok(mut failed) = FAILED.lock() {
                    *failed = true;
                }
                false
            }
            Some(Err(error)) => {
                eprintln!("Nana 系统媒体会话拿不到窗口句柄：{error}");
                if let Ok(mut failed) = FAILED.lock() {
                    *failed = true;
                }
                false
            }
            None => false,
        }
    }

    pub fn push_display(display: &MediaNowPlaying) -> Result<(), String> {
        let mut slot = SESSION.lock().map_err(|error| error.to_string())?;
        let Some(session) = slot.as_mut() else {
            return Ok(());
        };
        let fingerprint = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            display.enabled,
            display.playing,
            display.position_ms,
            display.duration_ms,
            display.title,
            display.artist,
            display.album,
            display.thumbnail_path.as_deref().unwrap_or("")
        );
        if session.fingerprint == fingerprint {
            return Ok(());
        }
        apply_display(&session.controls, display)?;
        session.fingerprint = fingerprint;
        Ok(())
    }

    fn install(handle: raw_window_handle::WindowHandle<'_>, redraw: nana_ui::WindowHandle) -> Result<(), String> {
        let hwnd = hwnd_of(handle)?;
        let controls = unsafe {
            let interop = factory::<SystemMediaTransportControls, ISystemMediaTransportControlsInterop>().map_err(|error| error.to_string())?;
            interop.GetForWindow::<SystemMediaTransportControls>(hwnd).map_err(|error| error.to_string())?
        };
        controls.SetIsEnabled(true).map_err(|error| error.to_string())?;
        controls.SetIsPlayEnabled(true).map_err(|error| error.to_string())?;
        controls.SetIsPauseEnabled(true).map_err(|error| error.to_string())?;
        controls.SetIsStopEnabled(true).map_err(|error| error.to_string())?;
        controls.SetIsNextEnabled(true).map_err(|error| error.to_string())?;
        controls.SetIsPreviousEnabled(true).map_err(|error| error.to_string())?;
        let redraw_buttons = redraw.clone();
        controls
            .ButtonPressed(&TypedEventHandler::<SystemMediaTransportControls, SystemMediaTransportControlsButtonPressedEventArgs>::new(move |_, args| {
                if let Some(args) = args.as_ref() {
                    if let Ok(button) = args.Button() {
                        if let Some(command) = command_of(button) {
                            enqueue(command);
                            let _ = redraw_buttons.request_redraw();
                        }
                    }
                }
                Ok(())
            }))
            .map_err(|error| error.to_string())?;
        let redraw_seek = redraw;
        controls
            .PlaybackPositionChangeRequested(&TypedEventHandler::<SystemMediaTransportControls, PlaybackPositionChangeRequestedEventArgs>::new(move |_, args| {
                if let Some(args) = args.as_ref() {
                    if let Ok(position) = args.RequestedPlaybackPosition() {
                        let millis = (position.Duration / 10_000).max(0) as u64;
                        enqueue(MediaCommand::Seek(millis));
                        let _ = redraw_seek.request_redraw();
                    }
                }
                Ok(())
            }))
            .map_err(|error| error.to_string())?;
        match SESSION.lock() {
            Ok(mut slot) => *slot = Some(Session { controls, fingerprint: String::new() }),
            Err(error) => return Err(error.to_string()),
        }
        Ok(())
    }

    fn command_of(button: SystemMediaTransportControlsButton) -> Option<MediaCommand> {
        match button {
            SystemMediaTransportControlsButton::Play => Some(MediaCommand::Play),
            SystemMediaTransportControlsButton::Pause => Some(MediaCommand::Pause),
            SystemMediaTransportControlsButton::Previous => Some(MediaCommand::Previous),
            SystemMediaTransportControlsButton::Next => Some(MediaCommand::Next),
            SystemMediaTransportControlsButton::Stop => Some(MediaCommand::Stop),
            _ => None,
        }
    }

    fn apply_display(controls: &SystemMediaTransportControls, display: &MediaNowPlaying) -> Result<(), String> {
        controls.SetIsEnabled(display.enabled).map_err(|error| error.to_string())?;
        let status = if !display.enabled {
            MediaPlaybackStatus::Closed
        } else if display.playing {
            MediaPlaybackStatus::Playing
        } else {
            MediaPlaybackStatus::Paused
        };
        controls.SetPlaybackStatus(status).map_err(|error| error.to_string())?;
        let updater = controls.DisplayUpdater().map_err(|error| error.to_string())?;
        updater.SetType(MediaPlaybackType::Music).map_err(|error| error.to_string())?;
        let music = updater.MusicProperties().map_err(|error| error.to_string())?;
        music.SetTitle(&HSTRING::from(&display.title)).map_err(|error| error.to_string())?;
        music.SetArtist(&HSTRING::from(&display.artist)).map_err(|error| error.to_string())?;
        music.SetAlbumTitle(&HSTRING::from(&display.album)).map_err(|error| error.to_string())?;
        if let Some(path) = display.thumbnail_path.as_deref() {
            if let Err(error) = set_thumbnail(&updater, path) {
                eprintln!("Nana 系统媒体封面没有写上：{error}");
            }
        }
        if display.duration_ms > 0 {
            let timeline = SystemMediaTransportControlsTimelineProperties::new().map_err(|error| error.to_string())?;
            let end = TimeSpan { Duration: (display.duration_ms as i64).saturating_mul(10_000) };
            let position = TimeSpan { Duration: (display.position_ms.min(display.duration_ms) as i64).saturating_mul(10_000) };
            timeline.SetStartTime(TimeSpan { Duration: 0 }).map_err(|error| error.to_string())?;
            timeline.SetEndTime(end).map_err(|error| error.to_string())?;
            timeline.SetMinSeekTime(TimeSpan { Duration: 0 }).map_err(|error| error.to_string())?;
            timeline.SetMaxSeekTime(end).map_err(|error| error.to_string())?;
            timeline.SetPosition(position).map_err(|error| error.to_string())?;
            controls.UpdateTimelineProperties(&timeline).map_err(|error| error.to_string())?;
        }
        updater.Update().map_err(|error| error.to_string())?;
        Ok(())
    }

    fn set_thumbnail(updater: &windows::Media::SystemMediaTransportControlsDisplayUpdater, path: &str) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|error| format!("读取封面失败：{error}"))?;
        if bytes.is_empty() {
            return Err("封面文件是空的".into());
        }
        let stream = windows::Storage::Streams::InMemoryRandomAccessStream::new().map_err(|error| error.to_string())?;
        let writer = windows::Storage::Streams::DataWriter::CreateDataWriter(&stream).map_err(|error| error.to_string())?;
        writer.WriteBytes(&bytes).map_err(|error| error.to_string())?;
        writer.StoreAsync().map_err(|error| error.to_string())?.join().map_err(|error| error.to_string())?;
        writer.DetachStream().map_err(|error| error.to_string())?;
        stream.Seek(0).map_err(|error| error.to_string())?;
        let reference = windows::Storage::Streams::RandomAccessStreamReference::CreateFromStream(&stream).map_err(|error| error.to_string())?;
        updater.SetThumbnail(&reference).map_err(|error| error.to_string())?;
        Ok(())
    }

    fn hwnd_of(handle: raw_window_handle::WindowHandle<'_>) -> Result<HWND, String> {
        let raw = handle.as_raw();
        match raw {
            RawWindowHandle::Win32(window) => Ok(HWND(window.hwnd.get() as *mut core::ffi::c_void)),
            _ => Err("当前窗口不是 Win32".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::player::PlayerState;

    #[test]
    fn test_build_does_not_enable_a_system_session() {
        assert!(!session_compiled_in());
    }

    #[test]
    fn media_commands_map_onto_existing_player_messages() {
        assert!(matches!(command_message(MediaCommand::Play, None), PlayerMessage::SetPlaying(true)));
        assert!(matches!(command_message(MediaCommand::Pause, None), PlayerMessage::SetPlaying(false)));
        assert!(matches!(command_message(MediaCommand::Previous, None), PlayerMessage::PlayPrevious));
        assert!(matches!(command_message(MediaCommand::Next, None), PlayerMessage::PlayNext { natural_end: false }));
        assert!(matches!(command_message(MediaCommand::Seek(1500), None), PlayerMessage::Seek(1500)));
        match command_message(MediaCommand::Stop, Some("repo".into())) {
            PlayerMessage::Stop { repo_id, clear_stored } => {
                assert_eq!(repo_id.as_deref(), Some("repo"));
                assert!(!clear_stored);
            }
            _ => panic!("停止没有写成现有的 Stop 消息"),
        }
    }

    #[test]
    fn now_playing_uses_metadata_title_artist_album_and_thumbnail() {
        let mut player = PlayerState::default();
        player.session.status = "playing".into();
        player.session.current_time_ms = 1200;
        player.session.duration_ms = Some(8000);
        player.current_id = Some("item".into());
        let detail = crate::backend::services::repository::PlaylistDetail {
            playlist: crate::backend::services::repository::PlaylistSummary {
                playlist_id: "pl".into(),
                repo_id: "repo".into(),
                name: "列表".into(),
                player_type_id: "audio".into(),
                player_plugin_id: "audio".into(),
                player_label: "音频".into(),
                file_class: "audio".into(),
                item_count: 1,
                sort_order: 0,
                created_at: String::new(),
                updated_at: String::new(),
            },
            items: vec![crate::backend::services::repository::PlaylistItem {
                playlist_item_id: "item".into(),
                playlist_id: "pl".into(),
                asset_id: "asset".into(),
                path: "a.mp3".into(),
                filename: "a.mp3".into(),
                extension: "mp3".into(),
                thumbnail_path: Some("cover.png".into()),
                status: "ready".into(),
                status_reason: None,
                sort_order: 0,
                added_at: String::new(),
                is_virtual: false,
                provider_id: None,
                provider_item_id: None,
                source_payload: None,
                metadata: Some(serde_json::json!({
                    "title": "夜曲",
                    "artists": ["甲", "乙"],
                    "album": "专辑"
                })),
                local_absolute_path: None,
            }],
        };
        player.queue.push(super::super::player::QueueItem {
            id: "item".into(),
            playlist_id: "pl".into(),
            asset_id: "asset".into(),
            path: "a.mp3".into(),
            filename: "a.mp3".into(),
            extension: "mp3".into(),
            status: "ready".into(),
            status_reason: None,
            transient: false,
            player_type_id: "audio".into(),
            player_label: "音频".into(),
            file_class: "audio".into(),
            thumbnail_path: None,
        });
        player.listed = Some(detail);
        let display = now_playing(&player, Some("other.png"));
        assert_eq!(display.title, "夜曲");
        assert_eq!(display.artist, "甲, 乙");
        assert_eq!(display.album, "专辑");
        assert_eq!(display.thumbnail_path.as_deref(), Some("cover.png"));
        assert!(display.playing);
        assert!(display.enabled);
        assert_eq!(display.position_ms, 1200);
    }
}
