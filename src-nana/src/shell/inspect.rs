//! 预览、元数据和搜索。
//!
//! 按扩展名分派图片、Markdown、纯文本、音视频，以及内置的压缩包、文档和模型预览。
//! 音视频先读取文件。WAV 停在 paused 并带时长；其它格式失败，不把失败当成播放。
//! ZIP、Open XML、OBJ、glTF 和 STL 由内置贡献读取。PDF、旧版 Office 和其余三维格式
//! 在登记可绘制的 Preview 贡献之前显示升级提示。Three.js 页面不嵌进 Runtime。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::backend::services::repository::{AssetDetail, PlaybackSessionState};
use crate::host_api::PlaybackSessionController;
use crate::plugin_api::{NativeContributionKind, NativePluginContribution};

use super::files::repository_is_writable;
use super::workspace::WorkspacePanel;
use super::ShellViewModel;

#[path = "native_preview.rs"]
pub(super) mod native_preview;

const RESERVED_METADATA: &[&str] = &[
    "rating", "comment", "note", "link", "tagGroups", "addedToLibraryAt", "fileCreatedAt", "fileModifiedAt", "width",
    "height", "originalSizeBytes", "thumbnailPalette", "palette", "originTitle", "sourceUrl",
];

/// 预览分派结果。原生贡献只保留 view_id，不携带 Vue 组件。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreviewKind {
    Image,
    Markdown,
    Text,
    Media,
    Native { view_id: String, label: String },
    Upgrade,
    Unsupported,
}

/// 当前预览体。图片像素仍放在壳层的 `preview_pixels`，这里只记成败。
#[derive(Clone, Debug)]
pub enum PreviewBody {
    Empty,
    Image,
    Document { markdown: bool, text: String },
    Media(PlaybackSessionState),
    Native { view_id: String, label: String, content: String },
    Failed(String),
    Upgrade(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MatchMode {
    #[default]
    And,
    Or,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AdvancedDraft {
    exclude_query: String,
    exclude_paths: String,
    exclude_tags: String,
    exclude_formats: String,
    metadata: String,
    exclude_metadata: String,
    number: String,
    exclude_number: String,
    date: String,
    exclude_date: String,
    sort_field: String,
    sort_direction: SortDirection,
    limit: String,
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchRow {
    pub repo_id: String,
    pub asset_id: String,
    pub path: String,
    pub filename: String,
    pub repo_name: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct MetadataDraft {
    rating: i64,
    comment: String,
    link: String,
    tags: Vec<String>,
    custom: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewBinding {
    pub extensions: Vec<String>,
    pub contribution: NativePluginContribution,
}

#[derive(Clone, Debug)]
pub enum InspectEffect {
    LoadImage { repo_id: String, path: String },
    LoadText { repo_id: String, path: String, markdown: bool, generation: u64 },
    LoadNative { repo_id: String, path: String, view_id: String, generation: u64 },
    LoadMedia { repo_id: String, path: String, generation: u64 },
    SaveMetadata { repo_id: String, asset_id: String, expected_version: i64, metadata: BTreeMap<String, Value> },
    Undo { repo_id: String, asset_id: String },
    Redo { repo_id: String, asset_id: String },
    LoadAsset { repo_id: String, asset_id: String },
    Search { generation: u64, request: SearchRequestDraft },
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumberBound {
    pub key: String,
    pub min: Option<String>,
    pub max: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DateBound {
    pub key: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Clone, Debug)]
pub enum InspectMessage {
    RegisterPreview(PreviewBinding),
    SetComment(String),
    SetLink(String),
    SetRating(i64),
    AddTag(String),
    RemoveTag(String),
    SetCustom { key: String, value: String },
    RemoveCustom(String),
    SaveMetadata,
    AdoptConflict,
    Undo,
    Redo,
    RevisionLoaded(Result<(String, AssetDetail), String>),
    MetadataSaved(Result<(String, AssetDetail), String>),
    BodyLoaded { path: String, markdown: bool, generation: u64, result: Result<String, String> },
    NativeLoaded { path: String, generation: u64, result: Result<String, String> },
    MediaLoaded { path: String, generation: u64, result: Result<PlaybackSessionState, String> },
    PlayPause,
    Seek(u64),
    SetVolume(f32),
    SetQuery(String),
    ToggleFilterBar,
    ToggleFilter { key: FilterList, value: String },
    SetMatchMode(MatchMode),
    SetMinimumRating(Option<f64>),
    SetFilterInput { key: FilterList, value: String },
    SubmitFilterInput,
    SetAdvanced { field: AdvancedField, value: String },
    SetSortDirection(SortDirection),
    ApplyAdvanced,
    ClearFilters,
    ApplyShortcut { metadata: String, sort_field: String, sort_direction: SortDirection },
    RunSearch,
    SearchFinished { generation: u64, result: Result<Vec<SearchRow>, String> },
    OpenHit(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilterList {
    Tags,
    Formats,
    Colors,
    Shapes,
    ExcludeTags,
    ExcludeFormats,
}

#[derive(Clone, Debug)]
pub struct InspectState {
    pub(super) kind: Option<PreviewKind>,
    pub(super) body: PreviewBody,
    pub(super) loading: bool,
    pub(super) activity: String,
    pub(super) error: String,
    pub(super) conflict: String,
    pub(super) target_path: Option<String>,
    pub(super) generation: u64,
    contributions: Vec<PreviewBinding>,
    asset_id: Option<String>,
    repo_id: Option<String>,
    expected_version: i64,
    virtual_asset: bool,
    baseline: MetadataDraft,
    draft: MetadataDraft,
    conflict_detail: Option<AssetDetail>,
    saving: bool,
    pub(super) query: String,
    pub(super) filters: SearchFilters,
    advanced: AdvancedDraft,
    pub(super) filter_bar_open: bool,
    filter_input_key: FilterList,
    filter_input: String,
    pub(super) searching: bool,
    search_generation: u64,
    pub(super) results: Vec<SearchRow>,
    pub(super) search_error: String,
    effects: Vec<InspectEffect>,
    pending_open: Option<SearchRow>,
    pub(super) tag_menu: bool,
    pub(super) tag_menu_x: f32,
    pub(super) tag_menu_y: f32,
}

impl Default for InspectState {
    fn default() -> Self {
        Self {
            kind: None,
            body: PreviewBody::Empty,
            loading: false,
            activity: String::new(),
            error: String::new(),
            conflict: String::new(),
            target_path: None,
            generation: 0,
            contributions: native_preview::builtin_bindings(),
            asset_id: None,
            repo_id: None,
            expected_version: 0,
            virtual_asset: false,
            baseline: MetadataDraft::default(),
            draft: MetadataDraft::default(),
            conflict_detail: None,
            saving: false,
            query: String::new(),
            filters: SearchFilters::default(),
            advanced: AdvancedDraft::default(),
            filter_bar_open: false,
            filter_input_key: FilterList::Tags,
            filter_input: String::new(),
            searching: false,
            search_generation: 0,
            results: Vec::new(),
            search_error: String::new(),
            effects: Vec::new(),
            pending_open: None,
            tag_menu: false,
            tag_menu_x: 0.0,
            tag_menu_y: 0.0,
        }
    }
}

#[path = "inspect_tags.rs"]
mod tags;

impl InspectState {
    pub fn take_effects(&mut self) -> Vec<InspectEffect> {
        std::mem::take(&mut self.effects)
    }

    pub(super) fn has_target(&self) -> bool {
        self.target_path.is_some()
    }

    pub(super) fn dirty(&self) -> bool {
        self.draft != self.baseline
    }

    pub(super) fn can_edit(&self) -> bool {
        self.asset_id.is_some() && !self.virtual_asset && !self.saving
    }

    pub(super) fn saving(&self) -> bool {
        self.saving
    }

    pub(super) fn draft_rating(&self) -> i64 {
        self.draft.rating
    }

    pub(super) fn draft_comment(&self) -> &str {
        &self.draft.comment
    }

    pub(super) fn draft_link(&self) -> &str {
        &self.draft.link
    }

    pub(super) fn draft_tags(&self) -> &[String] {
        &self.draft.tags
    }

    pub(super) fn draft_custom(&self) -> &BTreeMap<String, String> {
        &self.draft.custom
    }

    /// 选择新文件时清掉上一份预览，等元数据到达后再分派。
    pub(super) fn begin_selection(&mut self, path: &str) {
        self.generation += 1;
        self.target_path = Some(path.to_string());
        self.kind = None;
        self.body = PreviewBody::Empty;
        self.loading = true;
        self.activity = "正在读取文件元数据…".into();
        self.error.clear();
        self.conflict.clear();
        self.conflict_detail = None;
        self.asset_id = None;
        self.virtual_asset = false;
    }

    pub(super) fn clear(&mut self) {
        self.generation += 1;
        self.target_path = None;
        self.kind = None;
        self.body = PreviewBody::Empty;
        self.loading = false;
        self.activity.clear();
        self.error.clear();
        self.asset_id = None;
    }

    /// 按扩展名打开预览，并用水合后的元数据草稿替换上一份。
    pub(super) fn note_detail(&mut self, detail: &AssetDetail) {
        let extension = support::extension_of(&detail.summary.extension, &detail.summary.filename);
        let kind = support::classify(&extension, &self.contributions);
        self.generation += 1;
        self.target_path = Some(detail.summary.path.clone());
        self.repo_id = Some(detail.summary.repo_id.clone());
        self.asset_id = Some(detail.summary.asset_id.clone());
        self.expected_version = detail.summary.version;
        self.virtual_asset = detail.summary.is_virtual;
        self.kind = Some(kind.clone());
        self.error.clear();
        self.conflict.clear();
        self.conflict_detail = None;
        self.saving = false;
        let draft = MetadataDraft::from_detail(detail);
        self.baseline = draft.clone();
        self.draft = draft;
        self.start_preview(&detail.summary.repo_id, &detail.summary.path, kind);
    }

    pub(super) fn note_detail_error(&mut self, error: &str) {
        eprintln!("Nana 读取文件元数据失败：{error}");
        self.loading = false;
        self.activity.clear();
        self.body = PreviewBody::Failed(error.to_string());
        self.error = error.to_string();
    }

    /// 图片解码失败是错误态。路径与当前目标不一致时保留原预览。
    pub(super) fn note_pixels(&mut self, path: &str, pixels_ok: bool, error: Option<&str>) -> bool {
        if self.target_path.as_ref().is_some_and(|target| target != path) {
            eprintln!("Nana 忽略过期的图片预览：{path}");
            return false;
        }
        self.loading = false;
        self.activity.clear();
        if pixels_ok {
            self.body = PreviewBody::Image;
            self.error.clear();
        } else {
            let message = error.unwrap_or("图片解码失败").to_string();
            eprintln!("Nana 图片预览失败：{message}");
            self.body = PreviewBody::Failed(message.clone());
            self.error = message;
        }
        true
    }

    /// 主按钮只在已经有预览目标时重新打开。验收页没有目标，仍走原来的未接通文案。
    pub(super) fn request_open(&mut self) -> bool {
        let (Some(path), Some(repo_id), Some(kind)) = (self.target_path.clone(), self.repo_id.clone(), self.kind.clone()) else {
            return false;
        };
        self.generation += 1;
        self.start_preview(&repo_id, &path, kind);
        true
    }

    fn start_preview(&mut self, repo_id: &str, path: &str, kind: PreviewKind) {
        self.loading = false;
        self.activity.clear();
        match kind {
            PreviewKind::Image => {
                self.loading = true;
                self.activity = "正在准备图片预览…".into();
                self.body = PreviewBody::Empty;
                self.effects.push(InspectEffect::LoadImage { repo_id: repo_id.to_string(), path: path.to_string() });
            }
            PreviewKind::Markdown | PreviewKind::Text => {
                let markdown = matches!(kind, PreviewKind::Markdown);
                self.loading = true;
                self.activity = "正在读取文本…".into();
                self.body = PreviewBody::Empty;
                self.effects.push(InspectEffect::LoadText {
                    repo_id: repo_id.to_string(),
                    path: path.to_string(),
                    markdown,
                    generation: self.generation,
                });
            }
            PreviewKind::Media => support::begin_media(self, repo_id, path),
            PreviewKind::Native { view_id, label } => native_preview::begin(self, repo_id, path, view_id, label),
            PreviewKind::Upgrade => {
                let message = "该预览仍是 Vue 插件，需要升级为 Nana 原生预览贡献".to_string();
                eprintln!("Nana 预览插件需要升级：{path}");
                self.body = PreviewBody::Upgrade(message.clone());
                self.error = message;
            }
            PreviewKind::Unsupported => {
                let message = "无法预览此类型".to_string();
                eprintln!("Nana 无法预览此类型：{path}");
                self.body = PreviewBody::Failed(message.clone());
                self.error = message;
            }
        }
    }

    pub(super) fn reduce(&mut self, writable: bool, active_repo: Option<&str>, message: InspectMessage) {
        match message {
            InspectMessage::RegisterPreview(binding) => {
                if binding.contribution.kind != NativeContributionKind::Preview {
                    eprintln!("Nana 忽略非预览贡献：{}", binding.contribution.plugin_id);
                    return;
                }
                self.contributions.push(binding);
            }
            InspectMessage::SetComment(value) => self.draft.comment = value,
            InspectMessage::SetLink(value) => self.draft.link = value,
            InspectMessage::SetRating(rating) => {
                let rating = rating.clamp(0, 5);
                self.draft.rating = if self.draft.rating == rating { 0 } else { rating };
            }
            InspectMessage::AddTag(tag) => {
                let tag = tag.trim().to_string();
                if tag.is_empty() || self.draft.tags.iter().any(|item| item == &tag) {
                    return;
                }
                self.draft.tags.push(tag);
            }
            InspectMessage::RemoveTag(tag) => self.draft.tags.retain(|item| item != &tag),
            InspectMessage::SetCustom { key, value } => {
                let key = key.trim().to_string();
                if key.is_empty() || RESERVED_METADATA.contains(&key.as_str()) {
                    eprintln!("Nana 忽略无效的自定义字段：{key}");
                    return;
                }
                self.draft.custom.insert(key, value);
            }
            InspectMessage::RemoveCustom(key) => {
                self.draft.custom.remove(&key);
            }
            InspectMessage::SaveMetadata => {
                self.save_metadata();
            }
            InspectMessage::AdoptConflict => self.adopt_conflict(),
            InspectMessage::Undo => self.revise(true),
            InspectMessage::Redo => self.revise(false),
            InspectMessage::RevisionLoaded(result) | InspectMessage::MetadataSaved(result) => match result {
                Ok((outcome, detail)) => self.note_metadata_result(&outcome, detail),
                Err(error) => self.note_metadata_error(error),
            },
            InspectMessage::BodyLoaded { path, markdown, generation, result } => {
                self.note_body(path, markdown, generation, result);
            }
            InspectMessage::NativeLoaded { path, generation, result } => native_preview::note_loaded(self, path, generation, result),
            InspectMessage::MediaLoaded { path, generation, result } => support::note_media(self, path, generation, result),
            InspectMessage::PlayPause => self.transport_play_pause(),
            InspectMessage::Seek(position_ms) => self.transport_seek(position_ms),
            InspectMessage::SetVolume(volume) => self.transport_volume(volume),
            InspectMessage::SetQuery(value) => self.query = value,
            InspectMessage::ToggleFilterBar => self.filter_bar_open = !self.filter_bar_open,
            InspectMessage::ToggleFilter { key, value } => {
                if !writable {
                    eprintln!("Nana 当前资源库不可写，忽略筛选");
                    return;
                }
                support::toggle_filter(&mut self.filters, key, &value);
                self.run_search(active_repo);
            }
            InspectMessage::SetMatchMode(mode) => {
                if !writable {
                    eprintln!("Nana 当前资源库不可写，忽略筛选");
                    return;
                }
                self.filters.match_mode = mode;
                self.run_search(active_repo);
            }
            InspectMessage::SetFilterInput { key, value } => {
                self.filter_input_key = key;
                self.filter_input = value;
            }
            InspectMessage::SubmitFilterInput => {
                let key = self.filter_input_key;
                let value = self.filter_input.clone();
                self.filter_input.clear();
                self.reduce(writable, active_repo, InspectMessage::ToggleFilter { key, value });
            }
            InspectMessage::SetMinimumRating(value) => {
                if !writable {
                    eprintln!("Nana 当前资源库不可写，忽略评分筛选");
                    return;
                }
                self.filters.min_rating = value.filter(|rating| *rating > 0.0);
                self.run_search(active_repo);
            }
            InspectMessage::SetAdvanced { field, value } => self.set_advanced(field, value),
            InspectMessage::SetSortDirection(direction) => self.advanced.sort_direction = direction,
            InspectMessage::ApplyAdvanced => {
                if !writable {
                    eprintln!("Nana 当前资源库不可写，忽略高级筛选");
                    return;
                }
                self.commit_advanced();
                self.run_search(active_repo);
            }
            InspectMessage::ClearFilters => {
                if !writable {
                    eprintln!("Nana 当前资源库不可写，忽略清空筛选");
                    return;
                }
                self.filters = SearchFilters::default();
                self.advanced = AdvancedDraft::default();
                self.run_search(active_repo);
            }
            InspectMessage::ApplyShortcut { metadata, sort_field, sort_direction } => {
                if !writable {
                    eprintln!("Nana 当前资源库不可写，忽略快捷筛选");
                    return;
                }
                self.filters.metadata_filters = metadata;
                self.filters.sort_field = sort_field;
                self.filters.sort_direction = sort_direction;
                self.run_search(active_repo);
            }
            InspectMessage::RunSearch => self.run_search(active_repo),
            InspectMessage::SearchFinished { generation, result } => self.note_search(generation, result),
            InspectMessage::OpenHit(asset_id) => self.open_hit(&asset_id),
        }
    }

    fn save_metadata(&mut self) {
        if self.saving {
            eprintln!("Nana 元数据保存进行中");
            return;
        }
        if self.virtual_asset || self.asset_id.is_none() {
            eprintln!("Nana 当前素材不能编辑元数据");
            return;
        }
        if !self.dirty() {
            return;
        }
        let Some(repo_id) = self.repo_id.clone() else {
            eprintln!("Nana 保存元数据没有活动仓库");
            return;
        };
        let asset_id = self.asset_id.clone().unwrap_or_default();
        self.saving = true;
        self.error.clear();
        self.activity = "正在保存元数据…".into();
        self.effects.push(InspectEffect::SaveMetadata {
            repo_id,
            asset_id,
            expected_version: self.expected_version,
            metadata: self.draft.to_metadata_map(),
        });
    }

    fn adopt_conflict(&mut self) {
        let Some(detail) = self.conflict_detail.clone() else {
            return;
        };
        let draft = MetadataDraft::from_detail(&detail);
        self.expected_version = detail.summary.version;
        self.baseline = draft.clone();
        self.draft = draft;
        self.conflict.clear();
        self.conflict_detail = None;
        self.error.clear();
    }

    fn revise(&mut self, undo: bool) {
        if self.dirty() {
            eprintln!("Nana 有未保存的元数据，不能撤销或重做");
            return;
        }
        if self.saving || self.asset_id.is_none() {
            return;
        }
        let Some(repo_id) = self.repo_id.clone() else {
            return;
        };
        let asset_id = self.asset_id.clone().unwrap_or_default();
        self.saving = true;
        self.activity = if undo { "正在撤销…" } else { "正在重做…" }.into();
        self.effects.push(if undo {
            InspectEffect::Undo { repo_id, asset_id }
        } else {
            InspectEffect::Redo { repo_id, asset_id }
        });
    }

    fn note_metadata_result(&mut self, outcome: &str, detail: AssetDetail) {
        self.saving = false;
        self.activity.clear();
        if outcome == "conflict" {
            eprintln!("Nana 元数据版本冲突：{}", detail.summary.asset_id);
            self.conflict = "版本冲突，未写入".into();
            self.conflict_detail = Some(detail);
            return;
        }
        if outcome != "success" {
            self.note_metadata_error(format!("元数据结果无法识别：{outcome}"));
            return;
        }
        if self.target_path.as_deref() == Some(detail.summary.path.as_str()) {
            self.expected_version = detail.summary.version;
            self.virtual_asset = detail.summary.is_virtual;
            let draft = MetadataDraft::from_detail(&detail);
            self.baseline = draft.clone();
            self.draft = draft;
            self.conflict.clear();
            self.conflict_detail = None;
            self.error.clear();
            return;
        }
        self.note_detail(&detail);
    }

    fn note_metadata_error(&mut self, error: String) {
        eprintln!("Nana 保存元数据失败：{error}");
        self.saving = false;
        self.activity.clear();
        self.error = error;
    }

    fn note_body(&mut self, path: String, markdown: bool, generation: u64, result: Result<String, String>) {
        if generation != self.generation || self.target_path.as_deref() != Some(path.as_str()) {
            eprintln!("Nana 忽略过期的文本预览：{path}");
            return;
        }
        self.loading = false;
        self.activity.clear();
        match result {
            Ok(text) => {
                self.body = PreviewBody::Document { markdown, text };
                self.error.clear();
            }
            Err(error) => {
                eprintln!("Nana 读取文本预览失败：{error}");
                self.body = PreviewBody::Failed(error.clone());
                self.error = error;
            }
        }
    }

    fn transport_play_pause(&mut self) {
        let PreviewBody::Media(session) = &self.body else {
            eprintln!("Nana 当前预览不是音视频");
            return;
        };
        let playing = session.status == "playing";
        let mut controller = PlaybackSessionController::new(support::transport_plugin(session), session.clone());
        let result = if playing { controller.pause() } else { controller.play() };
        if let Err(error) = result {
            eprintln!("Nana 播放控制失败：{error}");
        }
        self.store_session(controller.state().clone());
    }

    fn transport_seek(&mut self, position_ms: u64) {
        let PreviewBody::Media(session) = &self.body else {
            return;
        };
        let mut controller = PlaybackSessionController::new(support::transport_plugin(session), session.clone());
        if let Err(error) = controller.seek(position_ms) {
            eprintln!("Nana 播放进度失败：{error}");
        }
        self.store_session(controller.state().clone());
    }

    fn transport_volume(&mut self, volume: f32) {
        let PreviewBody::Media(session) = &self.body else {
            return;
        };
        let mut controller = PlaybackSessionController::new(support::transport_plugin(session), session.clone());
        if let Err(error) = controller.set_volume(volume) {
            eprintln!("Nana 播放音量失败：{error}");
        }
        self.store_session(controller.state().clone());
    }

    fn store_session(&mut self, session: PlaybackSessionState) {
        self.error = session.error.clone().unwrap_or_default();
        self.body = PreviewBody::Media(session);
    }

    fn set_advanced(&mut self, field: AdvancedField, value: String) {
        let draft = &mut self.advanced;
        match field {
            AdvancedField::ExcludeQuery => draft.exclude_query = value,
            AdvancedField::ExcludePaths => draft.exclude_paths = value,
            AdvancedField::ExcludeTags => draft.exclude_tags = value,
            AdvancedField::ExcludeFormats => draft.exclude_formats = value,
            AdvancedField::Metadata => draft.metadata = value,
            AdvancedField::ExcludeMetadata => draft.exclude_metadata = value,
            AdvancedField::Number => draft.number = value,
            AdvancedField::ExcludeNumber => draft.exclude_number = value,
            AdvancedField::Date => draft.date = value,
            AdvancedField::ExcludeDate => draft.exclude_date = value,
            AdvancedField::SortField => draft.sort_field = value,
            AdvancedField::Limit => draft.limit = value,
        }
    }

    fn commit_advanced(&mut self) {
        let draft = &self.advanced;
        self.filters.exclude_query = draft.exclude_query.trim().to_string();
        self.filters.exclude_path_prefixes = draft.exclude_paths.trim().to_string();
        self.filters.exclude_tags = support::split_list(&draft.exclude_tags);
        self.filters.exclude_formats = support::split_list(&draft.exclude_formats);
        self.filters.metadata_filters = draft.metadata.trim().to_string();
        self.filters.exclude_metadata_filters = draft.exclude_metadata.trim().to_string();
        self.filters.number_filters = draft.number.trim().to_string();
        self.filters.exclude_number_filters = draft.exclude_number.trim().to_string();
        self.filters.date_filters = draft.date.trim().to_string();
        self.filters.exclude_date_filters = draft.exclude_date.trim().to_string();
        self.filters.sort_field = draft.sort_field.trim().to_string();
        self.filters.sort_direction = draft.sort_direction;
        self.filters.limit = support::parse_limit(&draft.limit);
    }

    /// 没有查询条件时清空结果，不发请求。筛选生效且没有仓库时也不发请求。
    fn run_search(&mut self, active_repo: Option<&str>) {
        self.search_generation += 1;
        let generation = self.search_generation;
        let request = support::build_search_request(&self.query, &self.filters, active_repo);
        if !request.has_criteria() {
            self.results.clear();
            self.searching = false;
            self.search_error.clear();
            return;
        }
        if request.repo_id.is_none() && self.filters.has_active_filters() && active_repo.is_none() {
            eprintln!("Nana 筛选需要活动仓库");
            self.results.clear();
            self.searching = false;
            return;
        }
        self.searching = true;
        self.search_error.clear();
        self.effects.push(InspectEffect::Search { generation, request });
    }

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

    fn open_hit(&mut self, asset_id: &str) {
        let Some(row) = self.results.iter().find(|row| row.asset_id == asset_id).cloned() else {
            eprintln!("Nana 搜索结果不存在：{asset_id}");
            return;
        };
        if row.asset_id.is_empty() {
            eprintln!("Nana 搜索结果没有素材 id：{}", row.path);
            self.search_error = "搜索结果没有素材 id".into();
            return;
        }
        self.pending_open = Some(row);
    }

    pub(crate) fn media_session(&self) -> Option<&PlaybackSessionState> {
        match &self.body {
            PreviewBody::Media(session) => Some(session),
            _ => None,
        }
    }

    /// 只有预览体已经是音视频时，才把共用会话写回去。
    pub(crate) fn replace_shared_media(&mut self, session: PlaybackSessionState) {
        if !matches!(self.body, PreviewBody::Media(_)) {
            return;
        }
        self.error = session.error.clone().unwrap_or_default();
        self.body = PreviewBody::Media(session);
    }

    /// 从播放列表打开文件。有素材 id 时再读取详情。
    pub(crate) fn open_playlist_item(&mut self, path: &str, repo_id: &str, asset_id: &str) {
        self.begin_selection(path);
        self.repo_id = Some(repo_id.to_string());
        if !asset_id.is_empty() {
            self.effects.push(InspectEffect::LoadAsset { repo_id: repo_id.to_string(), asset_id: asset_id.to_string() });
        }
    }
}

impl MetadataDraft {
    fn from_detail(detail: &AssetDetail) -> Self {
        let metadata = &detail.metadata;
        Self {
            rating: support::metadata_rating(metadata),
            comment: support::metadata_string(metadata, "comment").or_else(|| support::metadata_string(metadata, "note")).unwrap_or_default(),
            link: support::metadata_string(metadata, "link").unwrap_or_default(),
            tags: support::metadata_tags(metadata).unwrap_or_else(|| detail.summary.tags.clone()),
            custom: metadata
                .iter()
                .filter(|entry| !RESERVED_METADATA.contains(&entry.key.as_str()))
                .map(|entry| (entry.key.clone(), support::value_text(&entry.value)))
                .collect(),
        }
    }

    fn to_metadata_map(&self) -> BTreeMap<String, Value> {
        let mut metadata = BTreeMap::new();
        metadata.insert("rating".into(), Value::from(self.rating));
        metadata.insert("comment".into(), Value::String(self.comment.trim().to_string()));
        metadata.insert("link".into(), Value::String(self.link.trim().to_string()));
        metadata.insert("tagGroups".into(), Value::Array(self.tags.iter().cloned().map(Value::String).collect()));
        for (key, value) in &self.custom {
            metadata.insert(key.clone(), Value::String(value.clone()));
        }
        metadata
    }
}

impl SearchFilters {
    /// 与 Vue `hasActiveFilters` 相同：排除关键词、排除路径和数值日期排除不单独把搜索收进当前仓库。
    pub fn has_active_filters(&self) -> bool {
        !self.tags.is_empty()
            || !self.formats.is_empty()
            || !self.colors.is_empty()
            || !self.shapes.is_empty()
            || !self.exclude_tags.is_empty()
            || !self.exclude_formats.is_empty()
            || !self.metadata_filters.trim().is_empty()
            || !self.exclude_metadata_filters.trim().is_empty()
            || !self.number_filters.trim().is_empty()
            || !self.date_filters.trim().is_empty()
            || self.match_mode == MatchMode::Or
            || !self.sort_field.trim().is_empty()
            || self.limit.is_some()
            || self.min_rating.is_some()
    }
}

impl SearchRequestDraft {
    pub fn has_criteria(&self) -> bool {
        !self.query.trim().is_empty()
            || !self.tags.is_empty()
            || !self.formats.is_empty()
            || !self.metadata_filters.is_empty()
            || !self.exclude_tags.is_empty()
            || !self.exclude_formats.is_empty()
            || self.exclude_query.as_ref().is_some_and(|value| !value.trim().is_empty())
            || !self.exclude_path_prefixes.is_empty()
            || !self.exclude_metadata_filters.is_empty()
            || !self.exclude_number_filters.is_empty()
            || !self.exclude_date_filters.is_empty()
            || !self.number_filters.is_empty()
            || !self.date_filters.is_empty()
            || self.sort_field.is_some()
            || self.limit.is_some()
            || self.min_rating.is_some()
    }
}

pub(super) fn reduce_message(model: &mut ShellViewModel, message: super::ShellMessage) -> Option<super::ShellMessage> {
    let super::ShellMessage::Inspect(message) = message else {
        return Some(message);
    };
    let writable = model.workspace.active_repository().is_some_and(|repository| {
        repository_is_writable(&repository.status, &repository.capabilities)
    });
    let repo_id = model.workspace.active_repo_id.clone();
    model.inspect.reduce(writable, repo_id.as_deref(), message);
    if let Some(session) = model.inspect.media_session().cloned() {
        model.player.adopt_session(session);
    }
    if let Some(row) = model.inspect.pending_open.take() {
        model.workspace.panel = WorkspacePanel::Files;
        model.selected_path = Some(row.path.clone());
        model.inspect.begin_selection(&row.path);
        model.inspect.repo_id = Some(row.repo_id.clone());
        model.inspect.effects.push(InspectEffect::LoadAsset { repo_id: row.repo_id, asset_id: row.asset_id });
    }
    None
}

impl super::ShellViewModel {
    pub(super) fn inspect_surface_visible(&self) -> bool {
        self.workspace.startup.status == super::workspace::StartupStatus::Ready
            && self.workspace.main_region() == super::workspace::MainRegion::HasRepository
            && (self.workspace.panel == WorkspacePanel::Search || self.inspect.has_target())
    }
}

#[path = "inspect_support.rs"]
pub(super) mod support;
pub use support::prepare_text;

#[cfg(test)]
#[path = "inspect_tests.rs"]
mod tests;
