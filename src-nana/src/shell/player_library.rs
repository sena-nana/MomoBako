//! 播放列表成员、按路径加入、排序和下载进度。
//!
//! 这些只改播放器状态并留下副作用请求，真正的读写由 `player_dispatch` 交给仓库服务。

use std::collections::BTreeMap;

use crate::backend::services::repository::{
    DownloaderPlaylistProgressEvent, DownloaderPlaylistRequest, PlaylistItemsByPathsAddRequest, PlaylistItemsOrderRequest,
    PlaylistMembershipRequest, PlaylistMembershipSnapshot,
};

use super::support::{compatible_playlist_ids, membership_can_toggle, next_membership_ids, reorder_before};
use super::{DownloadProgress, MembershipAction, PlayerEffect, PlayerState};

impl PlayerState {
    pub fn membership_actions(&self, kind: &str, extension: &str, asset_id: &str, is_virtual: bool) -> Vec<MembershipAction> {
        let toggle = membership_can_toggle(kind, asset_id, is_virtual);
        let current = self.memberships.get(asset_id).map(Vec::as_slice).unwrap_or(&[]);
        compatible_playlist_ids(kind, extension, &self.playlists, &self.contributions, &self.candidates)
            .into_iter()
            .filter_map(|playlist_id| {
                let playlist = self.playlists.iter().find(|playlist| playlist.playlist_id == playlist_id)?;
                let checked = toggle && current.iter().any(|id| id == &playlist_id);
                let label = if checked { format!("移出 {}", playlist.name) } else { format!("加入 {}", playlist.name) };
                Some(MembershipAction { playlist_id, label, checked, toggle })
            })
            .collect()
    }

    pub fn download_indeterminate(&self) -> bool {
        self.download.phase == "submitting" && self.download.total == 0
    }

    pub fn download_text(&self) -> String {
        match self.download.phase.as_str() {
            "idle" | "" => String::new(),
            "submitting" => "正在提交播放列表下载…".into(),
            "start" | "track" => format!(
                "正在下载 {} / {}，失败 {}",
                self.download.completed,
                self.download.total,
                self.download.failed
            ),
            "complete" => format!("下载完成，成功 {}，失败 {}", self.download.completed, self.download.failed),
            "error" => self.download.error.clone().unwrap_or_else(|| "播放列表下载失败".into()),
            other => format!("下载 {other}"),
        }
    }

    pub(super) fn toggle_membership(
        &mut self,
        playlist_id: &str,
        kind: &str,
        extension: &str,
        asset_id: &str,
        is_virtual: bool,
        path: &str,
        writable: bool,
        repo_id: Option<&str>,
    ) {
        let compatible = compatible_playlist_ids(kind, extension, &self.playlists, &self.contributions, &self.candidates);
        if !compatible.iter().any(|id| id == playlist_id) {
            eprintln!("Nana 播放列表不兼容这个条目：{playlist_id}");
            return;
        }
        let Some(repo_id) = repo_id else {
            eprintln!("Nana 成员资格需要活动仓库");
            return;
        };
        if !writable {
            eprintln!("Nana 资源库不可写，已忽略播放列表成员变更");
            return;
        }
        if membership_can_toggle(kind, asset_id, is_virtual) {
            let current = self.memberships.get(asset_id).cloned().unwrap_or_default();
            let playlist_ids = next_membership_ids(&current, playlist_id);
            self.activity = "正在更新播放列表成员…".into();
            self.effects.push(PlayerEffect::SetMembership(PlaylistMembershipRequest {
                repo_id: repo_id.to_string(),
                asset_id: asset_id.to_string(),
                playlist_ids,
            }));
            return;
        }
        if path.trim().is_empty() {
            eprintln!("Nana 没有可加入播放列表的路径");
            return;
        }
        self.activity = "正在按路径加入播放列表…".into();
        self.effects.push(PlayerEffect::AddByPaths(PlaylistItemsByPathsAddRequest {
            repo_id: repo_id.to_string(),
            playlist_id: playlist_id.to_string(),
            paths: vec![path.to_string()],
        }));
    }

    pub(super) fn note_memberships(&mut self, repo_id: &str, result: Result<BTreeMap<String, Vec<String>>, String>, active_repo: Option<&str>) {
        if active_repo != Some(repo_id) {
            eprintln!("Nana 忽略过期的播放列表成员：{repo_id}");
            return;
        }
        match result {
            Ok(memberships) => {
                self.memberships = memberships;
                self.activity.clear();
            }
            Err(error) => {
                eprintln!("Nana 读取播放列表成员失败：{error}");
                self.memberships.clear();
                self.activity = error;
            }
        }
    }

    pub(super) fn note_membership_saved(&mut self, result: Result<PlaylistMembershipSnapshot, String>) {
        self.activity.clear();
        match result {
            Ok(snapshot) => {
                self.memberships.insert(snapshot.asset_id, snapshot.playlist_ids);
            }
            Err(error) => {
                eprintln!("Nana 更新播放列表成员失败：{error}");
                self.activity = error;
            }
        }
    }

    pub(super) fn start_download(&mut self, request: DownloaderPlaylistRequest) {
        self.download_playlist_id = Some(request.playlist_id);
        self.download = DownloadProgress { phase: "submitting".into(), ..DownloadProgress::default() };
        self.activity = "正在提交播放列表下载…".into();
        self.effects.push(PlayerEffect::Download(request));
    }

    pub(super) fn finish_download(&mut self, result: Result<(serde_json::Value, Vec<serde_json::Value>), String>) {
        match result {
            Ok((_output, events)) => {
                for event in events {
                    match serde_json::from_value::<DownloaderPlaylistProgressEvent>(event) {
                        Ok(event) => self.apply_download(event),
                        Err(error) => eprintln!("Nana 播放列表下载进度无法解析：{error}"),
                    }
                }
                if self.download.phase == "submitting" {
                    self.download.phase = "complete".into();
                    self.download_playlist_id = None;
                }
                self.activity = self.download_text();
            }
            Err(error) => {
                eprintln!("Nana 播放列表下载失败：{error}");
                self.download.phase = "error".into();
                self.download.error = Some(error.clone());
                self.download_playlist_id = None;
                self.activity = error;
            }
        }
    }

    pub(super) fn apply_download(&mut self, event: DownloaderPlaylistProgressEvent) {
        if self.download_playlist_id != Some(event.playlist_id) {
            eprintln!("Nana 忽略过期的播放列表下载进度：{}", event.playlist_id);
            return;
        }
        self.download.phase = event.phase.clone();
        self.download.total = event.total;
        self.download.completed = event.completed;
        self.download.failed = event.failed;
        self.download.current_song_name = event.current_song_name;
        self.download.error = event.error;
        if event.phase == "complete" {
            self.download_playlist_id = None;
        }
    }

    pub(super) fn reorder(&mut self, source: &str, before: Option<&str>, playlist_id: Option<&str>, ids: &[String], writable: bool, repo_id: Option<&str>) {
        let Some(playlist_id) = playlist_id else {
            eprintln!("Nana 排序需要先选中播放列表");
            return;
        };
        let Some(repo_id) = repo_id else {
            eprintln!("Nana 排序需要活动仓库");
            return;
        };
        if !writable {
            eprintln!("Nana 资源库不可写，已忽略播放列表排序");
            return;
        }
        let Some(item_ids) = reorder_before(ids, source, before) else {
            return;
        };
        self.activity = "正在保存播放列表顺序…".into();
        self.effects.push(PlayerEffect::Reorder(PlaylistItemsOrderRequest {
            repo_id: repo_id.to_string(),
            playlist_id: playlist_id.to_string(),
            item_ids,
        }));
    }
}
