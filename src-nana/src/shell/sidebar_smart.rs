//! 智能文件夹新建。名称必填，筛选留空时创建的是不带条件的文件夹。
//!
//! 加号只打开对话框。提交后把请求放进侧栏副作用，由宿主调用已有的创建接口。

use super::{SidebarEffect, SidebarSmartFolder, SidebarState};

/// 新建对话框里的文本字段。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmartFolderField {
    Name,
    Parent,
    Query,
    Path,
    Formats,
    Tags,
    Match,
}

/// 新建智能文件夹的草稿。匹配方式默认全部满足。
#[derive(Clone, Debug)]
pub struct SmartFolderDraft {
    pub open: bool,
    pub busy: bool,
    pub error: String,
    pub parent_id: String,
    pub name: String,
    pub query: String,
    pub path_prefix: String,
    pub formats: String,
    pub tags: String,
    pub match_mode: String,
}

impl Default for SmartFolderDraft {
    fn default() -> Self {
        Self {
            open: false,
            busy: false,
            error: String::new(),
            parent_id: String::new(),
            name: String::new(),
            query: String::new(),
            path_prefix: String::new(),
            formats: String::new(),
            tags: String::new(),
            match_mode: "and".into(),
        }
    }
}

impl SidebarState {
    /// 打开空白新建对话框。正在提交时不重置。
    pub fn open_smart_dialog(&mut self) {
        if self.smart_draft.busy {
            return;
        }
        let parent = std::mem::take(&mut self.smart_draft.parent_id);
        self.smart_draft = SmartFolderDraft { open: true, parent_id: parent, ..SmartFolderDraft::default() };
    }

    pub fn close_smart_dialog(&mut self) {
        if self.smart_draft.busy {
            return;
        }
        self.smart_draft = SmartFolderDraft::default();
    }

    pub fn set_smart_field(&mut self, field: SmartFolderField, value: String) {
        if self.smart_draft.busy {
            return;
        }
        let slot = match field {
            SmartFolderField::Name => &mut self.smart_draft.name,
            SmartFolderField::Parent => &mut self.smart_draft.parent_id,
            SmartFolderField::Query => &mut self.smart_draft.query,
            SmartFolderField::Path => &mut self.smart_draft.path_prefix,
            SmartFolderField::Formats => &mut self.smart_draft.formats,
            SmartFolderField::Tags => &mut self.smart_draft.tags,
            SmartFolderField::Match => &mut self.smart_draft.match_mode,
        };
        *slot = value;
    }

    /// 名称和仓库都有效时才发出创建请求。
    pub fn submit_smart_folder(&mut self, repo_id: Option<&str>) {
        if self.smart_draft.busy {
            return;
        }
        let Some(repo_id) = repo_id.map(str::trim).filter(|id| !id.is_empty()) else {
            self.smart_draft.error = "先选择一个资源库。".into();
            eprintln!("Nana 新建智能文件夹时没有活动仓库");
            return;
        };
        if self.smart_draft.name.trim().is_empty() {
            self.smart_draft.error = "名称不能为空".into();
            return;
        }
        self.smart_draft.error.clear();
        self.smart_draft.busy = true;
        self.effects.push(SidebarEffect::CreateSmartFolder { repo_id: repo_id.to_string() });
    }

    /// 把当前草稿收成创建请求。文件夹标识由服务分配。
    pub fn smart_create_request(&self, repo_id: &str) -> crate::backend::services::repository::SmartFolderMutationRequest {
        crate::backend::services::repository::SmartFolderMutationRequest {
            repo_id: repo_id.to_string(),
            smart_folder_id: None,
            parent_id: parent_id(&self.smart_draft),
            name: self.smart_draft.name.trim().to_string(),
            filter: filter_from_draft(&self.smart_draft),
        }
    }

    /// 创建返回整棵树。成功后关闭对话框，并展开父级。
    pub fn note_smart_saved(&mut self, repo_id: &str, result: Result<Vec<SidebarSmartFolder>, String>) {
        self.smart_draft.busy = false;
        if self.bound_repo_id.as_deref() != Some(repo_id) {
            eprintln!("Nana 忽略过期的智能文件夹创建结果：{repo_id}");
            self.smart_draft.error = "智能文件夹结果已过期".into();
            return;
        }
        match result {
            Ok(folders) => {
                let parent = self.smart_draft.parent_id.trim().to_string();
                self.apply_smart_folders(repo_id, Ok(folders));
                if !parent.is_empty() && !self.expanded_smart_folders.iter().any(|id| id == &parent) {
                    self.expanded_smart_folders.push(parent);
                }
                self.smart_draft = SmartFolderDraft::default();
            }
            Err(error) => {
                eprintln!("Nana 新建智能文件夹失败：{error}");
                self.smart_draft.error = error;
            }
        }
    }
}

/// 把对话框文本收成服务筛选。空白字段不写入。
pub fn filter_from_draft(draft: &SmartFolderDraft) -> crate::backend::services::repository::SmartFolderFilter {
    let mut filter = crate::backend::services::repository::SmartFolderFilter::default();
    filter.query = filled(&draft.query);
    filter.path_prefix = filled(&draft.path_prefix);
    filter.formats = split_list(&draft.formats);
    filter.tags = split_list(&draft.tags);
    filter.match_mode = Some(if draft.match_mode == "or" { "or" } else { "and" }.into());
    filter
}

pub fn parent_id(draft: &SmartFolderDraft) -> Option<String> {
    filled(&draft.parent_id)
}

fn filled(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// 逗号、顿号和换行都当成列表分隔。
fn split_list(value: &str) -> Option<Vec<String>> {
    let items: Vec<String> = value
        .split([',', '，', '、', '\n'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect();
    (!items.is_empty()).then_some(items)
}
