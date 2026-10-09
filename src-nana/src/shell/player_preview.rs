//! 预览页的音视频和播放条共用一个当前项，以及播放时钟。
//!
//! 和 Vue `FilePreviewPane` 一样：预览音视频时，文件作为临时条目插到当前项后面并接管播放条
//! （`playEntry`）；预览的正是已经装好的当前项时不重装，预览页跟着播放条那份会话走。
//! 离开预览页后临时条目留作当前项继续播放。播放时钟只有一份，在这里每帧往前拨，
//! 到头按自然结束切下一项，图片按停留时长切。

use crate::backend::services::repository::PlaybackSessionState;

use super::super::inspect::InspectState;
use super::wav_player::{self, Action, Output};
use super::{find_player_for_extension, PlayerMatch, PlayerState, PreviewPcm, QueueItem};

/// 播放时钟拨一帧的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClockStep {
    /// 没在播放，时钟没动。
    Idle,
    /// 只是进度往前走，界面跟着热信号走，不用重挂。
    Progressed,
    /// 播放状态变了：到头切了下一项，或者画面出现、消失。
    Changed,
}

/// 预览页正在显示的文件。扩展名为空时从路径取。
pub(crate) struct PreviewEntry<'a> {
    pub repo_id: &'a str,
    pub path: &'a str,
    pub extension: &'a str,
    pub asset_id: &'a str,
}

impl PlayerState {
    /// 预览刚接管当前项、还没离开预览页。
    pub(crate) fn preview_owns_bar(&self) -> bool {
        !self.preview_path.is_empty()
    }

    /// 当前项已经读好（音视频或图片），控制可以直接落到游标或时钟上。
    pub(super) fn item_loaded(&self) -> bool {
        self.loaded_item.is_some() && self.loaded_item == self.current_id
    }

    /// 当前项的 PCM 正装在共用游标里。
    pub(super) fn audible(&self) -> bool {
        self.cursor_item.is_some() && self.cursor_item == self.current_id
    }

    /// 控制落点：出声、只走时钟，或缺解码器。
    pub(super) fn output(&self) -> Output {
        if self.audible() {
            Output::Cursor
        } else if self.item_loaded() {
            Output::Clock
        } else {
            Output::Missing
        }
    }

    /// 预览页显示的正是播放器里已经装好、没有失败的当前项。
    pub(crate) fn mirrors_preview(&self, repo_id: Option<&str>, path: Option<&str>) -> bool {
        let Some(path) = path else {
            return false;
        };
        repo_id.is_some()
            && self.repo_id.as_deref() == repo_id
            && self.item_loaded()
            && self.session.status != "failed"
            && self.current_item().is_some_and(|item| item.path == path)
    }

    /// 预览页的音视频读好了。正是已经装好的当前项时，只把播放条的会话写回预览页，声音不断；
    /// 否则像 Vue `playEntry` 一样插成临时条目接管播放条：游标换成预览的 PCM（没有 PCM 就只走时钟），
    /// 会话换成预览解出的时长和能力，音量沿用播放条的。
    ///
    /// 没有能播放这个扩展名的播放器时不接管，和 Vue 一样只报「没有可用于播放此媒体的插件」。
    /// 返回是否插了临时条目（只有这时才该自动播放）。
    pub(crate) fn take_over_preview(
        &mut self,
        entry: PreviewEntry<'_>,
        session: PlaybackSessionState,
        pcm: Option<PreviewPcm>,
        inspect: &mut InspectState,
    ) -> bool {
        if self.mirrors_preview(Some(entry.repo_id), Some(entry.path)) {
            self.publish(inspect);
            return false;
        }
        let Some(item) = self.preview_item(&entry) else {
            eprintln!("Nana 没有可用于播放此媒体的插件：{}", entry.path);
            // Vue `playEntry` 的 `setError`：播放条的次行写原因，下一次装载成功或停止时清掉。
            self.session.error = Some("没有可用于播放此媒体的插件".into());
            return false;
        };
        if self.repo_id.as_deref().is_some_and(|current| current != entry.repo_id) {
            self.stop_runtime(false, inspect);
            self.listed = None;
            self.current_id = None;
            self.queue.clear();
            self.history.clear();
            self.shuffle_order.clear();
        }
        self.repo_id = Some(entry.repo_id.to_string());
        self.insert_transient(item.clone());
        self.current_id = Some(item.id.clone());
        if self.history.last() != Some(&item.id) {
            self.history.push(item.id.clone());
        }
        // 还在读取的播放列表当前项作废；游标、画面和幻灯片都让给预览。
        self.load_generation = self.load_generation.wrapping_add(1);
        self.wav.clear();
        self.still = None;
        self.outside_note = None;
        self.clip_frames = None;
        self.clip_owned = false;
        self.cursor_item = None;
        if let Some(pcm) = pcm {
            self.wav.install_preview(pcm);
            self.cursor_item = Some(item.id.clone());
        }
        self.loaded_item = Some(item.id.clone());
        self.preview_path = entry.path.to_string();
        let volume = self.session.volume;
        self.session = PlaybackSessionState {
            session_id: format!("playback-{}", entry.repo_id),
            repo_id: entry.repo_id.to_string(),
            playlist_id: item.playlist_id.clone(),
            playlist_item_id: Some(item.id.clone()),
            volume,
            ..session
        };
        self.apply_cursor_volume();
        self.wants_playing = self.session.status == "playing";
        self.can_play = self.session.status != "failed";
        self.publish(inspect);
        true
    }

    /// 离开预览页（选别的文件、回到目录）。临时条目留作当前项，有声音的继续放；
    /// 只有画面的条目离开后没有东西可放，停在当前位置。
    pub(crate) fn disarm_preview_audio(&mut self) {
        if !self.preview_owns_bar() {
            return;
        }
        self.preview_path.clear();
        if !self.audible() && self.session.status == "playing" {
            self.session.status = "paused".into();
            self.wants_playing = false;
        }
    }

    /// 预览接管后的自动播放，和播放条上点播放走同一条路。
    pub(crate) fn play_from_preview(&mut self, inspect: &mut InspectState) {
        self.set_playing(true, inspect);
    }

    /// 播放时钟：播放中每帧往前拨 `step_ms`，把新进度写回预览页。到头（音视频到时长、
    /// 图片到停留时长）按自然结束切下一项，没有下一项就停在末尾。返回这一帧是只往前走还是切了项。
    ///
    /// 算法：只有一份时钟。条目没装好、不在播放时不动；到头先把会话停在时长处并停掉游标，
    /// 再交给 `play_next(true)`，单曲循环、随机和列表循环的取舍都在那里。
    pub(crate) fn advance_clock(&mut self, step_ms: u64, inspect: &mut InspectState) -> ClockStep {
        if self.session.status != "playing" || !self.item_loaded() {
            return ClockStep::Idle;
        }
        let duration = self.session.duration_ms.unwrap_or(0);
        let next = self.session.current_time_ms.saturating_add(step_ms);
        if duration == 0 || next < duration {
            self.session.current_time_ms = next;
            self.publish(inspect);
            return ClockStep::Progressed;
        }
        self.session.current_time_ms = duration;
        if self.audible() {
            let session = self.session.clone();
            let (session, error) = wav_player::drive(Output::Cursor, &self.wav, session, Action::Pause);
            self.session = session;
            if let Some(error) = error {
                eprintln!("Nana 播放到头后停声失败：{error}");
            }
        }
        self.session.status = "paused".into();
        self.play_next(true, inspect);
        ClockStep::Changed
    }

    /// 测试用：游标里当前项的声音是否在放。测试构建不打开声卡，只看游标状态。
    #[cfg(test)]
    pub(crate) fn cursor_playing(&self) -> bool {
        self.audible() && self.wav.is_playing()
    }

    /// 新装进游标的 PCM 用播放条当前的音量，不让换曲把音量弹回 100%。
    pub(super) fn apply_cursor_volume(&mut self) {
        if !self.audible() || !self.session.can_volume {
            return;
        }
        let session = self.session.clone();
        let volume = session.volume;
        let (session, error) = wav_player::drive(Output::Cursor, &self.wav, session, Action::Volume(volume));
        self.session = session;
        if let Some(error) = error {
            eprintln!("Nana 新条目沿用音量失败：{error}");
        }
    }

    /// 临时条目插到当前项后面，同一路径的旧临时条目先去掉。随机模式下也排在当前项后面。
    pub(super) fn insert_transient(&mut self, item: QueueItem) {
        let current = self.current_id.clone();
        self.queue.retain(|queue_item| !queue_item.transient || queue_item.path != item.path);
        let Some(index) = current.as_ref().and_then(|id| self.queue.iter().position(|queue_item| &queue_item.id == id)) else {
            self.queue.push(item);
            return;
        };
        self.queue.insert(index + 1, item.clone());
        if self.mode != super::PlaybackMode::Shuffle {
            return;
        }
        let mut order: Vec<String> = self
            .shuffle_order
            .iter()
            .filter(|id| self.queue.iter().any(|queue_item| &queue_item.id == *id))
            .cloned()
            .collect();
        if let Some(order_index) = order.iter().position(|id| Some(id) == current.as_ref()) {
            order.insert(order_index + 1, item.id);
            self.shuffle_order = order;
        }
    }

    /// 按扩展名找播放器，拼出预览文件的临时条目。找不到播放器返回 `None`。
    fn preview_item(&mut self, entry: &PreviewEntry<'_>) -> Option<QueueItem> {
        let filename = entry.path.rsplit(['/', '\\']).next().unwrap_or(entry.path).to_string();
        let extension = if entry.extension.trim().is_empty() {
            filename.rsplit_once('.').map(|(_, ext)| ext.to_string()).unwrap_or_default()
        } else {
            entry.extension.to_string()
        };
        let extension = extension.trim().trim_start_matches('.').to_ascii_lowercase();
        let (player_type_id, label, file_class) = self.entry_player(&extension)?;
        Some(self.transient_item(entry.path, &filename, &extension, entry.asset_id, &player_type_id, &label, &file_class))
    }

    /// 能放这个扩展名的音视频播放器：插件登记的贡献优先，其次是内置候选。
    pub(super) fn entry_player(&self, extension: &str) -> Option<(String, String, String)> {
        match find_player_for_extension(extension, &self.candidates, &self.contributions)? {
            PlayerMatch::Native(candidate) => {
                Some((candidate.player_type_id.clone(), candidate.label.clone(), candidate.file_class.clone()))
            }
            PlayerMatch::Contribution(contribution) => Some((
                contribution.player_type_id.clone(),
                contribution.label.clone(),
                contribution.file_class.clone(),
            )),
        }
    }
}
