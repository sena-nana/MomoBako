//! 智能文件夹增改删和查询的结果落到壳层上。
//!
//! 照 Vue `smartFolders.ts`：新建以后打开新建的那个，编辑以后重查正在看的结果，删掉正在看的（连同它的
//! 祖先）以后回到文件面板。查询结果记下文件夹名、和祖先合并以后的筛选与条数，文件页头据此写标题和摘要行
//! （Vue `useWorkspaceViewState.ts` 的 `virtualViewTitle`、`smartFolderSummary`）。
//!
//! 和 Vue 不同的地方（Vue 的缺陷不照抄）：编辑了正在看的文件夹的祖先（继承的条件跟着变）或者把它挪了位置
//! 也重查；只在智能文件夹面板开着时重查，不把在别的面板上的用户拉回来；删除以后只有正在看的被删了才回文件
//! 面板，Vue 在没有打开的智能文件夹时删掉任何一个都会切到文件面板。

use crate::backend::services::repository::SmartFolderFilter;

use super::super::files::VirtualQuery;
use super::super::status::FailureSource;
use super::super::{ShellViewModel, WorkspacePanel};
use super::smart::format_number;
use super::{smart_ancestor_ids, SidebarEffect, SidebarSmartFolder, SidebarState};

/// 没有打开的智能文件夹、也找不到名字时文件页头的标题（Vue `virtualViewTitle` 的回退）。
const FALLBACK_TITLE: &str = "智能文件夹";

/// 一次增改删改的是哪个智能文件夹。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SmartFolderChange {
    /// 新建；带服务交回的新文件夹标识，没交回时为 `None`。
    Created(Option<String>),
    Updated(String),
    Deleted(String),
}

/// 增改删成功以后交回的东西：整棵智能文件夹树，和这次改的是哪个。
#[derive(Clone, Debug)]
pub struct SmartFolderOutcome {
    pub folders: Vec<SidebarSmartFolder>,
    pub change: SmartFolderChange,
}

/// 一次查询的结果：交给文件列表的只读行，加上文件夹名和与祖先合并以后的筛选（服务 `inheritedFilter`）。
#[derive(Clone, Debug)]
pub struct SmartFolderResult {
    pub name: String,
    pub filter: SmartFolderFilter,
    pub query: VirtualQuery,
}

/// 最近一次查询结果的摘要，文件页头按它写标题和摘要行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartResultSummary {
    pub smart_folder_id: String,
    pub name: String,
    pub count: usize,
    /// 「N 条结果 · 关键词 … · 格式 …」，见 [`summary_text`]。
    pub text: String,
}

impl SmartResultSummary {
    /// 从一次查询结果算摘要。
    pub fn of(smart_folder_id: &str, result: &SmartFolderResult) -> Self {
        Self {
            smart_folder_id: smart_folder_id.to_string(),
            name: result.name.clone(),
            count: result.query.count,
            text: summary_text(result.query.count, &result.filter),
        }
    }
}

/// 摘要行，照 Vue `smartFolderSummary`：「N 条结果」，后面按关键词、路径、格式、标签、颜色、形状、最低评分、
/// 元数据条件的顺序接上有值的几项，用「 · 」隔开。祖先和自己的关键词、路径合在一起时是多行，用「 + 」连。
pub fn summary_text(count: usize, filter: &SmartFolderFilter) -> String {
    let text = |value: &Option<String>| value.as_deref().filter(|value| !value.is_empty()).map(|value| value.replace('\n', " + "));
    let list = |values: &Option<Vec<String>>| values.as_ref().filter(|values| !values.is_empty()).map(|values| values.join(" / "));
    let parts = [
        text(&filter.query).map(|value| format!("关键词 {value}")),
        text(&filter.path_prefix).map(|value| format!("路径 {value}")),
        list(&filter.formats).map(|value| format!("格式 {value}")),
        list(&filter.tags).map(|value| format!("标签 {value}")),
        list(&filter.colors).map(|value| format!("颜色 {value}")),
        list(&filter.shapes).map(|value| format!("形状 {value}")),
        filter.min_rating.filter(|rating| rating.is_finite() && *rating > 0.0).map(|rating| format!("{} 星+", format_number(rating))),
        filter.metadata_filters.as_ref().map(Vec::len).filter(|count| *count > 0).map(|count| format!("{count} 个元数据条件")),
    ];
    let mut summary = format!("{count} 条结果");
    for part in parts.into_iter().flatten() {
        summary.push_str(" · ");
        summary.push_str(&part);
    }
    summary
}

impl SidebarState {
    /// 文件页头的标题：正在看的智能文件夹的名字。在整棵树里找（子级也找得到）；树里还没有时用查询结果
    /// 带回的名字，都没有时写「智能文件夹」。
    pub fn smart_title(&self) -> String {
        let Some(active) = self.active_smart_folder_id.as_deref() else {
            return FALLBACK_TITLE.into();
        };
        find_folder(&self.smart_folders, active)
            .map(|folder| folder.name.clone())
            .or_else(|| self.smart_result.as_ref().filter(|summary| summary.smart_folder_id == active).map(|summary| summary.name.clone()))
            .unwrap_or_else(|| FALLBACK_TITLE.into())
    }

    /// 文件页头的摘要行：正在看的这一个查回来以后才有；换了文件夹、新的还没查回来时为空，不写上一个的。
    pub fn smart_summary(&self) -> String {
        let active = self.active_smart_folder_id.as_deref();
        self.smart_result
            .as_ref()
            .filter(|summary| Some(summary.smart_folder_id.as_str()) == active)
            .map(|summary| summary.text.clone())
            .unwrap_or_default()
    }

    /// 重查正在看的智能文件夹。
    fn requery_active(&mut self, repo_id: &str) {
        let Some(smart_folder_id) = self.active_smart_folder_id.clone() else {
            return;
        };
        self.smart_loading = true;
        self.smart_error.clear();
        self.effects.push(SidebarEffect::QuerySmartFolder { repo_id: repo_id.to_string(), smart_folder_id });
    }
}

impl ShellViewModel {
    /// 新建、编辑、删除的结果（Vue `createSmartFolderInWorkspace` / `updateSmartFolderInWorkspace` /
    /// `deleteSmartFolderInWorkspace`）。换上整棵树、关掉对话框以后：新建的打开它，编辑的重查正在看的结果，
    /// 删除的清掉被删的结果。失败和过期的结果留在对话框里；删除的确认框没有错误行，失败写进状态区。
    pub(crate) fn apply_smart_saved(&mut self, repo_id: &str, result: Result<SmartFolderOutcome, String>) {
        let (folders, change) = match result {
            Ok(outcome) => (Ok(outcome.folders), Some(outcome.change)),
            Err(error) => (Err(error), None),
        };
        if !self.sidebar.note_smart_saved(repo_id, folders) {
            let draft = &self.sidebar.smart_draft;
            if !draft.open && !draft.error.is_empty() {
                let failure = format!("智能文件夹操作失败：{}", draft.error);
                self.status.fail(FailureSource::SmartFolder, failure);
            }
            return;
        }
        match change {
            Some(SmartFolderChange::Created(Some(id))) => self.apply_smart_folder(id),
            Some(SmartFolderChange::Created(None)) => eprintln!("Nana 新建智能文件夹的结果没有带回新文件夹，不打开"),
            Some(SmartFolderChange::Updated(id)) => self.requery_edited_smart(repo_id, &id),
            Some(SmartFolderChange::Deleted(id)) => self.leave_deleted_smart(&id),
            None => {}
        }
    }

    /// 编辑以后：智能文件夹面板开着、正在看的就是它或者它的子孙（继承的条件跟着变）时重查。
    fn requery_edited_smart(&mut self, repo_id: &str, edited: &str) {
        if self.workspace.panel != WorkspacePanel::SmartFolder {
            return;
        }
        let Some(active) = self.sidebar.active_smart_folder_id.clone() else {
            return;
        };
        let affected = active == edited || smart_ancestor_ids(&self.sidebar.smart_folders, &active).iter().any(|id| id == edited);
        if affected {
            self.sidebar.requery_active(repo_id);
        }
    }

    /// 删除以后：正在看的（删的是它或它的祖先）已经不在树里了，清掉它和它的结果；智能文件夹面板开着时回到
    /// 文件面板，列表照旧是上一次读的目录（和 Vue 回到文件面板一样，不另读目录）。
    fn leave_deleted_smart(&mut self, deleted: &str) {
        let Some(active) = self.sidebar.active_smart_folder_id.clone() else {
            return;
        };
        if find_folder(&self.sidebar.smart_folders, &active).is_some() {
            return;
        }
        eprintln!("Nana 正在看的智能文件夹随 {deleted} 一起删掉了，清掉它的结果：{active}");
        self.sidebar.active_smart_folder_id = None;
        self.sidebar.smart_result = None;
        self.sidebar.smart_loading = false;
        self.sidebar.smart_error.clear();
        self.files.set_virtual_rows(Vec::new());
        if self.workspace.panel == WorkspacePanel::SmartFolder {
            self.workspace.panel = WorkspacePanel::Files;
        }
    }

    /// 查询结果（Vue `selectSmartFolder`）：仓库或正在看的文件夹已经换了的不写；写下摘要，把行交给文件列表。
    pub(crate) fn apply_smart_query(&mut self, repo_id: &str, smart_folder_id: &str, result: Result<SmartFolderResult, String>) {
        let summary = match &result {
            Ok(result) => Ok(SmartResultSummary::of(smart_folder_id, result)),
            Err(error) => Err(error.clone()),
        };
        if self.sidebar.note_smart_query(repo_id, smart_folder_id, summary) {
            self.files.set_virtual_rows(result.map(|result| result.query.rows).unwrap_or_default());
        }
    }
}

/// 在整棵智能文件夹树里按标识找。
fn find_folder<'a>(folders: &'a [SidebarSmartFolder], id: &str) -> Option<&'a SidebarSmartFolder> {
    folders.iter().find_map(|folder| if folder.id == id { Some(folder) } else { find_folder(&folder.children, id) })
}

#[cfg(test)]
#[path = "sidebar_smart_flow_tests.rs"]
mod tests;
