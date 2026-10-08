//! 搜索与筛选状态：查询、筛选条件、筛选栏输入草稿、搜索结果和仓库摘要候选。
//!
//! 对应 Vue `composables/workspace/search.ts`、`state.ts` 的筛选字段，以及
//! `pages/workspace/search/useSearchUi.ts`、`useWorkspaceFilterShortcuts.ts` 和
//! `files/useFileInteraction.ts` 的 `openSearchHit`。切换芯片、提交颜色形状、改评分、
//! 应用高级条件、库类型快捷方式和清除都会切到搜索面板并重新查询；仓库不可写时这些动作不生效。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::backend::services::repository::{FileBrowserSnapshot, RepositorySnapshot, SearchHit};

use super::super::workspace::{LibraryCategory, WorkspacePanel};
use super::super::workspace_refresh::SilentMessage;
use super::super::{FilesMessage, ShellMessage, ShellPage, ShellViewModel};
use super::{InspectEffect, InspectMessage, InspectState};

#[path = "search_request.rs"]
mod request;
pub(crate) use request::{build_search_request, parse_limit, split_list, toggle_filter};

/// 多个条件之间的匹配方式。筛选栏不提供切换，保留给已有调用。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MatchMode {
    #[default]
    And,
    Or,
}

/// 排序方向，对应筛选栏的「升序 / 降序」下拉。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

impl SortDirection {
    /// 下拉选项的值，和 Vue `<option value>` 一致。
    pub fn value(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }

    /// 下拉里显示的文字。
    pub fn label(self) -> &'static str {
        match self {
            Self::Asc => "升序",
            Self::Desc => "降序",
        }
    }

    /// 只有 `desc` 是降序，其余都按升序。
    pub fn parse(value: &str) -> Self {
        if value == "desc" { Self::Desc } else { Self::Asc }
    }
}

/// 高级区的文本输入。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdvancedField {
    ExcludeQuery,
    ExcludePaths,
    ExcludeTags,
    ExcludeFormats,
    Metadata,
    ExcludeMetadata,
    Number,
    ExcludeNumber,
    Date,
    ExcludeDate,
    SortField,
    Limit,
}

/// 可以逐项切换的筛选列表。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterList {
    Tags,
    Formats,
    Colors,
    Shapes,
    ExcludeTags,
    ExcludeFormats,
}

/// 带「添加」按钮的两个输入：颜色和形状。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataInput {
    Colors,
    Shapes,
}

impl MetadataInput {
    /// 提交后切换的筛选列表。
    pub fn list(self) -> FilterList {
        match self {
            Self::Colors => FilterList::Colors,
            Self::Shapes => FilterList::Shapes,
        }
    }
}

/// 已生效的筛选条件，对应 Vue `WorkspaceFilterState`。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchFilters {
    pub tags: Vec<String>,
    pub formats: Vec<String>,
    pub colors: Vec<String>,
    pub shapes: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub exclude_formats: Vec<String>,
    pub exclude_query: String,
    pub exclude_path_prefixes: String,
    pub metadata_filters: String,
    pub exclude_metadata_filters: String,
    pub exclude_number_filters: String,
    pub exclude_date_filters: String,
    pub number_filters: String,
    pub date_filters: String,
    pub match_mode: MatchMode,
    pub sort_field: String,
    pub sort_direction: SortDirection,
    pub limit: Option<usize>,
    pub min_rating: Option<f64>,
}

/// 一条搜索命中。标签和元数据用于结果芯片、筛选候选和库类型快捷方式。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchRow {
    pub repo_id: String,
    pub asset_id: String,
    pub path: String,
    pub filename: String,
    pub repo_name: String,
    pub tags: Vec<String>,
    pub metadata: BTreeMap<String, Value>,
}

impl SearchRow {
    /// 服务返回的命中转成结果行。
    pub fn from_hit(hit: SearchHit) -> Self {
        Self {
            repo_id: hit.repo_id,
            asset_id: hit.asset_id,
            path: hit.path,
            filename: hit.filename,
            repo_name: hit.repo_name,
            tags: hit.tags,
            metadata: hit.metadata,
        }
    }
}

/// 可克隆的搜索请求。派发时再转成领域服务的 `SearchRequest`。
#[derive(Clone, Debug, PartialEq)]
pub struct SearchRequestDraft {
    pub query: String,
    pub repo_id: Option<String>,
    pub exclude_query: Option<String>,
    pub tags: Vec<String>,
    pub formats: Vec<String>,
    pub metadata_filters: Vec<(String, String)>,
    pub exclude_tags: Vec<String>,
    pub exclude_formats: Vec<String>,
    pub exclude_metadata_filters: Vec<(String, String)>,
    pub exclude_path_prefixes: Vec<String>,
    pub exclude_number_filters: Vec<NumberBound>,
    pub exclude_date_filters: Vec<DateBound>,
    pub number_filters: Vec<NumberBound>,
    pub date_filters: Vec<DateBound>,
    pub match_mode: Option<String>,
    pub sort_field: Option<String>,
    pub sort_direction: Option<String>,
    pub limit: Option<usize>,
    pub min_rating: Option<f64>,
}

/// 数值范围条件。两端都为空的条目在解析时丢掉。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumberBound {
    pub key: String,
    pub min: Option<String>,
    pub max: Option<String>,
}

/// 日期范围条件。两端都为空的条目在解析时丢掉。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DateBound {
    pub key: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// 筛选栏输入框的草稿，对应 Vue `useSearchUi` 里的各个 ref。
/// 只有「添加」「应用」或库类型快捷方式才把草稿写进条件。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchDraft {
    pub color: String,
    pub shape: String,
    pub exclude_query: String,
    pub exclude_paths: String,
    pub exclude_tags: String,
    pub exclude_formats: String,
    pub metadata: String,
    pub exclude_metadata: String,
    pub number: String,
    pub exclude_number: String,
    pub date: String,
    pub exclude_date: String,
    pub sort_field: String,
    pub sort_direction: SortDirection,
    pub limit: String,
}

impl SearchDraft {
    /// 高级区某个输入框的当前文字。
    pub fn advanced(&self, field: AdvancedField) -> &str {
        match field {
            AdvancedField::ExcludeQuery => &self.exclude_query,
            AdvancedField::ExcludePaths => &self.exclude_paths,
            AdvancedField::ExcludeTags => &self.exclude_tags,
            AdvancedField::ExcludeFormats => &self.exclude_formats,
            AdvancedField::Metadata => &self.metadata,
            AdvancedField::ExcludeMetadata => &self.exclude_metadata,
            AdvancedField::Number => &self.number,
            AdvancedField::ExcludeNumber => &self.exclude_number,
            AdvancedField::Date => &self.date,
            AdvancedField::ExcludeDate => &self.exclude_date,
            AdvancedField::SortField => &self.sort_field,
            AdvancedField::Limit => &self.limit,
        }
    }

    fn advanced_mut(&mut self, field: AdvancedField) -> &mut String {
        match field {
            AdvancedField::ExcludeQuery => &mut self.exclude_query,
            AdvancedField::ExcludePaths => &mut self.exclude_paths,
            AdvancedField::ExcludeTags => &mut self.exclude_tags,
            AdvancedField::ExcludeFormats => &mut self.exclude_formats,
            AdvancedField::Metadata => &mut self.metadata,
            AdvancedField::ExcludeMetadata => &mut self.exclude_metadata,
            AdvancedField::Number => &mut self.number,
            AdvancedField::ExcludeNumber => &mut self.exclude_number,
            AdvancedField::Date => &mut self.date,
            AdvancedField::ExcludeDate => &mut self.exclude_date,
            AdvancedField::SortField => &mut self.sort_field,
            AdvancedField::Limit => &mut self.limit,
        }
    }

    /// 颜色或形状输入框的当前文字。
    pub fn metadata_input(&self, key: MetadataInput) -> &str {
        match key {
            MetadataInput::Colors => &self.color,
            MetadataInput::Shapes => &self.shape,
        }
    }

    fn metadata_input_mut(&mut self, key: MetadataInput) -> &mut String {
        match key {
            MetadataInput::Colors => &mut self.color,
            MetadataInput::Shapes => &mut self.shape,
        }
    }
}

/// 仓库摘要里一项素材的标签和扩展名，格式与标签候选从这里取。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AssetFacet {
    pub tags: Vec<String>,
    pub extension: String,
}

/// 搜索面板的附加状态：输入草稿、仓库摘要候选、要切到搜索面板的标记和等待目录的命中。
#[derive(Clone, Debug, Default)]
pub struct SearchUi {
    pub draft: SearchDraft,
    /// 摘要所属的仓库。和活动仓库不同时不参与候选。
    pub facets_repo: Option<String>,
    pub facets: Vec<AssetFacet>,
    reveal_panel: bool,
    reveal_hit: Option<HitReveal>,
}

/// 打开命中时先读所在目录，目录到达后再选中并读取详情。
#[derive(Clone, Debug)]
struct HitReveal {
    row: SearchRow,
    parent: String,
}

impl InspectState {
    /// 标题栏角标和筛选栏「N 个条件」用的条件数。
    pub(crate) fn active_filter_count(&self) -> usize {
        self.filters.active_count()
    }

    /// 只归约搜索与筛选消息，其余消息由 `InspectState::reduce` 自己处理。
    pub(super) fn reduce_search(&mut self, writable: bool, active_repo: Option<&str>, message: InspectMessage) {
        match message {
            InspectMessage::SetQuery(value) => self.query = value,
            InspectMessage::RunSearch => self.run_search(active_repo),
            InspectMessage::ToggleFilterBar => self.filter_bar_open = !self.filter_bar_open,
            InspectMessage::CloseFilterBar => self.filter_bar_open = false,
            InspectMessage::ToggleFilter { key, value } => self.toggle_search_filter(writable, active_repo, key, &value),
            InspectMessage::SetMatchMode(mode) => {
                if !writable {
                    eprintln!("Nana 当前资源库不可写，忽略匹配方式");
                    return;
                }
                self.filters.match_mode = mode;
                self.run_filtered_search(active_repo);
            }
            InspectMessage::SetMinimumRating(value) => {
                if !writable {
                    eprintln!("Nana 当前资源库不可写，忽略评分筛选");
                    return;
                }
                self.filters.min_rating = value.filter(|rating| *rating > 0.0);
                self.search_ui.reveal_panel = true;
                self.run_filtered_search(active_repo);
            }
            InspectMessage::SetMetadataInput { key, value } => *self.search_ui.draft.metadata_input_mut(key) = value,
            InspectMessage::SubmitMetadataInput(key) => self.submit_metadata_input(writable, active_repo, key),
            InspectMessage::SetAdvanced { field, value } => *self.search_ui.draft.advanced_mut(field) = value,
            InspectMessage::SetSortDirection(direction) => self.search_ui.draft.sort_direction = direction,
            InspectMessage::ApplyAdvanced => self.apply_advanced(writable, active_repo),
            InspectMessage::ClearFilters => self.clear_filters(writable, active_repo),
            InspectMessage::ApplyShortcut { metadata, sort_field, sort_direction } => {
                self.apply_shortcut(writable, active_repo, metadata, sort_field, sort_direction);
            }
            InspectMessage::SearchFinished { generation, result } => self.note_search(generation, result),
            InspectMessage::OpenHit { repo_id, asset_id } => self.request_hit(&repo_id, &asset_id),
            _ => eprintln!("Nana 搜索归约收到非搜索消息"),
        }
    }

    /// Vue `toggleSearchFilter`：切换芯片后切到搜索面板并重新查询。
    fn toggle_search_filter(&mut self, writable: bool, active_repo: Option<&str>, key: FilterList, value: &str) {
        if !writable {
            eprintln!("Nana 当前资源库不可写，忽略筛选");
            return;
        }
        toggle_filter(&mut self.filters, key, value);
        self.search_ui.reveal_panel = true;
        self.run_filtered_search(active_repo);
    }

    /// Vue `submitMetadataFilterInput`：去空白后按芯片切换，再清空输入框。
    fn submit_metadata_input(&mut self, writable: bool, active_repo: Option<&str>, key: MetadataInput) {
        if !writable {
            eprintln!("Nana 当前资源库不可写，忽略添加筛选");
            return;
        }
        let value = self.search_ui.draft.metadata_input(key).trim().to_string();
        if value.is_empty() {
            return;
        }
        self.toggle_search_filter(writable, active_repo, key.list(), &value);
        self.search_ui.draft.metadata_input_mut(key).clear();
    }

    /// Vue `applyAdvancedSearchFilters`：把高级区草稿写进条件。
    fn apply_advanced(&mut self, writable: bool, active_repo: Option<&str>) {
        if !writable {
            eprintln!("Nana 当前资源库不可写，忽略高级筛选");
            return;
        }
        let draft = &self.search_ui.draft;
        self.filters.exclude_query = draft.exclude_query.trim().to_string();
        self.filters.exclude_path_prefixes = draft.exclude_paths.trim().to_string();
        self.filters.metadata_filters = draft.metadata.trim().to_string();
        self.filters.exclude_tags = split_list(&draft.exclude_tags);
        self.filters.exclude_formats = split_list(&draft.exclude_formats);
        self.filters.exclude_metadata_filters = draft.exclude_metadata.trim().to_string();
        self.filters.exclude_number_filters = draft.exclude_number.trim().to_string();
        self.filters.exclude_date_filters = draft.exclude_date.trim().to_string();
        self.filters.number_filters = draft.number.trim().to_string();
        self.filters.date_filters = draft.date.trim().to_string();
        self.filters.sort_field = draft.sort_field.trim().to_string();
        self.filters.sort_direction = draft.sort_direction;
        self.filters.limit = parse_limit(&draft.limit);
        self.search_ui.reveal_panel = true;
        self.run_filtered_search(active_repo);
    }

    /// Vue `clearSearchFilters`：清空条件和全部输入框，查询保留。
    fn clear_filters(&mut self, writable: bool, active_repo: Option<&str>) {
        if !writable {
            eprintln!("Nana 当前资源库不可写，忽略清空筛选");
            return;
        }
        self.filters = SearchFilters::default();
        self.search_ui.draft = SearchDraft::default();
        self.search_ui.reveal_panel = true;
        self.run_filtered_search(active_repo);
    }

    /// Vue `applyMetadataFilterShortcut`：快捷方式改写元数据和排序，
    /// 清掉排除标签、排除格式和数量，其余高级输入按草稿生效。
    fn apply_shortcut(
        &mut self,
        writable: bool,
        active_repo: Option<&str>,
        metadata: String,
        sort_field: String,
        sort_direction: SortDirection,
    ) {
        if !writable {
            eprintln!("Nana 当前资源库不可写，忽略快捷筛选");
            return;
        }
        let draft = &mut self.search_ui.draft;
        draft.metadata = metadata.clone();
        draft.exclude_tags.clear();
        draft.exclude_formats.clear();
        draft.limit.clear();
        draft.sort_field = sort_field.clone();
        draft.sort_direction = sort_direction;
        self.filters.metadata_filters = metadata;
        self.filters.exclude_query = draft.exclude_query.trim().to_string();
        self.filters.exclude_path_prefixes = draft.exclude_paths.trim().to_string();
        self.filters.exclude_tags.clear();
        self.filters.exclude_formats.clear();
        self.filters.exclude_metadata_filters = draft.exclude_metadata.trim().to_string();
        self.filters.exclude_number_filters = draft.exclude_number.trim().to_string();
        self.filters.exclude_date_filters = draft.exclude_date.trim().to_string();
        self.filters.number_filters = draft.number.trim().to_string();
        self.filters.date_filters = draft.date.trim().to_string();
        self.filters.sort_field = sort_field;
        self.filters.sort_direction = sort_direction;
        self.filters.limit = None;
        self.search_ui.reveal_panel = true;
        self.run_filtered_search(active_repo);
    }

    /// Vue `runSearch`：按当前查询和筛选搜索。没有任何条件时清空结果，不发请求。
    fn run_search(&mut self, active_repo: Option<&str>) {
        self.search_generation += 1;
        let generation = self.search_generation;
        let request = build_search_request(&self.query, &self.filters, active_repo);
        if !request.has_criteria() {
            self.results.clear();
            self.searching = false;
            self.search_error.clear();
            return;
        }
        self.searching = true;
        self.search_error.clear();
        self.effects.push(InspectEffect::Search { generation, request });
    }

    /// Vue `runFilteredSearch`：筛选生效却没有活动仓库时只清空结果。
    fn run_filtered_search(&mut self, active_repo: Option<&str>) {
        if active_repo.is_none() && self.filters.has_active_filters() {
            eprintln!("Nana 筛选需要活动仓库");
            self.search_generation += 1;
            self.results.clear();
            self.searching = false;
            return;
        }
        self.run_search(active_repo);
    }

    /// 只接受最近一次请求的结果。失败时保留上一批结果并写出错误。
    fn note_search(&mut self, generation: u64, result: Result<Vec<SearchRow>, String>) {
        if generation != self.search_generation {
            eprintln!("Nana 忽略过期的搜索结果");
            return;
        }
        self.searching = false;
        match result {
            Ok(rows) => {
                self.results = rows;
                self.search_error.clear();
            }
            Err(error) => {
                eprintln!("Nana 搜索失败：{error}");
                self.search_error = error;
            }
        }
    }

    /// 点中一条结果。素材 id 缺失时留在搜索面板并写明原因。
    fn request_hit(&mut self, repo_id: &str, asset_id: &str) {
        let Some(row) = self.results.iter().find(|row| row.repo_id == repo_id && row.asset_id == asset_id).cloned() else {
            eprintln!("Nana 搜索结果不存在：{repo_id} / {asset_id}");
            return;
        };
        if row.asset_id.is_empty() {
            eprintln!("Nana 搜索结果没有素材 id：{}", row.path);
            self.search_error = "搜索结果没有素材 id".into();
            return;
        }
        self.pending_open = Some(row);
    }

    /// Vue `resetSearchState`：切换资源库时清空查询、结果和条件，收起筛选栏。
    /// 筛选栏输入框的草稿属于组件，Vue 不清，这里也不清。
    pub(crate) fn reset_search(&mut self) {
        self.search_generation += 1;
        self.query.clear();
        self.results.clear();
        self.filters = SearchFilters::default();
        self.filter_bar_open = false;
        self.searching = false;
        self.search_error.clear();
        self.search_ui.reveal_hit = None;
    }
}

/// 主归约之前看一眼和搜索有关的壳层消息：仓库摘要提供候选，切换仓库清空搜索，
/// 命中所在目录到达后再选中它并读取详情。
pub(super) fn observe(model: &mut ShellViewModel, message: &ShellMessage) {
    match message {
        ShellMessage::RepositorySnapshotLoaded(Ok(snapshot)) => note_snapshot(&mut model.inspect, snapshot),
        ShellMessage::SilentWorkspace(SilentMessage::Snapshot(Ok(snapshot))) => note_snapshot(&mut model.inspect, snapshot),
        ShellMessage::SelectWorkspaceRepository(repo_id) => {
            let switching = model.workspace.active_repo_id.as_deref() != Some(repo_id.as_str())
                && model.workspace.repositories.iter().any(|item| &item.repo_id == repo_id);
            if switching {
                model.inspect.reset_search();
            }
        }
        ShellMessage::FileBrowserLoaded(result) => reveal_after_browse(model, result),
        // 用户已经去了别处：等待中的命中不再打开，免得以后读到同一目录时突然跳进预览。
        ShellMessage::OpenDirectory(_)
        | ShellMessage::SelectFile { .. }
        | ShellMessage::SetWorkspacePanel(_)
        | ShellMessage::SetLibraryCategory(_)
        | ShellMessage::Files(FilesMessage::OpenPath(_) | FilesMessage::OpenRow(_) | FilesMessage::ActivateRow(_)) => {
            if model.inspect.search_ui.reveal_hit.take().is_some() {
                eprintln!("Nana 打开搜索结果前已经切到别处，放弃等待中的命中");
            }
        }
        _ => {}
    }
}

/// 主归约之后：需要时切到搜索面板，并开始打开等待的命中。
pub(super) fn settle(model: &mut ShellViewModel) {
    if std::mem::take(&mut model.inspect.search_ui.reveal_panel) {
        model.workspace.panel = WorkspacePanel::Search;
    }
    if let Some(row) = model.inspect.pending_open.take() {
        open_hit(model, row);
    }
}

fn note_snapshot(inspect: &mut InspectState, snapshot: &RepositorySnapshot) {
    inspect.search_ui.facets_repo = Some(snapshot.repository.repo_id.clone());
    inspect.search_ui.facets = snapshot
        .assets
        .iter()
        .map(|asset| AssetFacet { tags: asset.tags.clone(), extension: asset.extension.clone() })
        .collect();
}

/// Vue `openSearchHit`：关掉预览、回到全部文件，必要时先切换资源库，再读命中所在目录。
/// 目录读不了时直接读详情，和 Vue 读目录失败后仍然 `selectAsset` 一致。
fn open_hit(model: &mut ShellViewModel, row: SearchRow) {
    model.workspace.library_category = LibraryCategory::All;
    model.workspace.panel = WorkspacePanel::Files;
    model.page = ShellPage::FileList;
    model.player.disarm_preview_audio();
    model.selected_path = None;
    model.preview_url = None;
    model.preview_token = None;
    model.preview_pixels = None;
    model.inspect.clear();
    if model.workspace.active_repo_id.as_deref() != Some(row.repo_id.as_str()) {
        if !model.workspace.repositories.iter().any(|item| item.repo_id == row.repo_id) {
            eprintln!("Nana 搜索结果所在的资源库不在列表里：{}", row.repo_id);
            return;
        }
        model.reduce(ShellMessage::SelectWorkspaceRepository(row.repo_id.clone()));
        if model.workspace.active_repo_id.as_deref() != Some(row.repo_id.as_str()) {
            eprintln!("Nana 没能切换到搜索结果所在的资源库：{}", row.repo_id);
            return;
        }
    }
    let parent = parent_path(&row.path);
    let context = super::super::files::FileContext::from_model(model);
    if model.files.request_browse(&context, &parent) {
        model.files.set_drag_selection(vec![row.path.clone()], Some(row.path.clone()), Some(row.path.clone()));
        model.inspect.search_ui.reveal_hit = Some(HitReveal { row, parent });
    } else {
        eprintln!("Nana 搜索结果所在目录没有发出读取：{parent}");
        load_hit(model, row);
    }
}

/// 命中所在目录的快照到达：选中命中并读取详情，详情到达后进入预览。
fn reveal_after_browse(model: &mut ShellViewModel, result: &Result<FileBrowserSnapshot, String>) {
    let Some(reveal) = model.inspect.search_ui.reveal_hit.as_ref() else {
        return;
    };
    if let Ok(snapshot) = result {
        let same = snapshot.repo_id == reveal.row.repo_id
            && normalize_directory(&snapshot.current_path) == reveal.parent
            && snapshot.special_location.as_deref() != Some("trash");
        if !same {
            return;
        }
    } else {
        eprintln!("Nana 搜索结果所在目录读取失败，直接打开详情：{}", reveal.parent);
    }
    let Some(reveal) = model.inspect.search_ui.reveal_hit.take() else {
        return;
    };
    load_hit(model, reveal.row);
}

fn load_hit(model: &mut ShellViewModel, row: SearchRow) {
    model.selected_path = Some(row.path.clone());
    model.inspect.begin_selection(&row.path);
    model.inspect.repo_id = Some(row.repo_id.clone());
    model.inspect.effects.push(InspectEffect::LoadAsset { repo_id: row.repo_id, asset_id: row.asset_id });
}

/// 命中路径的父目录。根目录下的文件父目录是空串。
fn parent_path(path: &str) -> String {
    let path = normalize_directory(path);
    path.rsplit_once('/').map(|(parent, _)| parent.to_string()).unwrap_or_default()
}

fn normalize_directory(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_string()
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
