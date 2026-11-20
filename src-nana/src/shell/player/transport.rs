//! 播放器的会话恢复、列表与侧栏播放、上一首下一首、播放暂停、跳转、音量与图片时长。

use super::*;

impl PlayerState {
    pub(super) fn restore(&mut self, detail: &PlaylistDetail) {
        let repo_id = detail.playlist.repo_id.clone();
        let Some(stored) = self.stored.get(&repo_id).cloned() else {
            self.clear_stored(&repo_id);
            return;
        };
        if !self.session_can_restore(&stored, detail) {
            eprintln!("Nana 播放会话无法恢复：{}", stored.playlist_id);
            self.clear_stored(&repo_id);
            return;
        }
        self.listed = Some(detail.clone());
        self.active_player_type_id = Some(detail.playlist.player_type_id.clone());
        self.repo_id = Some(repo_id);
        self.mode = stored.mode;
        self.wants_playing = stored.is_playing;
        self.session.volume = stored.volume;
        self.queue = detail
            .items
            .iter()
            .map(|item| queue_item_from_playlist(item, &detail.playlist))
            .collect();
        self.play_item(&stored.current_item_id, stored.is_playing);
        // 恢复时沿用存下的进度和时长，装好以后能跳转就跳过去；
        // 装载期间播放条显示的也是存下的进度，会话文件不被装载清零。
        self.session.current_time_ms = stored.current_time_ms;
        self.session.duration_ms = (stored.duration_ms > 0).then_some(stored.duration_ms);
        if stored.current_time_ms > 0 && self.session.status == "loading" {
            self.resume = Some((stored.current_item_id.clone(), stored.current_time_ms));
        }
        self.persist_if_needed();
    }

    pub(super) fn session_can_restore(
        &self,
        stored: &StoredSession,
        detail: &PlaylistDetail,
    ) -> bool {
        if detail.playlist.playlist_id != stored.playlist_id {
            return false;
        }
        let ready = detail
            .items
            .iter()
            .any(|item| item.playlist_item_id == stored.current_item_id && item.status == "ready");
        self.player_type_known(&stored.player_type_id) && ready
    }

    pub(super) fn play_listed(&mut self, item_id: Option<String>, inspect: &mut InspectState) {
        let Some(detail) = self.listed.clone() else {
            eprintln!("Nana 没有点开的播放集，不能播放");
            return;
        };
        self.play_detail(&detail, item_id, inspect);
    }

    /// 侧栏行上的「播放」读回了详情。
    ///
    /// 从第一条就绪条目自动播放，没有就绪的就用第一条，并且不点开这张播放集。
    /// 正在看的播放集页留在原地；没点开过播放集时也不因此打开一页。正在看的就是这一张时，用这份新详情换上页上的条目。
    /// 返回 `true` 表示播放的就是正在看的那张，壳层可以刷新它的条目编号。
    pub(super) fn play_sidebar(
        &mut self,
        detail: &PlaylistDetail,
        inspect: &mut InspectState,
    ) -> bool {
        let same_page = self
            .listed
            .as_ref()
            .is_some_and(|page| page.playlist.playlist_id == detail.playlist.playlist_id);
        if self.restore_playlist_id.as_deref() == Some(detail.playlist.playlist_id.as_str()) {
            let open_page = self.listed.clone();
            self.restore_playlist_id = None;
            self.restore(detail);
            if !same_page {
                self.listed = open_page;
            } else {
                self.remember_playlist_detail(detail);
            }
            return same_page;
        }
        if same_page {
            self.listed = Some(detail.clone());
            self.remember_playlist_detail(detail);
        }
        self.play_detail(detail, None, inspect);
        same_page
    }

    /// 从一份播放集详情开始放。`item_id` 为空时先找第一条就绪的，没有就用第一条。不改播放集页。
    pub(super) fn play_detail(
        &mut self,
        detail: &PlaylistDetail,
        item_id: Option<String>,
        inspect: &mut InspectState,
    ) {
        // 没有当前项时封面仍要按这个类型写「音频 / 图片 / 视频」。认不得就留空，封面写「媒体」。
        self.active_player_type_id = Some(detail.playlist.player_type_id.clone());
        if detail.items.is_empty() {
            self.fail_empty_playlist(detail);
            self.publish(inspect);
            return;
        }
        let repo_id = detail.playlist.repo_id.clone();
        if self
            .repo_id
            .as_ref()
            .is_some_and(|current| current != &repo_id)
        {
            self.stop_runtime(true, inspect);
        }
        self.repo_id = Some(repo_id);
        self.queue = detail
            .items
            .iter()
            .map(|item| queue_item_from_playlist(item, &detail.playlist))
            .collect();
        self.history.clear();
        self.shuffle_order.clear();
        let ready = detail
            .items
            .iter()
            .find(|item| item.status == "ready")
            .map(|item| item.playlist_item_id.clone());
        let start = item_id.or(ready).or_else(|| {
            detail
                .items
                .first()
                .map(|item| item.playlist_item_id.clone())
        });
        let Some(start) = start else {
            eprintln!(
                "Nana 播放集没有可以开始的条目：{}",
                detail.playlist.playlist_id
            );
            return;
        };
        if self.mode == PlaybackMode::Shuffle {
            self.ensure_shuffle();
        }
        self.play_item(&start, true);
        self.publish(inspect);
    }

    /// 播放集一项也没有。
    ///
    /// 没有当前项时，播放器类型认得就在播放条次行写
    /// 「当前没有可播放条目」，认不得写「缺少对应播放插件」。进度归零，上一首不再接着放。
    /// 没有当前项就没有会话可存，不写会话文件。队列本来开着就留着。
    pub(super) fn fail_empty_playlist(&mut self, detail: &PlaylistDetail) {
        let repo_id = detail.playlist.repo_id.clone();
        if self
            .repo_id
            .as_ref()
            .is_some_and(|current| current != &repo_id)
        {
            if let Some(previous) = self.repo_id.clone().filter(|id| !id.is_empty()) {
                self.clear_stored(&previous);
            }
        }
        self.repo_id = Some(repo_id.clone());
        self.wav.clear();
        self.cursor_item = None;
        self.loaded_item = None;
        self.retry_item = None;
        self.resume = None;
        self.preview_path.clear();
        self.load_generation = self.load_generation.wrapping_add(1);
        self.drop_clip_frames();
        self.queue.clear();
        self.history.clear();
        self.shuffle_order.clear();
        self.current_id = None;
        self.wants_playing = false;
        self.can_play = false;
        self.notice.clear();
        self.session.repo_id = repo_id;
        self.session.playlist_id = detail.playlist.playlist_id.clone();
        self.session.playlist_item_id = None;
        self.session.status = "failed".into();
        self.session.current_time_ms = 0;
        self.session.duration_ms = None;
        self.session.can_seek = false;
        self.session.can_volume = false;
        let message = if self.player_type_known(&detail.playlist.player_type_id) {
            "当前没有可播放条目"
        } else {
            "缺少对应播放插件"
        };
        eprintln!(
            "Nana 播放集还是空的：{}：{message}",
            detail.playlist.playlist_id
        );
        self.session.error = Some(message.into());
    }

    /// 选中队列项并尝试装载。装载失败时会话保持 failed，不会变成 playing。
    /// 找不到条目时不解码。
    pub(super) fn play_item(&mut self, item_id: &str, auto_play: bool) {
        let Some(item) = self.queue.iter().find(|item| item.id == item_id).cloned() else {
            eprintln!("Nana 播放队列没有这个条目：{item_id}");
            return;
        };
        self.resume = None;
        self.retry_item = None;
        self.current_id = Some(item.id.clone());
        if self.history.last().map(String::as_str) != Some(item.id.as_str()) {
            self.history.push(item.id.clone());
        }
        self.wants_playing = auto_play;
        clip::load_item(self, &item);
        self.persist_if_needed();
    }

    /// 当前项的播放源准备失败过、可以重试：播放条的「播放」可点，点了重新准备（`set_playing`）。
    pub(crate) fn retry_pending(&self) -> bool {
        self.retry_item.is_some() && self.retry_item == self.current_id
    }

    pub(super) fn play_next(&mut self, natural_end: bool, inspect: &mut InspectState) {
        let previous_transient = self.current_item().is_some_and(|item| item.transient);
        let next = self.resolve_next(natural_end);
        let Some(next) = next else {
            self.wants_playing = false;
            if natural_end {
                self.prune_transients();
            }
            self.persist_if_needed();
            self.publish(inspect);
            return;
        };
        self.play_item(&next, true);
        if natural_end || previous_transient {
            self.prune_transients();
        }
        self.publish(inspect);
    }

    pub(super) fn resolve_next(&mut self, natural_end: bool) -> Option<String> {
        let ready = ready_ids(&self.queue);
        match next_ready_id(
            self.mode,
            self.current_id.as_deref(),
            &ready,
            &self.shuffle_order,
            natural_end,
        ) {
            NextStep::None => None,
            NextStep::Item(id) => Some(id),
            NextStep::ReshuffleMissing => {
                self.ensure_shuffle();
                let ready = ready_ids(&self.queue);
                match next_ready_id(
                    self.mode,
                    self.current_id.as_deref(),
                    &ready,
                    &self.shuffle_order,
                    natural_end,
                ) {
                    NextStep::Item(id) => Some(id),
                    NextStep::ReshuffleWrap => {
                        self.ensure_shuffle();
                        self.shuffle_order.first().cloned()
                    }
                    NextStep::None | NextStep::ReshuffleMissing => {
                        self.shuffle_order.first().cloned()
                    }
                }
            }
            NextStep::ReshuffleWrap => {
                self.ensure_shuffle();
                self.shuffle_order.first().cloned()
            }
        }
    }

    pub(super) fn play_previous(&mut self, inspect: &mut InspectState) {
        let ready = ready_ids(&self.queue);
        let Some(previous) =
            previous_ready_id(self.mode, self.current_id.as_deref(), &ready, &self.history)
        else {
            return;
        };
        self.history.pop();
        self.play_item(&previous, true);
        self.publish(inspect);
    }

    pub(super) fn set_playing(&mut self, playing: bool, inspect: &mut InspectState) {
        if playing && self.retry_pending() {
            // 播放源准备失败过：再按「播放」就是重试，换一个代次重新准备，上一次晚到的结果丢掉。
            if let Some(id) = self.current_id.clone() {
                self.play_item(&id, true);
                self.publish(inspect);
            }
            return;
        }
        if self.still.is_some() {
            let drawable = self.still.as_ref().is_some_and(|item| item.frame.is_some());
            self.wants_playing = playing && drawable;
            if drawable {
                self.session.status = if playing { "playing" } else { "paused" }.into();
                self.session.error = None;
                self.can_play = true;
            }
            self.publish(inspect);
            return;
        }
        self.wants_playing = playing;
        if self.session.status == "loading" {
            // 还在读取：只记下意图，装载完成后按它决定是否出声。
            return;
        }
        let Some(output) = self.output() else {
            // 当前项没装好（停过、失败过或换过仓库）：要播放就重新读取，装好后按意图出声。
            if let Some(id) = self.current_id.clone().filter(|_| playing) {
                self.play_item(&id, true);
                self.publish(inspect);
            }
            return;
        };
        let session = self.session.clone();
        let action = if playing {
            wav_player::Action::Play
        } else {
            wav_player::Action::Pause
        };
        let (session, error) = wav_player::drive(output, &self.wav, session, action);
        self.session = session;
        if let Some(error) = error {
            eprintln!("Nana 播放控制失败：{error}");
            self.note_failure(format!("播放控制失败：{error}"));
        }
        self.can_play = self.session.status != "failed" && self.current_item().is_some();
        self.persist_if_needed();
        self.publish(inspect);
    }

    pub(super) fn seek(&mut self, position_ms: u64, inspect: &mut InspectState) {
        if self.still.is_some() {
            eprintln!("Nana 图片幻灯片不支持跳转");
            return;
        }
        let Some(output) = self.output() else {
            eprintln!("Nana 当前项还没装好，不能跳转");
            return;
        };
        let session = self.session.clone();
        let (session, error) = wav_player::drive(
            output,
            &self.wav,
            session,
            wav_player::Action::Seek(position_ms),
        );
        self.session = session;
        if let Some(error) = error {
            eprintln!("Nana 播放进度失败：{error}");
        }
        self.persist_if_needed();
        self.publish(inspect);
    }

    pub(super) fn set_volume(&mut self, volume: f32, inspect: &mut InspectState) {
        if self.still.is_some() {
            eprintln!("Nana 图片幻灯片没有音量");
            return;
        }
        let volume = volume.clamp(0.0, 1.0);
        if self.output() != Some(wav_player::Output::Cursor) {
            // 还没有装好的条目，或者装好的条目没有声音（只有画面的视频）：只记下音量，下一项有声音时沿用。没有声音
            // 时不经控制器，它会按「不支持音量控制」把正在放的画面改成 failed；播放条照样收下音量。
            self.session.volume = volume;
            self.persist_if_needed();
            self.publish(inspect);
            return;
        }
        let session = self.session.clone();
        let (session, error) = wav_player::drive(
            wav_player::Output::Cursor,
            &self.wav,
            session,
            wav_player::Action::Volume(volume),
        );
        self.session = session;
        if let Some(error) = error {
            eprintln!("Nana 播放音量失败：{error}");
        }
        self.persist_if_needed();
        self.publish(inspect);
    }

    pub(super) fn set_image_duration(&mut self, value: u32) {
        self.settings.image_duration_ms =
            support::normalize_image_duration_ms(Some(f64::from(value)));
        self.effects.push(PlayerEffect::PersistSettings);
        if self
            .current_item()
            .is_some_and(|item| item.file_class == "image")
        {
            self.apply_image_duration();
            self.persist_if_needed();
        }
    }

    pub(super) fn apply_image_duration(&mut self) {
        let duration = u64::from(self.settings.image_duration_ms);
        self.session.duration_ms = Some(duration);
        if self.session.current_time_ms > duration {
            self.session.current_time_ms = duration;
        }
    }
}
