//! 播放列表成员、按路径加入和排序。
//!
//! 这些只改播放器状态并留下副作用请求，真正的读写由 `player_dispatch` 交给仓库服务。

use std::collections::BTreeMap;

use crate::backend::services::repository::{
    PlaylistItemsByPathsAddRequest, PlaylistItemsOrderRequest, PlaylistMembershipRequest, PlaylistMembershipSnapshot,
};

use super::support::{compatible_playlist_ids, membership_can_toggle, next_membership_ids, reorder_before};
use super::{MembershipAction, PlayerEffect, PlayerState};

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
            Ok(memberships) => self.memberships = memberships,
            // 和 Vue `syncPlaylistMemberships` 一样读不到就当没有成员，不提示。
            Err(error) => {
                eprintln!("Nana 读取播放列表成员失败：{error}");
                self.memberships.clear();
            }
        }
    }

    pub(super) fn note_membership_saved(&mut self, result: Result<PlaylistMembershipSnapshot, String>) {
        match result {
            Ok(snapshot) => {
                self.memberships.insert(snapshot.asset_id, snapshot.playlist_ids);
            }
            Err(error) => {
                eprintln!("Nana 更新播放列表成员失败：{error}");
                self.note_failure(format!("更新播放集成员失败：{error}"));
            }
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
        self.effects.push(PlayerEffect::Reorder(PlaylistItemsOrderRequest {
            repo_id: repo_id.to_string(),
            playlist_id: playlist_id.to_string(),
            item_ids,
        }));
    }
}
