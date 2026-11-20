//! 侧栏智能文件夹与播放集的展开、选择与列表回写。

use super::*;

impl SidebarState {
    pub fn toggle_smart_folder(&mut self, id: &str) {
        toggle_id(&mut self.expanded_smart_folders, id);
    }

    pub fn select_smart_folder(
        &mut self,
        workspace: &mut WorkspaceState,
        smart_folder_id: &str,
        locked: bool,
    ) -> bool {
        if locked || smart_folder_id.is_empty() || workspace.active_repo_id.is_none() {
            return false;
        }
        let repo_id = workspace.active_repo_id.clone().expect("repository id");
        workspace.panel = WorkspacePanel::SmartFolder;
        self.active_smart_folder_id = Some(smart_folder_id.to_string());
        self.smart_loading = true;
        self.smart_error.clear();
        self.expand_smart_ancestors(smart_folder_id);
        self.effects.push(SidebarEffect::QuerySmartFolder {
            repo_id,
            smart_folder_id: smart_folder_id.to_string(),
        });
        true
    }

    pub fn apply_smart_folders(
        &mut self,
        repo_id: &str,
        result: Result<Vec<SidebarSmartFolder>, String>,
    ) {
        if self.bound_repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 忽略过期的智能文件夹：{repo_id}");
            return;
        }
        match result {
            Ok(folders) => {
                self.smart_folders = folders;
                let valid = smart_ids(&self.smart_folders);
                self.expanded_smart_folders.retain(|id| valid.contains(id));
                if let Some(active) = self.active_smart_folder_id.clone() {
                    self.expand_smart_ancestors(&active);
                }
            }
            Err(error) => {
                eprintln!("Nana 读取智能文件夹失败：{error}");
                self.smart_error = error;
            }
        }
    }

    /// 仓库或当前智能文件夹已经变化时，旧查询不能覆盖新的结果。
    pub fn note_smart_query(
        &mut self,
        repo_id: &str,
        smart_folder_id: &str,
        result: Result<SmartResultSummary, String>,
    ) -> bool {
        if self.bound_repo_id.as_deref() != Some(repo_id)
            || self.active_smart_folder_id.as_deref() != Some(smart_folder_id)
        {
            eprintln!("Nana 忽略过期的智能文件夹查询：{smart_folder_id}");
            return false;
        }
        self.smart_loading = false;
        match result {
            Ok(summary) => {
                self.smart_error.clear();
                self.smart_result = Some(summary);
            }
            Err(error) => {
                eprintln!("Nana 查询智能文件夹失败：{error}");
                self.smart_error = error;
            }
        }
        true
    }

    pub fn toggle_playlists(&mut self) {
        self.playlists_expanded = !self.playlists_expanded;
    }

    pub fn select_playlist(&mut self, workspace: &mut WorkspaceState, playlist_id: &str) -> bool {
        if workspace.active_repo_id.is_none() || playlist_id.is_empty() {
            return false;
        }
        let repo_id = workspace.active_repo_id.clone().expect("repository id");
        workspace.panel = WorkspacePanel::Playlist;
        self.active_playlist_id = Some(playlist_id.to_string());
        self.effects.push(SidebarEffect::LoadPlaylistDetail {
            repo_id,
            playlist_id: playlist_id.to_string(),
        });
        true
    }

    /// 播放集成员变了以后重读绑定仓库的播放集列表（侧栏项数，读回以后播放器跟着重读成员索引）；`detail` 有值时
    /// 再读这一张的详情。没绑仓库或仓库缺失时不读。
    pub(crate) fn reload_playlists(&mut self, detail: Option<String>) {
        let Some(repo_id) = self.bound_repo_id.clone().filter(|_| !self.bound_missing) else {
            eprintln!("Nana 侧栏没有可用的仓库，不重读播放集");
            return;
        };
        self.effects.push(SidebarEffect::LoadPlaylists {
            repo_id: repo_id.clone(),
        });
        if let Some(playlist_id) = detail {
            self.effects.push(SidebarEffect::LoadPlaylistDetail {
                repo_id,
                playlist_id,
            });
        }
    }

    pub fn apply_playlists(
        &mut self,
        workspace: &mut WorkspaceState,
        repo_id: &str,
        result: Result<Vec<SidebarPlaylist>, String>,
    ) {
        if self.bound_repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 忽略过期的播放集列表：{repo_id}");
            return;
        }
        match result {
            Ok(playlists) => {
                let active_exists = self
                    .active_playlist_id
                    .as_ref()
                    .is_some_and(|id| playlists.iter().any(|item| &item.id == id));
                if self.active_playlist_id.is_some() && !active_exists {
                    self.active_playlist_id = None;
                    if workspace.panel == WorkspacePanel::Playlist {
                        workspace.panel = WorkspacePanel::Files;
                    }
                }
                self.playlists = playlists;
            }
            Err(error) => eprintln!("Nana 读取播放集失败：{error}"),
        }
    }
}
