//! 侧栏消息落到壳层页面上的那一层。

use super::super::workspace::MainRegion;
use super::super::{ShellPage, ShellViewModel};
use super::{ShortcutAsset, ShortcutId, SidebarShortcut, WorkspacePanel};

impl ShellViewModel {
    pub(crate) fn navigation_locked(&self) -> bool {
        self.workspace.main_region() == MainRegion::MissingRepository
            || self.workspace.active_repository().is_some_and(|repository| repository.status == "missing")
    }

    /// 按当前活动仓库重新绑定侧栏。缺失和空仓库只清空，不发请求。
    pub(crate) fn bind_sidebar_repository(&mut self) {
        let missing = self.navigation_locked();
        let repo_id = self.workspace.active_repo_id.clone();
        self.sidebar.bind_repository(repo_id.as_deref(), missing);
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
            self.detail = if self.workspace.panel == WorkspacePanel::Trash {
                "正在读取回收站…".into()
            } else {
                format!("当前分类 {}", id.label())
            };
        }
    }

    pub(crate) fn apply_quick_access(&mut self, shortcut_id: String) {
        let locked = self.navigation_locked();
        if self.sidebar.open_quick_access(&mut self.workspace, &shortcut_id, locked) {
            self.leave_settings_page();
            self.selected_path = self.sidebar.selected_path.clone();
            self.current_directory = self.sidebar.current_directory.clone();
            self.detail = self.sidebar.selected_path.clone().unwrap_or_else(|| self.current_directory.clone());
        }
    }

    /// 侧栏浏览已经交给这一帧派发。文件列表先切到该目录，快照回来后再换行。
    pub(crate) fn note_sidebar_browse(&mut self, path: &str) {
        self.files.current_path = path.to_string();
        self.files.loading = true;
        self.files.activity = "正在读取目录…".into();
    }

    /// 把本帧新产生的目录浏览移出普通队列，并让文件列表进入该目录。
    pub(crate) fn stage_browses(&mut self) {
        for effect in self.sidebar.take_browses() {
            if let super::SidebarEffect::Browse { path, .. } = &effect {
                self.note_sidebar_browse(path);
            }
            self.staged_browses.push(effect);
        }
    }

    pub(crate) fn apply_open_folder(&mut self, path: String) {
        let locked = self.navigation_locked();
        if self.sidebar.open_folder(&mut self.workspace, &path, locked) {
            self.leave_settings_page();
            self.current_directory = self.sidebar.current_directory.clone();
            self.detail = format!("正在读取目录 {path}…");
            self.surface_dirty = true;
        }
    }

    pub(crate) fn apply_smart_folder(&mut self, smart_folder_id: String) {
        let locked = self.navigation_locked();
        if self.sidebar.select_smart_folder(&mut self.workspace, &smart_folder_id, locked) {
            self.leave_settings_page();
            self.detail = format!("正在查询智能文件夹 {smart_folder_id}…");
        }
    }

    pub(crate) fn apply_playlist(&mut self, playlist_id: String) {
        if self.sidebar.select_playlist(&mut self.workspace, &playlist_id) {
            self.leave_settings_page();
            self.selected_playlist_id = Some(playlist_id.clone());
            self.detail = format!("正在读取播放集 {playlist_id}…");
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
}
