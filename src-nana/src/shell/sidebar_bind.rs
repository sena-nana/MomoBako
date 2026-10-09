//! 侧栏消息落到壳层页面上的那一层。

use crate::backend::services::repository::PlaylistSummary;

use super::super::workspace::{LibraryCategory, MainRegion, WorkspaceEffect};
use super::super::{ShellPage, ShellViewModel};
use super::{ShortcutAsset, ShortcutId, SidebarEffect, SidebarPlaylist, SidebarShortcut, WorkspacePanel};

impl ShellViewModel {
    pub(crate) fn navigation_locked(&self) -> bool {
        self.workspace.main_region() == MainRegion::MissingRepository
            || self.workspace.active_repository().is_some_and(|repository| repository.status == "missing")
    }

    /// 按当前活动仓库重新绑定侧栏。缺失和空仓库只清空，不发请求。换到可用的仓库时照 Vue
    /// `queueRepositoryBackgroundLoads` 在后台再读仓库动作（侧栏「动作」入口要用）和硬链接候选。
    pub(crate) fn bind_sidebar_repository(&mut self) {
        let missing = self.navigation_locked();
        let repo_id = self.workspace.active_repo_id.clone();
        if self.sidebar.bind_repository(repo_id.as_deref(), missing)
            && let Some(repo_id) = repo_id
        {
            self.admin.queue_actions(Some(repo_id.clone()));
            self.files.check_hardlinks(&repo_id);
        }
    }

    pub(crate) fn leave_settings_page(&mut self) {
        if matches!(self.page, ShellPage::Settings | ShellPage::SettingsError) {
            self.page = ShellPage::FileList;
        }
    }

    pub(crate) fn apply_shortcut(&mut self, id: ShortcutId) {
        let locked = self.navigation_locked();
        if self.sidebar.select_shortcut(&mut self.workspace, id, locked) {
            self.leave_settings_page();
        }
    }

    pub(crate) fn apply_quick_access(&mut self, shortcut_id: String) {
        let locked = self.navigation_locked();
        if self.sidebar.open_quick_access(&mut self.workspace, &shortcut_id, locked) {
            self.leave_settings_page();
            self.selected_path = self.sidebar.selected_path.clone();
            self.current_directory = self.sidebar.current_directory.clone();
        }
    }

    /// 快照返回前，文件列表先停在正在打开的目录上。
    pub(crate) fn note_sidebar_browse(&mut self, path: &str) {
        self.files.current_path = path.to_string();
        self.files.loading = true;
    }

    /// 悬停打开发生在 `prepare`。松手会先写下移动文案，拆树前要把目录加载盖回去。
    pub(crate) fn stage_browses(&mut self) {
        let mut browses = std::mem::take(&mut self.staged_browses);
        browses.extend(self.sidebar.take_browses());
        for effect in &browses {
            if let super::SidebarEffect::Browse { path, .. } = effect {
                self.note_sidebar_browse(path);
            }
        }
        self.staged_browses = browses;
    }

    pub(crate) fn apply_open_folder(&mut self, path: String) {
        let locked = self.navigation_locked();
        if self.sidebar.open_folder(&mut self.workspace, &path, locked) {
            self.leave_settings_page();
            self.current_directory = self.sidebar.current_directory.clone();
            self.mark_surface_dirty();
        }
    }

    pub(crate) fn apply_smart_folder(&mut self, smart_folder_id: String) {
        let locked = self.navigation_locked();
        if self.sidebar.select_smart_folder(&mut self.workspace, &smart_folder_id, locked) {
            self.leave_settings_page();
        }
    }

    /// 添加菜单选了一个来源后端。本地文件夹和 Eagle 打开系统文件夹对话框，
    /// 登录型来源跳到插件设置，其余来源切到表单（由侧栏状态完成）。
    pub(crate) fn apply_repository_backend(&mut self, plugin_id: &str) {
        let options = super::backend_options(&self.admin.plugins);
        let Some(option) = options.iter().find(|option| option.plugin_id == plugin_id) else {
            eprintln!("Nana 添加资源库找不到来源插件：{plugin_id}");
            return;
        };
        match self.sidebar.choose_backend(option) {
            Some(super::BackendRoute::PickLocalFolder | super::BackendRoute::PickEagleLibrary) => {
                self.input.queue_attach_dialog();
            }
            Some(super::BackendRoute::OpenSettings(plugin_id)) => {
                super::super::admin::open_settings_page(self, Some(&plugin_id));
            }
            Some(super::BackendRoute::Form) => {}
            None => eprintln!("Nana 来源后端当前不可用：{plugin_id}"),
        }
    }

    /// 每次归约后结算侧栏里等文件服务结果的对话框。
    pub(crate) fn settle_sidebar_dialogs(&mut self) {
        let error = self.files.error.clone();
        self.sidebar.settle_folder_dialogs(self.files.mutating, &error);
    }

    pub(crate) fn apply_playlist(&mut self, playlist_id: String) {
        if self.sidebar.select_playlist(&mut self.workspace, &playlist_id) {
            self.leave_settings_page();
            self.selected_playlist_id = Some(playlist_id);
        }
    }

    /// 仓库的一份新播放集列表（侧栏读取、新建或删除以后）。侧栏和播放器换上同一份：播放器据此读成员、
    /// 恢复存下的会话，文件右键的「加入播放列表」也从这里来，和 Vue 共用一份 `playlists` 一样。
    /// 换了仓库的旧结果不写。`open` 是新建出来的播放集，照 Vue `createPlaylistInWorkspace` 接着点开它。
    pub(crate) fn apply_playlist_list(&mut self, repo_id: &str, playlists: &[PlaylistSummary], open: Option<String>) {
        if self.sidebar.bound_repo_id() != Some(repo_id) {
            eprintln!("Nana 忽略其他仓库的播放集列表：{repo_id}");
            return;
        }
        let rows = playlists.iter().map(SidebarPlaylist::from_summary).collect();
        self.sidebar.apply_playlists(&mut self.workspace, repo_id, Ok(rows));
        self.player.note_playlists(repo_id, playlists);
        if let Some(playlist_id) = open {
            self.apply_playlist(playlist_id);
        }
    }

    /// 侧栏「新建播放集」。和 Vue `openPlaylistDialog` 一样：没有仓库或仓库丢失时不开；
    /// 每次打开都清空名称，类型默认选第一个可用的播放器类型。
    pub(crate) fn open_playlist_dialog(&mut self) {
        if self.workspace.active_repo_id.is_none() || self.navigation_locked() {
            eprintln!("Nana 当前不能新建播放集");
            return;
        }
        self.new_playlist_name.clear();
        self.selected_new_playlist_player_type_id = self.playlist_players.first().map(|player| player.player_type_id.clone());
        self.playlist_dialog_error.clear();
        self.playlist_dialog_open = true;
    }

    /// 新建播放集的「取消」、遮罩和 Escape。请求在途时不关，和别的对话框「处理中不能关」一致。
    pub(crate) fn close_playlist_dialog(&mut self) {
        if self.playlist_creating {
            eprintln!("Nana 新建播放集处理中，不能关闭对话框");
            return;
        }
        self.playlist_dialog_open = false;
        self.playlist_dialog_error.clear();
    }

    /// 新建播放集的「创建」和回车。名称为空或没有选类型时不建（Vue `playlistDialogDisabled`），
    /// 请求在途时不重复提交；对话框等结果回来再关（Vue `submitPlaylistDialog` 等 `createPlaylistInWorkspace`）。
    pub(crate) fn submit_playlist_dialog(&mut self) {
        let name = self.new_playlist_name.trim().to_string();
        let Some(player_type_id) = self.selected_new_playlist_player_type_id.clone().filter(|_| !name.is_empty()) else {
            eprintln!("Nana 新建播放集缺少名称或播放类型，不提交");
            return;
        };
        if self.playlist_creating {
            eprintln!("Nana 新建播放集已在处理中，不重复提交");
            return;
        }
        let Some(repo_id) = self.workspace.active_repo_id.clone() else {
            eprintln!("Nana 新建播放集没有活动仓库");
            return;
        };
        self.playlist_creating = true;
        self.playlist_dialog_error.clear();
        self.sidebar.effects.push(SidebarEffect::CreatePlaylist { repo_id, name, player_type_id });
    }

    /// 新建播放集的结果。成功关掉对话框、换上整份列表并照 Vue 接着点开新建的那个；失败留着对话框，
    /// 原因写在对话框里（Vue 这里的失败没有界面反馈）。
    pub(crate) fn note_playlist_created(&mut self, repo_id: &str, result: Result<(Vec<PlaylistSummary>, Option<String>), String>) {
        self.playlist_creating = false;
        match result {
            Ok((playlists, open)) => {
                self.playlist_dialog_open = false;
                self.playlist_dialog_error.clear();
                self.apply_playlist_list(repo_id, &playlists, open);
            }
            Err(error) => {
                eprintln!("Nana 新建播放集失败：{error}");
                self.playlist_dialog_error = error;
            }
        }
    }

    pub(crate) fn apply_snapshot_sidebar(&mut self, snapshot: &crate::backend::services::repository::RepositorySnapshot) {
        self.bind_sidebar_repository();
        let assets = snapshot.assets.iter().map(|asset| ShortcutAsset {
            path: asset.path.clone(),
            untagged: asset.tags.is_empty(),
            accessed: asset.last_accessed_at.is_some(),
            deleted: asset.status == "deleted",
        }).collect::<Vec<_>>();
        let quick_access = snapshot.quick_access.iter().map(|shortcut| SidebarShortcut {
            id: shortcut.shortcut_id.clone(),
            label: shortcut.label.clone(),
            target_kind: shortcut.target_kind.clone(),
            target_path: shortcut.target_path.clone(),
            target_id: shortcut.target_id.clone(),
        }).collect();
        self.sidebar.apply_snapshot(&assets, snapshot.overview.trash_count, quick_access);
    }

    /// 只在最近使用视图、有记录且未锁定时清空访问历史。
    pub(crate) fn clear_recent_access(&mut self) {
        if self.navigation_locked() {
            eprintln!("Nana 当前不能清空最近使用");
            return;
        }
        if self.workspace.panel != WorkspacePanel::Files || self.workspace.library_category != LibraryCategory::Recent {
            eprintln!("Nana 只有最近使用视图可以清空记录");
            return;
        }
        if self.sidebar.counts.recent == 0 {
            return;
        }
        let Some(repo_id) = self.workspace.active_repo_id.clone() else {
            eprintln!("Nana 清空最近使用没有活动仓库");
            return;
        };
        self.sidebar.effects.push(SidebarEffect::ClearRecent { repo_id });
    }

    /// 清空成功后计数归零，并静默重读仓库摘要。目录列表不在这里清空。
    pub(crate) fn note_recent_cleared(&mut self, repo_id: &str, result: Result<usize, String>) {
        if self.workspace.active_repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 忽略过期的最近使用清空：{repo_id}");
            return;
        }
        match result {
            Ok(_) => {
                self.sidebar.counts.recent = 0;
                self.workspace.effects.push(WorkspaceEffect::LoadSnapshotSilent { repo_id: repo_id.to_string() });
            }
            Err(error) => {
                eprintln!("Nana 清空最近使用失败：{error}");
                self.files.error = error;
            }
        }
    }
}
