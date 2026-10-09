//! 智能文件夹新建和编辑的草稿。
//!
//! 字段和 Vue `WorkspaceSidebarSmartFolderDialogs.vue` 一一对应；文本字段的拆分和解析照
//! `composables/workspace/filterInputs.ts`，提交时由 [`filter_from_draft`] 收成服务筛选。
//! 名称必填，其余留空时不写进筛选。加号只打开对话框，提交后把请求放进侧栏副作用。

use crate::backend::services::repository::{
    SearchDateFilter, SearchMetadataFilter, SearchNumberFilter, SearchSort, SmartFolderFilter,
};

use super::{SidebarEffect, SidebarSmartFolder, SidebarState};

/// 对话框里的字段。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmartFolderField {
    Name,
    Parent,
    Query,
    Path,
    Formats,
    Tags,
    Colors,
    Shapes,
    MinRating,
    Match,
    Metadata,
    ExcludeQuery,
    ExcludePaths,
    ExcludeTags,
    ExcludeFormats,
    SortField,
    SortDirection,
    Limit,
    ExcludeMetadata,
    ExcludeNumbers,
    ExcludeDates,
    Numbers,
    Dates,
}

/// 智能文件夹草稿。匹配方式默认全部满足，排序默认升序。
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
    pub colors: String,
    pub shapes: String,
    pub min_rating: String,
    pub match_mode: String,
    pub metadata: String,
    pub exclude_query: String,
    pub exclude_paths: String,
    pub exclude_tags: String,
    pub exclude_formats: String,
    pub sort_field: String,
    pub sort_direction: String,
    pub limit: String,
    pub exclude_metadata: String,
    pub exclude_numbers: String,
    pub exclude_dates: String,
    pub numbers: String,
    pub dates: String,
    pub mode_edit: bool,
    pub target_id: String,
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
            colors: String::new(),
            shapes: String::new(),
            min_rating: String::new(),
            match_mode: "and".into(),
            metadata: String::new(),
            exclude_query: String::new(),
            exclude_paths: String::new(),
            exclude_tags: String::new(),
            exclude_formats: String::new(),
            sort_field: String::new(),
            sort_direction: "asc".into(),
            limit: String::new(),
            exclude_metadata: String::new(),
            exclude_numbers: String::new(),
            exclude_dates: String::new(),
            numbers: String::new(),
            dates: String::new(),
            mode_edit: false,
            target_id: String::new(),
        }
    }
}

impl SmartFolderDraft {
    /// 编辑已有智能文件夹：把筛选写回文本，对应 Vue `openEditSmartFolderDialog`。
    pub fn from_folder(folder: &SidebarSmartFolder) -> Self {
        let filter = &folder.filter;
        Self {
            open: true,
            mode_edit: true,
            target_id: folder.id.clone(),
            parent_id: folder.parent_id.clone().unwrap_or_default(),
            name: folder.name.clone(),
            query: filter.query.clone().unwrap_or_default(),
            path_prefix: filter.path_prefix.clone().unwrap_or_default(),
            formats: join_list(filter.formats.as_deref()),
            tags: join_list(filter.tags.as_deref()),
            colors: join_list(filter.colors.as_deref()),
            shapes: join_list(filter.shapes.as_deref()),
            min_rating: filter.min_rating.map(format_number).unwrap_or_default(),
            match_mode: if filter.match_mode.as_deref() == Some("or") { "or" } else { "and" }.into(),
            metadata: format_metadata(filter.metadata_filters.as_deref()),
            exclude_query: filter.exclude_query.clone().unwrap_or_default(),
            exclude_paths: join_list(filter.exclude_path_prefixes.as_deref()),
            exclude_tags: join_list(filter.exclude_tags.as_deref()),
            exclude_formats: join_list(filter.exclude_formats.as_deref()),
            sort_field: filter.sort.as_ref().map(|sort| sort.field.clone()).unwrap_or_default(),
            sort_direction: if filter.sort.as_ref().is_some_and(|sort| sort.direction == "desc") { "desc" } else { "asc" }.into(),
            limit: filter.limit.map(|limit| limit.to_string()).unwrap_or_default(),
            exclude_metadata: format_metadata(filter.exclude_metadata_filters.as_deref()),
            exclude_numbers: format_numbers(filter.exclude_number_filters.as_deref()),
            exclude_dates: format_dates(filter.exclude_date_filters.as_deref()),
            numbers: format_numbers(filter.number_filters.as_deref()),
            dates: format_dates(filter.date_filters.as_deref()),
            ..Self::default()
        }
    }

    /// 主按钮文案：新建是「创建」，编辑是「保存」。
    pub fn action_label(&self) -> &'static str {
        if self.mode_edit { "保存" } else { "创建" }
    }

    /// 读一个字段的当前文本，视图用它填输入框。
    pub fn value(&self, field: SmartFolderField) -> &str {
        match field {
            SmartFolderField::Name => &self.name,
            SmartFolderField::Parent => &self.parent_id,
            SmartFolderField::Query => &self.query,
            SmartFolderField::Path => &self.path_prefix,
            SmartFolderField::Formats => &self.formats,
            SmartFolderField::Tags => &self.tags,
            SmartFolderField::Colors => &self.colors,
            SmartFolderField::Shapes => &self.shapes,
            SmartFolderField::MinRating => &self.min_rating,
            SmartFolderField::Match => &self.match_mode,
            SmartFolderField::Metadata => &self.metadata,
            SmartFolderField::ExcludeQuery => &self.exclude_query,
            SmartFolderField::ExcludePaths => &self.exclude_paths,
            SmartFolderField::ExcludeTags => &self.exclude_tags,
            SmartFolderField::ExcludeFormats => &self.exclude_formats,
            SmartFolderField::SortField => &self.sort_field,
            SmartFolderField::SortDirection => &self.sort_direction,
            SmartFolderField::Limit => &self.limit,
            SmartFolderField::ExcludeMetadata => &self.exclude_metadata,
            SmartFolderField::ExcludeNumbers => &self.exclude_numbers,
            SmartFolderField::ExcludeDates => &self.exclude_dates,
            SmartFolderField::Numbers => &self.numbers,
            SmartFolderField::Dates => &self.dates,
        }
    }

    fn slot(&mut self, field: SmartFolderField) -> &mut String {
        match field {
            SmartFolderField::Name => &mut self.name,
            SmartFolderField::Parent => &mut self.parent_id,
            SmartFolderField::Query => &mut self.query,
            SmartFolderField::Path => &mut self.path_prefix,
            SmartFolderField::Formats => &mut self.formats,
            SmartFolderField::Tags => &mut self.tags,
            SmartFolderField::Colors => &mut self.colors,
            SmartFolderField::Shapes => &mut self.shapes,
            SmartFolderField::MinRating => &mut self.min_rating,
            SmartFolderField::Match => &mut self.match_mode,
            SmartFolderField::Metadata => &mut self.metadata,
            SmartFolderField::ExcludeQuery => &mut self.exclude_query,
            SmartFolderField::ExcludePaths => &mut self.exclude_paths,
            SmartFolderField::ExcludeTags => &mut self.exclude_tags,
            SmartFolderField::ExcludeFormats => &mut self.exclude_formats,
            SmartFolderField::SortField => &mut self.sort_field,
            SmartFolderField::SortDirection => &mut self.sort_direction,
            SmartFolderField::Limit => &mut self.limit,
            SmartFolderField::ExcludeMetadata => &mut self.exclude_metadata,
            SmartFolderField::ExcludeNumbers => &mut self.exclude_numbers,
            SmartFolderField::ExcludeDates => &mut self.exclude_dates,
            SmartFolderField::Numbers => &mut self.numbers,
            SmartFolderField::Dates => &mut self.dates,
        }
    }
}

impl SidebarState {
    /// 打开空白新建对话框。`parent_id` 是从某个智能文件夹行上新建子级时的父级。
    pub fn open_smart_dialog(&mut self, parent_id: Option<String>) {
        if self.smart_draft.busy {
            return;
        }
        self.smart_draft = SmartFolderDraft { open: true, parent_id: parent_id.unwrap_or_default(), ..SmartFolderDraft::default() };
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
        *self.smart_draft.slot(field) = value;
    }

    /// 名称和仓库都有效时才发出创建或保存请求。
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
        if self.smart_draft.mode_edit {
            self.effects.push(SidebarEffect::UpdateSmartFolder { repo_id: repo_id.to_string() });
        } else {
            self.effects.push(SidebarEffect::CreateSmartFolder { repo_id: repo_id.to_string() });
        }
    }

    /// 把当前草稿收成保存请求。
    pub fn smart_update_request(&self, repo_id: &str) -> crate::backend::services::repository::SmartFolderUpdateRequest {
        crate::backend::services::repository::SmartFolderUpdateRequest {
            repo_id: repo_id.to_string(),
            smart_folder_id: self.smart_draft.target_id.clone(),
            parent_id: parent_id(&self.smart_draft),
            name: self.smart_draft.name.trim().to_string(),
            filter: filter_from_draft(&self.smart_draft),
        }
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

    /// 创建和保存都返回整棵树。成功关闭对话框并展开父级，失败把错误留在对话框里。
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
                self.smart_delete_id.clear();
                self.smart_delete_label.clear();
            }
            Err(error) => {
                eprintln!("Nana 保存智能文件夹失败：{error}");
                self.smart_draft.error = error;
            }
        }
    }
}

/// 把对话框文本收成服务筛选，对应 Vue `buildSmartFolderFilter`。空白字段不写入。
pub fn filter_from_draft(draft: &SmartFolderDraft) -> SmartFolderFilter {
    SmartFolderFilter {
        query: filled(&draft.query),
        path_prefix: filled(&draft.path_prefix),
        exclude_query: filled(&draft.exclude_query),
        exclude_path_prefixes: split_list(&draft.exclude_paths),
        tags: split_list(&draft.tags),
        formats: split_list(&draft.formats),
        colors: split_list(&draft.colors),
        shapes: split_list(&draft.shapes),
        metadata_filters: parse_metadata(&draft.metadata),
        exclude_tags: split_list(&draft.exclude_tags),
        exclude_formats: split_list(&draft.exclude_formats),
        exclude_metadata_filters: parse_metadata(&draft.exclude_metadata),
        exclude_number_filters: parse_numbers(&draft.exclude_numbers),
        exclude_date_filters: parse_dates(&draft.exclude_dates),
        number_filters: parse_numbers(&draft.numbers),
        date_filters: parse_dates(&draft.dates),
        min_rating: positive_number(&draft.min_rating),
        match_mode: Some(if draft.match_mode == "or" { "or" } else { "and" }.into()),
        sort: filled(&draft.sort_field).map(|field| SearchSort {
            field,
            direction: if draft.sort_direction == "desc" { "desc" } else { "asc" }.into(),
        }),
        limit: positive_number(&draft.limit).map(|limit| limit as usize),
    }
}

pub fn parent_id(draft: &SmartFolderDraft) -> Option<String> {
    filled(&draft.parent_id)
}

fn filled(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// 有限且大于 0 的数，对应 Vue `Number(x) > 0`。
fn positive_number(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok().filter(|number| number.is_finite() && *number > 0.0)
}

/// Vue `splitListInput`：按半角逗号、全角逗号和换行拆开，去空白、去重，保留顺序。
fn split_list(value: &str) -> Option<Vec<String>> {
    let mut items: Vec<String> = Vec::new();
    for item in value.split([',', '，', '\n']).map(str::trim).filter(|item| !item.is_empty()) {
        if !items.iter().any(|existing| existing == item) {
            items.push(item.to_string());
        }
    }
    (!items.is_empty()).then_some(items)
}

/// 拆成 `key=...` 条目：按换行和逗号分开，去空白。
fn entries(value: &str) -> impl Iterator<Item = &str> {
    value.split(['\n', ',', '，']).map(str::trim).filter(|item| !item.is_empty())
}

/// Vue `parseMetadataFiltersInput`：`key=value`，键和值都不能为空。
fn parse_metadata(value: &str) -> Option<Vec<SearchMetadataFilter>> {
    let filters: Vec<_> = entries(value)
        .filter_map(|item| {
            let (key, filter_value) = item.split_once('=')?;
            let (key, filter_value) = (key.trim(), filter_value.trim());
            (!key.is_empty() && !filter_value.is_empty()).then(|| SearchMetadataFilter { key: key.into(), value: filter_value.into() })
        })
        .collect();
    (!filters.is_empty()).then_some(filters)
}

/// Vue `parseNumberFiltersInput`：`key=min..max`，两端都可空但不能同时为空。
fn parse_numbers(value: &str) -> Option<Vec<SearchNumberFilter>> {
    let bound = |text: &str| text.trim().parse::<f64>().ok().filter(|number| number.is_finite());
    let filters: Vec<_> = entries(value)
        .filter_map(|item| {
            let mut parts = item.split('=');
            let key = parts.next()?.trim();
            let range = parts.next()?.trim();
            if key.is_empty() || range.is_empty() {
                return None;
            }
            let (min, max) = range.split_once("..").unwrap_or((range, ""));
            let filter = SearchNumberFilter { key: key.into(), min: bound(min), max: bound(max) };
            (filter.min.is_some() || filter.max.is_some()).then_some(filter)
        })
        .collect();
    (!filters.is_empty()).then_some(filters)
}

/// Vue `parseDateFiltersInput`：`key=from..to`，两端都可空但不能同时为空。
fn parse_dates(value: &str) -> Option<Vec<SearchDateFilter>> {
    let filters: Vec<_> = entries(value)
        .filter_map(|item| {
            let mut parts = item.split('=');
            let key = parts.next()?.trim();
            let range = parts.next()?.trim();
            if key.is_empty() || range.is_empty() {
                return None;
            }
            let (from, to) = range.split_once("..").unwrap_or((range, ""));
            let filter = SearchDateFilter { key: key.into(), from: filled(from), to: filled(to) };
            (filter.from.is_some() || filter.to.is_some()).then_some(filter)
        })
        .collect();
    (!filters.is_empty()).then_some(filters)
}

fn join_list(values: Option<&[String]>) -> String {
    values.map(|values| values.join("，")).unwrap_or_default()
}

fn format_metadata(filters: Option<&[SearchMetadataFilter]>) -> String {
    filters
        .map(|filters| filters.iter().map(|item| format!("{}={}", item.key, item.value)).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

fn format_numbers(filters: Option<&[SearchNumberFilter]>) -> String {
    let side = |value: Option<f64>| value.map(format_number).unwrap_or_default();
    filters
        .map(|filters| filters.iter().map(|item| format!("{}={}..{}", item.key, side(item.min), side(item.max))).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

fn format_dates(filters: Option<&[SearchDateFilter]>) -> String {
    filters
        .map(|filters| {
            filters
                .iter()
                .map(|item| format!("{}={}..{}", item.key, item.from.clone().unwrap_or_default(), item.to.clone().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// 整数不带小数点，和 JS `String(4)` 一样写成 `4`。
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 { format!("{}", value as i64) } else { value.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_parses_every_vue_field_and_round_trips_through_edit() {
        let draft = SmartFolderDraft {
            name: "高评分".into(),
            query: " cover ".into(),
            formats: "psd，png,psd\n".into(),
            colors: "红色".into(),
            min_rating: "4".into(),
            match_mode: "or".into(),
            metadata: "artist=demo\nbroken\nsource=".into(),
            exclude_paths: "Archive，Temp".into(),
            sort_field: "rating".into(),
            sort_direction: "desc".into(),
            limit: "100".into(),
            numbers: "width=1024..4096\noriginalSizeBytes=..10485760\nempty=..".into(),
            dates: "fileCreatedAt=2024-01-01T00:00:00Z..".into(),
            ..SmartFolderDraft::default()
        };
        let filter = filter_from_draft(&draft);
        assert_eq!(filter.query.as_deref(), Some("cover"));
        assert_eq!(filter.formats.as_deref(), Some(["psd".to_string(), "png".to_string()].as_slice()));
        assert_eq!(filter.min_rating, Some(4.0));
        assert_eq!(filter.match_mode.as_deref(), Some("or"));
        assert_eq!(filter.metadata_filters.as_ref().map(Vec::len), Some(1));
        assert_eq!(filter.exclude_path_prefixes.as_deref(), Some(["Archive".to_string(), "Temp".to_string()].as_slice()));
        assert_eq!(filter.sort.as_ref().map(|sort| sort.direction.as_str()), Some("desc"));
        assert_eq!(filter.limit, Some(100));
        let numbers = filter.number_filters.clone().expect("数值范围");
        assert_eq!(numbers.len(), 2, "两端都空的条目不写入");
        assert_eq!(numbers[1].max, Some(10485760.0));
        assert!(filter.tags.is_none() && filter.exclude_query.is_none());

        let folder = SidebarSmartFolder { id: "sf".into(), parent_id: Some("root".into()), name: "高评分".into(), filter, children: Vec::new() };
        let edit = SmartFolderDraft::from_folder(&folder);
        assert!(edit.open && edit.mode_edit);
        assert_eq!(edit.action_label(), "保存");
        assert_eq!(edit.parent_id, "root");
        assert_eq!(edit.formats, "psd，png");
        assert_eq!(edit.min_rating, "4");
        assert_eq!(edit.numbers, "width=1024..4096\noriginalSizeBytes=..10485760");
        assert_eq!(edit.dates, "fileCreatedAt=2024-01-01T00:00:00Z..");
        assert_eq!(edit.sort_direction, "desc");
    }

    #[test]
    fn zero_rating_and_limit_are_left_out() {
        let draft = SmartFolderDraft { name: "x".into(), min_rating: "0".into(), limit: "abc".into(), ..SmartFolderDraft::default() };
        let filter = filter_from_draft(&draft);
        assert_eq!(filter.min_rating, None);
        assert_eq!(filter.limit, None);
        assert_eq!(filter.match_mode.as_deref(), Some("and"));
        assert!(filter.sort.is_none());
    }
}
