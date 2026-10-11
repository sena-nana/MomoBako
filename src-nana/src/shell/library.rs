//! 仓库摘要里的素材：侧栏快捷方式的计数，以及访问记录的当场更新。
//!
//! 对应 Vue `activeSnapshot.assets`。计数照 `useSidebarShortcutsUi.ts`：只看没删的素材，未分类是路径里没有 `/`，
//! 未标签是没有标签，最近使用是有访问时间。
//!
//! 预览、播放和「打开」记下访问以后照 Vue `patchActiveSnapshotAssetAccess` 当场改这一条的访问时间，只留最近的
//! 50 条（和后端 `MAX_RECENT_ACCESS_ENTRIES` 一样），侧栏「最近使用」的计数跟着变，不重读摘要；「清空记录」成功照
//! `clearActiveSnapshotRecentAccess` 全部清掉。后端已经写了访问时间的（预览源、在线条目的播放源）只改这里；本地
//! 文件直接读字节、后端没有经手的，排一次 `record_entry_access`，回来再改这里。分类视图的列表不从这里取。

use std::cmp::Ordering;

use crate::backend::services::repository::{AssetSummary, RepositorySnapshot};

use super::search_collate::compare_zh;
use super::sidebar::{ShortcutAsset, SidebarShortcut};
use super::ShellViewModel;

/// 每个资源库只留最近这么多条访问时间，和后端 `MAX_RECENT_ACCESS_ENTRIES` 一样。
const RECENT_LIMIT: usize = 50;

/// 交给宿主的访问记录请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LibraryEffect {
    /// 本地文件直接读了字节（后端没有经手），或者用系统程序打开了它：写一次访问时间。
    RecordAccess { repo_id: String, path: String },
}

/// 当前资源库摘要里的素材。
#[derive(Clone, Debug, Default)]
pub struct LibraryAssets {
    repo_id: Option<String>,
    assets: Vec<AssetSummary>,
    effects: Vec<LibraryEffect>,
}

impl LibraryAssets {
    pub(crate) fn take_effects(&mut self) -> Vec<LibraryEffect> {
        std::mem::take(&mut self.effects)
    }

    /// 换上一份仓库摘要的素材。
    pub(super) fn replace(&mut self, snapshot: &RepositorySnapshot) {
        self.repo_id = Some(snapshot.repository.repo_id.clone());
        self.assets = snapshot.assets.clone();
    }

    /// 换仓库或列表空了：上一个仓库的素材不留。
    pub(super) fn clear(&mut self) {
        self.repo_id = None;
        self.assets.clear();
    }

    /// 照 Vue `patchActiveSnapshotAssetAccess` 记下一次访问：别的仓库、摘要里没有这一条（或已经删掉）时不动；
    /// 改完只留最近的 50 条访问时间。返回素材有没有变。
    pub(super) fn note_access(&mut self, repo_id: &str, path: &str, recorded_at: &str) -> bool {
        if self.repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 访问记录不是当前资源库的，忽略：{repo_id} {path}");
            return false;
        }
        let path = normalize_path(path);
        let Some(asset) = self.assets.iter_mut().find(|asset| asset.path == path && asset.status != "deleted") else {
            eprintln!("Nana 访问记录的条目不在资源库摘要里，忽略：{path}");
            return false;
        };
        asset.last_accessed_at = Some(recorded_at.to_string());
        self.prune_recent();
        true
    }

    /// 照 Vue `clearActiveSnapshotRecentAccess`：「清空记录」成功以后去掉所有访问时间。返回素材有没有变。
    pub(super) fn clear_access(&mut self, repo_id: &str) -> bool {
        if self.repo_id.as_deref() != Some(repo_id) || !self.assets.iter().any(accessed) {
            return false;
        }
        for asset in &mut self.assets {
            asset.last_accessed_at = None;
        }
        true
    }

    /// 侧栏快捷方式计数要的字段。
    pub(super) fn shortcut_assets(&self) -> Vec<ShortcutAsset> {
        self.assets
            .iter()
            .map(|asset| ShortcutAsset {
                path: asset.path.clone(),
                untagged: asset.tags.is_empty(),
                accessed: accessed(asset),
                deleted: asset.status == "deleted",
            })
            .collect()
    }

    /// 算法：有访问时间的素材按 Vue `compareRecentAssetAccess` 排好，第 50 条以后的访问时间清掉，和后端写入时的
    /// 修剪一致。
    fn prune_recent(&mut self) {
        let mut ranked: Vec<usize> = (0..self.assets.len()).filter(|index| accessed(&self.assets[*index])).collect();
        if ranked.len() <= RECENT_LIMIT {
            return;
        }
        ranked.sort_by(|left, right| compare_recent(&self.assets[*left], &self.assets[*right]));
        for index in ranked.into_iter().skip(RECENT_LIMIT) {
            self.assets[index].last_accessed_at = None;
        }
    }
}

fn accessed(asset: &AssetSummary) -> bool {
    asset.last_accessed_at.as_deref().is_some_and(|value| !value.trim().is_empty())
}

/// Vue `compareRecentAssetAccess`：访问时间倒序，再修改时间倒序，再文件名的中文顺序。时间是 RFC 3339 原文，
/// 和 Vue、后端一样按文本比。
fn compare_recent(left: &AssetSummary, right: &AssetSummary) -> Ordering {
    let left_access = left.last_accessed_at.as_deref().unwrap_or_default();
    let right_access = right.last_accessed_at.as_deref().unwrap_or_default();
    right_access
        .cmp(left_access)
        .then_with(|| right.modified_at.cmp(&left.modified_at))
        .then_with(|| compare_zh(&left.filename, &right.filename))
}

/// 和摘要里的路径同一种写法：`/` 分隔、去掉首尾斜杠。
fn normalize_path(path: &str) -> String {
    path.trim().replace('\\', "/").trim_matches('/').to_string()
}

impl ShellViewModel {
    /// 仓库摘要到了（启动、刷新、静默重读、刷新文件夹树）：换上素材，重算侧栏计数，换上快捷访问。
    pub(super) fn note_library_snapshot(&mut self, snapshot: &RepositorySnapshot) {
        self.library.replace(snapshot);
        let quick_access = snapshot
            .quick_access
            .iter()
            .map(|shortcut| SidebarShortcut {
                id: shortcut.shortcut_id.clone(),
                label: shortcut.label.clone(),
                target_kind: shortcut.target_kind.clone(),
                target_path: shortcut.target_path.clone(),
                target_id: shortcut.target_id.clone(),
            })
            .collect();
        self.sidebar.apply_snapshot(&self.library.shortcut_assets(), snapshot.overview.trash_count, quick_access);
    }

    /// 后端已经写下一次访问（预览源、在线条目的播放源、`record_entry_access`）：当场改素材，侧栏「最近使用」的
    /// 计数跟着变。
    pub(super) fn note_entry_access(&mut self, repo_id: &str, path: &str, recorded_at: &str) {
        if self.library.note_access(repo_id, path, recorded_at) {
            self.sidebar.recount(&self.library.shortcut_assets());
        }
    }

    /// 「清空记录」成功：素材的访问时间全部清掉，「最近使用」的计数当场归零。
    pub(super) fn clear_library_access(&mut self, repo_id: &str) {
        if self.library.clear_access(repo_id) {
            self.sidebar.recount(&self.library.shortcut_assets());
        }
    }

    /// 本地文件读好了字节，或者交给系统程序打开：后端没有经手，排一次访问记录。路径为空时不排。
    pub(super) fn request_entry_access(&mut self, repo_id: &str, path: &str) {
        let path = normalize_path(path);
        if repo_id.trim().is_empty() || path.is_empty() {
            eprintln!("Nana 访问记录缺少资源库或路径，不记：{repo_id} {path}");
            return;
        }
        self.library.effects.push(LibraryEffect::RecordAccess { repo_id: repo_id.to_string(), path });
    }

    /// 访问记录的结果。成功当场改素材；失败只记日志，不挡住打开和预览。
    pub(super) fn note_access_recorded(&mut self, repo_id: &str, path: &str, result: Result<String, String>) {
        match result {
            Ok(recorded_at) => self.note_entry_access(repo_id, path, &recorded_at),
            Err(error) => eprintln!("Nana 记录访问失败：{path}：{error}"),
        }
    }
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
