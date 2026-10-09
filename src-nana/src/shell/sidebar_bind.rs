//! 侧栏消息落到壳层页面上的那一层。

use super::super::workspace::{LibraryCategory, MainRegion, WorkspaceEffect};
use super::super::{ShellPage, ShellViewModel};
use super::{ShortcutAsset, ShortcutId, SidebarEffect, SidebarShortcut, WorkspacePanel};

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

    /// 快照返回前，文件列表先停在正在打开的目录上。
    pub(crate) fn note_sidebar_browse(&mut self, path: &str) {
        self.files.current_path = path.to_string();
        self.files.loading = true;
        self.files.activity = "正在读取目录…".into();
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
            self.detail = format!("正在读取目录 {path}…");
            self.mark_surface_dirty();
        }
    }

    pub(crate) fn apply_smart_folder(&mut self, smart_folder_id: String) {
        let locked = self.navigation_locked();
        if self.sidebar.select_smart_folder(&mut self.workspace, &smart_folder_id, locked) {
            self.leave_settings_page();
            self.detail = format!("正在查询智能文件夹 {smart_folder_id}…");
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
                self.page = ShellPage::Settings;
                self.reduce(super::super::ShellMessage::Admin(super::super::AdminMessage::RoutePlugin(plugin_id)));
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
            Ok(count) => {
                self.sidebar.counts.recent = 0;
                self.files.activity = format!("已清空最近使用 {count} 条。");
                self.workspace.effects.push(WorkspaceEffect::LoadSnapshotSilent { repo_id: repo_id.to_string() });
            }
            Err(error) => {
                eprintln!("Nana 清空最近使用失败：{error}");
                self.files.error = error;
            }
        }
    }
}
