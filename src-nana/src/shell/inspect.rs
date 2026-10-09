//! 预览、元数据和搜索。按扩展名分派图片、文本、音视频和内置文档，Three.js 不嵌进 Runtime。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::backend::services::repository::{AssetDetail, PlaybackSessionState};
use crate::plugin_api::{NativeContributionKind, NativePluginContribution};

use super::files::repository_is_writable;
use super::workspace::WorkspacePanel;
use super::ShellViewModel;

#[path = "native_preview.rs"]
pub(super) mod native_preview;
#[path = "preview_bridge.rs"]
mod bridge;
pub use bridge::NativeLoad;
pub(super) use bridge::rgba_png_data_url;

/// 元数据编辑器自己画的通用字段，即 Vue `FileMetadataEditor.vue` 读的键，覆盖 `docs/api-design.md` 的
/// Generic file metadata keys：评分、注释（含旧键 note）、链接和标签组四个输入，以及入库、创建、修改时间、
/// 尺寸、原始大小、调色板、来源标题和来源链接的只读行。自定义草稿不收这些键，免得和编辑器字段互相覆盖。
const EDITOR_METADATA: &[&str] = &[
    "rating", "comment", "note", "link", "tagGroups", "addedToLibraryAt", "fileCreatedAt", "fileModifiedAt", "width",
    "height", "originalSizeBytes", "thumbnailPalette", "palette", "originTitle", "sourceUrl",
];

/// 后端给每个素材种下、编辑器不单独画的系统字段（`sync_metadata.rs::ensure_default_metadata` 和 Eagle 导入种子）：
/// 标题是文件名，类型是扩展名，主色是调色板第一色，收藏是 Eagle 的旧标记。插件补全仍可写入，只是不另起一行。
const SEEDED_METADATA: &[&str] = &["title", "type", "favorite", "color"];

/// 元数据区自定义行跳过的保留字段：编辑器自己画的通用字段，加上后端种下的系统字段。
pub(super) fn is_reserved_metadata(key: &str) -> bool {
    EDITOR_METADATA.contains(&key) || SEEDED_METADATA.contains(&key)
}

/// 预览分派结果。原生贡献只保留 view_id，不携带 Vue 组件。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreviewKind {
    Image,
    Markdown,
    Text,
    Media,
    Native { view_id: String, label: String },
    Unsupported,
}

/// 当前预览体。图片像素仍放在壳层的 `preview_pixels`，这里只记成败。
#[derive(Clone, Debug)]
pub enum PreviewBody {
    Empty,
    Image,
    /// `truncated_at` 有值时只显示了文件开头这么多字节。
    Document { markdown: bool, text: String, truncated_at: Option<u64> },
    Media(PlaybackSessionState),
    Native { view_id: String, label: String, content: String },
    Failed(String),
}

#[path = "inspect_search.rs"]
mod search;
pub use search::{
    AdvancedField, AssetFacet, DateBound, FilterList, MatchMode, MetadataInput, NumberBound, SearchFilters,
    SearchRequestDraft, SearchRow, SortDirection,
};

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
    /// 预览的音视频接管了播放条，宿主派发后开始播放（Vue 预览页挂载即播放）。
    Autoplay { path: String, generation: u64 },
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
    BodyLoaded { path: String, markdown: bool, generation: u64, result: Result<support::PreviewText, String> },
    NativeLoaded { path: String, generation: u64, result: Result<bridge::NativeLoad, String> },
    MediaLoaded {
        path: String,
        generation: u64,
        result: Result<PlaybackSessionState, String>,
        pcm: Option<super::player::PreviewPcm>,
        frames: Option<Vec<support::VideoFrame>>,
    },
    TurnPage(i32),
    Orbit { yaw: f32, zoom: f32 },
    /// 宿主派发的自动播放。代次或文件对不上就不动。
    Autoplay { path: String, generation: u64 },
    SetQuery(String),
    ToggleFilterBar,
    CloseFilterBar,
    ToggleFilter { key: FilterList, value: String },
    SetMatchMode(MatchMode),
    SetMinimumRating(Option<f64>),
    SetMetadataInput { key: MetadataInput, value: String },
    SubmitMetadataInput(MetadataInput),
    SetAdvanced { field: AdvancedField, value: String },
    SetSortDirection(SortDirection),
    ApplyAdvanced,
    ClearFilters,
    ApplyShortcut { metadata: String, sort_field: String, sort_direction: SortDirection },
    RunSearch,
    SearchFinished { generation: u64, result: Result<Vec<SearchRow>, String> },
    OpenHit { repo_id: String, asset_id: String },
    OpenTagMenu { x: f32, y: f32 },
    CloseTagMenu,
    ToggleTagGroup,
}

/// 预览右侧的文件事实。空字符串在视图里写成「未知」或「未记录」。
/// 入库时间、创建时间和原始大小只用于展示，不写进可编辑草稿。
#[derive(Clone, Debug, Default)]
pub(super) struct FileFacts {
    pub extension: String,
    pub size_label: String,
    pub modified_at: String,
    pub hardlink_state: Option<String>,
    pub thumbnail_path: Option<String>,
    pub added_to_library_at: String,
    pub file_created_at: String,
    pub file_modified_meta: String,
    pub meta_width: u32,
    pub meta_height: u32,
    pub original_size_bytes: Option<f64>,
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
    /// 筛选栏草稿、仓库摘要候选和待打开的命中，见 `inspect_search`。
    pub(super) search_ui: search::SearchUi,
    pub(super) filter_bar_open: bool,
    pub(super) searching: bool,
    search_generation: u64,
    pub(super) results: Vec<SearchRow>,
    pub(super) search_error: String,
    effects: Vec<InspectEffect>,
    pending_open: Option<SearchRow>,
    pub(super) tag_menu: bool,
    pub(super) tag_menu_x: f32,
    pub(super) tag_menu_y: f32,
    /// 标签组默认展开，折叠后才藏起「添加标签」。
    pub(super) tags_expanded: bool,
    pub(super) palette: Vec<String>,
    pub(super) facts: FileFacts,
    pub(super) shortcuts: Vec<super::inspect_shortcuts::SearchShortcut>,
    timers: tags::LiveTimers,
    deck: bridge::Deck,
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
            search_ui: search::SearchUi::default(),
            filter_bar_open: false,
            searching: false,
            search_generation: 0,
            results: Vec::new(),
            search_error: String::new(),
            effects: Vec::new(),
            pending_open: None,
            tag_menu: false,
            tag_menu_x: 0.0,
            tag_menu_y: 0.0,
            tags_expanded: true,
            palette: Vec::new(),
            facts: FileFacts::default(),
            shortcuts: Vec::new(),
            timers: tags::LiveTimers::default(),
            deck: bridge::Deck::default(),
        }
    }
}

#[path = "inspect_tags.rs"]
mod tags;
pub(crate) use tags::poll_timers;

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
        self.flush_pending_metadata();
        self.palette.clear();
        self.generation += 1;
        self.target_path = Some(path.to_string());
        self.kind = None;
        self.body = PreviewBody::Empty;
        self.loading = true;
        self.activity = "正在读取文件元数据…".into();
        self.error.clear();
        bridge::clear_deck(self);
        self.conflict.clear();
        self.conflict_detail = None;
        self.asset_id = None;
        self.virtual_asset = false;
        self.facts = FileFacts::default();
        self.tags_expanded = true;
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
        self.palette.clear();
        self.facts = FileFacts::default();
        self.tags_expanded = true;
        self.timers.clear_metadata();
        bridge::clear_deck(self);
    }

    /// 按扩展名打开预览，并用水合后的元数据草稿替换上一份。
    pub(super) fn note_detail(&mut self, detail: &AssetDetail) {
        let extension = support::extension_of(&detail.summary.extension, &detail.summary.filename);
        let kind = support::classify(&extension, &self.contributions);
        let same_file = self.target_path.as_deref() == Some(detail.summary.path.as_str());
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
        self.palette = super::palette::from_metadata_entries(&detail.metadata);
        if !same_file {
            self.tags_expanded = true;
        }
        self.facts = FileFacts {
            extension: extension.clone(),
            size_label: detail.summary.size_label.clone(),
            modified_at: detail.summary.modified_at.clone(),
            hardlink_state: detail.summary.hardlink_state.clone(),
            thumbnail_path: detail.summary.thumbnail_path.clone(),
            ..FileFacts::default()
        };
        support::fill_recorded_facts(&mut self.facts, &detail.metadata);
        self.timers.clear_metadata();
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

    /// 主按钮只在已经有预览目标时重新打开。没有目标时不拉取预览。
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
            PreviewKind::Unsupported => {
                let message = "无法预览此类型".to_string();
                eprintln!("Nana 无法预览此类型：{path}");
                self.body = PreviewBody::Failed(message.clone());
                self.error = message;
            }
        }
    }

    pub(super) fn reduce(&mut self, writable: bool, active_repo: Option<&str>, message: InspectMessage) {
        let hint = tags::clock_hint(&message);
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
                self.close_tag_menu();
            }
            InspectMessage::RemoveTag(tag) => self.draft.tags.retain(|item| item != &tag),
            InspectMessage::SetCustom { key, value } => {
                let key = key.trim().to_string();
                if key.is_empty() || EDITOR_METADATA.contains(&key.as_str()) {
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
            InspectMessage::MediaLoaded { path, generation, result, frames, .. } => support::note_media(self, path, generation, result, frames),
            InspectMessage::TurnPage(delta) => bridge::turn(self, delta),
            InspectMessage::Orbit { yaw, zoom } => bridge::orbit(self, yaw, zoom),
            // 播放归播放器管，由 `bridge::apply` 交过去；预览状态本身不变。
            InspectMessage::Autoplay { .. } => {}
            message @ (InspectMessage::SetQuery(_)
            | InspectMessage::ToggleFilterBar
            | InspectMessage::CloseFilterBar
            | InspectMessage::ToggleFilter { .. }
            | InspectMessage::SetMatchMode(_)
            | InspectMessage::SetMinimumRating(_)
            | InspectMessage::SetMetadataInput { .. }
            | InspectMessage::SubmitMetadataInput(_)
            | InspectMessage::SetAdvanced { .. }
            | InspectMessage::SetSortDirection(_)
            | InspectMessage::ApplyAdvanced
            | InspectMessage::ClearFilters
            | InspectMessage::ApplyShortcut { .. }
            | InspectMessage::RunSearch
            | InspectMessage::SearchFinished { .. }
            | InspectMessage::OpenHit { .. }) => self.reduce_search(writable, active_repo, message),
            InspectMessage::OpenTagMenu { x, y } => self.open_tag_menu(x, y, 220.0, 280.0, 1280.0, 800.0),
            InspectMessage::CloseTagMenu => self.close_tag_menu(),
            InspectMessage::ToggleTagGroup => self.tags_expanded = !self.tags_expanded,
        }
        self.apply_clock(hint);
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

    fn note_body(&mut self, path: String, markdown: bool, generation: u64, result: Result<support::PreviewText, String>) {
        if generation != self.generation || self.target_path.as_deref() != Some(path.as_str()) {
            eprintln!("Nana 忽略过期的文本预览：{path}");
            return;
        }
        self.loading = false;
        self.activity.clear();
        match result {
            Ok(read) => {
                self.body = PreviewBody::Document { markdown, text: read.text, truncated_at: read.truncated_at };
                self.error.clear();
            }
            Err(error) => {
                eprintln!("Nana 读取文本预览失败：{error}");
                self.body = PreviewBody::Failed(error.clone());
                self.error = error;
            }
        }
    }

    pub(crate) fn media_session(&self) -> Option<&PlaybackSessionState> {
        match &self.body {
            PreviewBody::Media(session) => Some(session),
            _ => None,
        }
    }

    /// 播放列表当前项的画面。空则清掉上一份，避免旧帧留在新文件上。
    pub(crate) fn install_clip_frames(&mut self, frames: Option<Vec<support::VideoFrame>>) {
        bridge::install_video(self, frames);
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
                .filter(|entry| !EDITOR_METADATA.contains(&entry.key.as_str()))
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

/// 播放器改了进度或状态后，预览页的视频换到对应画面。
pub(crate) fn sync_preview_frame(model: &mut ShellViewModel) {
    bridge::show_video_frame(model);
}

pub(super) fn reduce_message(model: &mut ShellViewModel, message: super::ShellMessage) -> Option<super::ShellMessage> {
    search::observe(model, &message);
    let super::ShellMessage::Inspect(message) = message else {
        return Some(message);
    };
    let writable = model.workspace.active_repository().is_some_and(|repository| {
        repository_is_writable(&repository.status, &repository.capabilities)
    });
    let repo_id = model.workspace.active_repo_id.clone();
    let follow = bridge::follow(&message);
    model.inspect.reduce(writable, repo_id.as_deref(), message);
    bridge::apply(model, follow);
    search::settle(model);
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
