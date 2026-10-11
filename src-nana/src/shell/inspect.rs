//! 预览、元数据和搜索。按扩展名分派图片、文本、音视频和内置文档，Three.js 不嵌进 Runtime。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::backend::services::repository::{AssetDetail, PlaybackSessionState};

use super::files::repository_is_writable;
use super::ShellViewModel;

#[path = "native_preview.rs"]
pub(super) mod native_preview;
#[path = "preview_bridge.rs"]
mod bridge;
pub use bridge::NativeLoad;
pub(super) use bridge::rgba_png_data_url;

/// 元数据编辑器自己画的通用字段，即 Vue `FileMetadataEditor.vue` 读的键，覆盖 `docs/api-design.md` 的
/// Generic file metadata keys：评分、注释（含旧键 note）、链接和标签组四个输入，以及入库、创建、修改时间、
/// 尺寸、原始大小、调色板、来源标题和来源链接的只读行。自定义行不再重复画这些键。
const EDITOR_METADATA: &[&str] = &[
    "rating", "comment", "note", "link", "tagGroups", "addedToLibraryAt", "fileCreatedAt", "fileModifiedAt", "width",
    "height", "originalSizeBytes", "thumbnailPalette", "palette", "originTitle", "sourceUrl",
];

/// 编辑器的四个输入（评分、注释和旧键 note、链接、标签组）。草稿单独存它们；元数据补丁不写这几个键，
/// 免得和输入框互相覆盖。只读行的键（例如来源链接）补丁可以写。
const EDITABLE_METADATA: &[&str] = &["rating", "comment", "note", "link", "tagGroups"];

/// 后端给每个素材种下、编辑器不单独画的系统字段（`sync_metadata.rs::ensure_default_metadata` 和 Eagle 导入种子）：
/// 标题是文件名，类型是扩展名，主色是调色板第一色，收藏是 Eagle 的旧标记。插件补全仍可写入，只是不另起一行。
const SEEDED_METADATA: &[&str] = &["title", "type", "favorite", "color"];

/// 元数据区自定义行跳过的保留字段：编辑器自己画的通用字段，加上后端种下的系统字段。
pub(super) fn is_reserved_metadata(key: &str) -> bool {
    EDITOR_METADATA.contains(&key) || SEEDED_METADATA.contains(&key)
}

/// 预览分派结果。`Native` 是内置的压缩包、PDF、文档和模型视图（`native_preview::builtin_view`）。
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
pub(super) mod search;
pub use search::{
    AdvancedField, DateBound, FilterList, MetadataInput, NumberBound, SearchFilters, SearchRequestDraft, SearchRow,
    SortDirection,
};
/// 搜索候选只从仓库摘要算出，测试直接造候选时用它。
#[cfg(test)]
pub(crate) use search::AssetFacet;

#[derive(Clone, Debug, Default, PartialEq)]
struct MetadataDraft {
    rating: i64,
    comment: String,
    link: String,
    tags: Vec<String>,
    /// 编辑器四个输入以外的元数据，保留原始 JSON 类型：数组、数字、布尔照原样存，写回时也照原样。
    /// ASMR 候选的应用、导入和抓取结果作为补丁写在这里（[`InspectState::apply_metadata_patch`]）。
    custom: BTreeMap<String, Value>,
}

/// 发出去、还没回来的一次保存。同一时刻只有一次（`saving` 挡住第二次），结果按它认是哪个素材的。
#[derive(Clone, Debug)]
struct SaveInFlight {
    asset_id: String,
    /// 发出时的草稿。结果回来时，发出以后才写进草稿的改动按它挑出来，留在新草稿里。
    sent: MetadataDraft,
}

#[derive(Clone, Debug)]
pub enum InspectEffect {
    LoadImage { repo_id: String, path: String },
    LoadText { repo_id: String, path: String, markdown: bool, generation: u64 },
    LoadNative { repo_id: String, path: String, view_id: String, generation: u64 },
    LoadMedia { repo_id: String, path: String, generation: u64 },
    SaveMetadata { repo_id: String, asset_id: String, expected_version: i64, metadata: BTreeMap<String, Value> },
    LoadAsset { repo_id: String, asset_id: String },
    Search { generation: u64, request: SearchRequestDraft },
    /// 预览的音视频接管了播放条，宿主派发后开始播放（Vue 预览页挂载即播放）。
    Autoplay { path: String, generation: u64 },
}

#[derive(Clone, Debug)]
pub enum InspectMessage {
    SetComment(String),
    SetLink(String),
    SetRating(i64),
    AddTag(String),
    RemoveTag(String),
    SaveMetadata,
    AdoptConflict,
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
    /// 打开元数据里的标签菜单。菜单铺在标签区里，跟着元数据卡片排版，不按坐标定位。
    OpenTagMenu,
    CloseTagMenu,
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
    pub(super) error: String,
    pub(super) conflict: String,
    pub(super) target_path: Option<String>,
    pub(super) generation: u64,
    asset_id: Option<String>,
    repo_id: Option<String>,
    expected_version: i64,
    virtual_asset: bool,
    baseline: MetadataDraft,
    draft: MetadataDraft,
    conflict_detail: Option<AssetDetail>,
    saving: bool,
    in_flight: Option<SaveInFlight>,
    pub(super) query: String,
    pub(super) filters: SearchFilters,
    /// 筛选栏草稿、仓库摘要候选和待打开的命中，见 `inspect_search`。
    pub(super) search_ui: search::SearchUi,
    pub(super) filter_bar_open: bool,
    pub(super) searching: bool,
    /// 结果来自一次跑完的带条件搜索。为假时还没有条件，结果区等条件；为真且没有结果时是没有命中。
    pub(super) searched: bool,
    search_generation: u64,
    pub(super) results: Vec<SearchRow>,
    pub(super) search_error: String,
    effects: Vec<InspectEffect>,
    pending_open: Option<SearchRow>,
    pub(super) tag_menu: bool,
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
            error: String::new(),
            conflict: String::new(),
            target_path: None,
            generation: 0,
            asset_id: None,
            repo_id: None,
            expected_version: 0,
            virtual_asset: false,
            baseline: MetadataDraft::default(),
            draft: MetadataDraft::default(),
            conflict_detail: None,
            saving: false,
            in_flight: None,
            query: String::new(),
            filters: SearchFilters::default(),
            search_ui: search::SearchUi::default(),
            filter_bar_open: false,
            searching: false,
            searched: false,
            search_generation: 0,
            results: Vec::new(),
            search_error: String::new(),
            effects: Vec::new(),
            pending_open: None,
            tag_menu: false,
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

    /// 在保存元数据。
    pub(super) fn saving(&self) -> bool {
        self.saving
    }

    /// 选中了文件、详情还没读回（Vue `isLoadingAssetDetail`）。
    pub(super) fn detail_loading(&self) -> bool {
        self.loading && self.kind.is_none() && self.target_path.is_some()
    }

    pub(super) fn dirty(&self) -> bool {
        self.draft != self.baseline
    }

    pub(super) fn can_edit(&self) -> bool {
        self.asset_id.is_some() && !self.virtual_asset && !self.saving
    }

    /// 正在看的素材。
    pub(super) fn current_asset_id(&self) -> Option<&str> {
        self.asset_id.as_deref()
    }

    /// 正在看的素材所在的仓库（素材详情读回时记下的）。
    pub(super) fn current_repo_id(&self) -> Option<&str> {
        self.repo_id.as_deref()
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

    /// 当前素材编辑器四个输入以外的元数据（含还没存完的补丁），原始 JSON 类型。素材详情读回之前是空的。
    pub(super) fn draft_custom(&self) -> &BTreeMap<String, Value> {
        &self.draft.custom
    }

    /// 选择新文件时清掉上一份预览和元数据草稿，等元数据到达后再分派。上一个文件没存的改动先写出去。
    pub(super) fn begin_selection(&mut self, path: &str) {
        self.flush_pending_metadata();
        self.reset_draft();
        self.palette.clear();
        self.generation += 1;
        self.target_path = Some(path.to_string());
        self.kind = None;
        self.body = PreviewBody::Empty;
        self.loading = true;
        self.error.clear();
        bridge::clear_deck(self);
        self.asset_id = None;
        self.virtual_asset = false;
        self.facts = FileFacts::default();
    }

    /// 不再看任何文件（回到目录、换仓库、打开搜索结果之前）。等着自动保存的改动先写出去，不随预览一起丢掉。
    pub(super) fn clear(&mut self) {
        self.flush_pending_metadata();
        self.reset_draft();
        self.generation += 1;
        self.target_path = None;
        self.kind = None;
        self.body = PreviewBody::Empty;
        self.loading = false;
        self.error.clear();
        self.asset_id = None;
        self.palette.clear();
        self.facts = FileFacts::default();
        self.timers.clear_metadata();
        bridge::clear_deck(self);
    }

    /// 草稿和基线回到空白，冲突一起清掉：它们属于上一个素材，不能留到下一个素材的编辑器里。
    /// 先调 [`Self::flush_pending_metadata`]；还剩没写出的改动（上一次保存还没回来时补进来的）只能丢下，记日志。
    fn reset_draft(&mut self) {
        let sent = self.in_flight.as_ref().filter(|flight| self.asset_id.as_deref() == Some(flight.asset_id.as_str()));
        if self.dirty() && sent.is_none_or(|flight| flight.sent != self.draft) {
            eprintln!("Nana 换选时丢下还没写出的元数据改动：{}", self.asset_id.as_deref().unwrap_or("没有素材"));
        }
        self.baseline = MetadataDraft::default();
        self.draft = MetadataDraft::default();
        self.conflict.clear();
        self.conflict_detail = None;
    }

    /// 按扩展名打开预览，并用水合后的元数据草稿替换上一份。
    pub(super) fn note_detail(&mut self, detail: &AssetDetail) {
        let extension = support::extension_of(&detail.summary.extension, &detail.summary.filename);
        let kind = support::classify(&extension);
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
        // 上一个素材的保存还在路上时 `saving` 留着：同一时刻只发一次保存，结果按 `in_flight` 认素材。
        let draft = MetadataDraft::from_detail(detail);
        self.baseline = draft.clone();
        self.draft = draft;
        self.palette = super::palette::from_metadata_entries(&detail.metadata);
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

    fn start_preview(&mut self, repo_id: &str, path: &str, kind: PreviewKind) {
        self.loading = false;
        match kind {
            PreviewKind::Image => {
                self.loading = true;
                self.body = PreviewBody::Empty;
                self.effects.push(InspectEffect::LoadImage { repo_id: repo_id.to_string(), path: path.to_string() });
            }
            PreviewKind::Markdown | PreviewKind::Text => {
                let markdown = matches!(kind, PreviewKind::Markdown);
                self.loading = true;
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
            InspectMessage::SaveMetadata => {
                self.save_metadata();
            }
            InspectMessage::AdoptConflict => self.adopt_conflict(),
            InspectMessage::MetadataSaved(result) => {
                // `error` 归约完以后由状态区报成「保存元数据失败」，先清掉预览留下的失败，免得冒充这次保存的原因。
                self.error.clear();
                match result {
                    Ok((outcome, detail)) => self.note_metadata_result(&outcome, detail),
                    Err(error) => self.note_metadata_error(error),
                }
            }
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
            InspectMessage::OpenTagMenu => self.open_tag_menu(),
            InspectMessage::CloseTagMenu => self.close_tag_menu(),
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
        let metadata = self.draft.to_metadata_map(&self.baseline);
        self.saving = true;
        self.error.clear();
        self.in_flight = Some(SaveInFlight { asset_id: asset_id.clone(), sent: self.draft.clone() });
        self.effects.push(InspectEffect::SaveMetadata { repo_id, asset_id, expected_version: self.expected_version, metadata });
    }

    /// 按原始 JSON 类型给当前素材写一组元数据（ASMR 候选的应用、导入和抓取结果），并立刻保存，
    /// 编辑器里还没存的改动一起写出去。
    ///
    /// `asset_id` 必须还是正在看的素材，补丁不会落到别的文件上。编辑器四个输入的键不收（记日志跳过）。
    /// 上一次保存还在路上时补丁先进草稿，结果回来以后接着保存（见 [`MetadataDraft::rebase`]）。
    /// 拒绝写入时返回给用户看的原因。
    pub(super) fn apply_metadata_patch(&mut self, asset_id: &str, patch: BTreeMap<String, Value>) -> Result<(), String> {
        if self.asset_id.as_deref() != Some(asset_id) {
            eprintln!("Nana 元数据补丁的素材已经不是当前文件：{asset_id}");
            return Err("当前文件已经换了，没有写入".into());
        }
        if self.virtual_asset {
            eprintln!("Nana 虚拟素材不能写元数据补丁：{asset_id}");
            return Err("虚拟条目不能写元数据".into());
        }
        if !self.conflict.is_empty() {
            eprintln!("Nana 元数据冲突未处理，补丁不写入：{asset_id}");
            return Err("版本冲突，先采用服务器版本再写入".into());
        }
        for (key, value) in patch {
            let key = key.trim().to_string();
            if key.is_empty() || EDITABLE_METADATA.contains(&key.as_str()) {
                eprintln!("Nana 元数据补丁跳过编辑器自己的字段：{key}");
                continue;
            }
            self.draft.custom.insert(key, value);
        }
        self.timers.clear_metadata();
        if !self.saving {
            self.save_metadata();
        }
        Ok(())
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

    /// 保存结果。结果按 `in_flight` 认素材：
    /// - 是正在看的素材：成功时换上服务器的版本和元数据，发出以后才写进草稿的改动留着并接着保存；冲突时留住草稿、
    ///   给「采用服务器版本」。
    /// - 换选前写出去的上一个素材：不动当前素材的草稿、冲突和预览，当前素材等着写的补丁接着保存；
    ///   上一个素材的冲突写进 `error`，由状态区报出来。
    fn note_metadata_result(&mut self, outcome: &str, detail: AssetDetail) {
        self.saving = false;
        let asset_id = detail.summary.asset_id.clone();
        let flight = self.in_flight.take().filter(|flight| flight.asset_id == asset_id);
        if outcome != "success" && outcome != "conflict" {
            self.note_metadata_error(format!("元数据结果无法识别：{outcome}"));
            return;
        }
        if self.asset_id.as_deref() != Some(asset_id.as_str()) {
            eprintln!("Nana 收到别的素材的元数据保存结果：{asset_id}（{outcome}）");
            self.save_pending();
            if outcome == "conflict" {
                self.error = format!("「{}」版本冲突，修改没有写入", detail.summary.filename);
            }
            return;
        }
        if outcome == "conflict" {
            eprintln!("Nana 元数据版本冲突：{asset_id}");
            self.conflict = "版本冲突，未写入".into();
            self.conflict_detail = Some(detail);
            return;
        }
        let server = MetadataDraft::from_detail(&detail);
        self.draft = match flight {
            Some(flight) => self.draft.rebase(&flight.sent, &server),
            None => server.clone(),
        };
        self.baseline = server;
        self.expected_version = detail.summary.version;
        self.virtual_asset = detail.summary.is_virtual;
        self.conflict.clear();
        self.conflict_detail = None;
        self.error.clear();
        self.save_pending();
    }

    /// 保存失败。失败的是换选前的上一个素材时，当前素材等着写的补丁照常写出去；失败的是当前素材时
    /// 草稿留着，不立刻重试（同样的原因多半还会失败），下一次编辑再自动保存。
    fn note_metadata_error(&mut self, error: String) {
        eprintln!("Nana 保存元数据失败：{error}");
        self.saving = false;
        let flight = self.in_flight.take();
        if flight.is_some_and(|flight| self.asset_id.as_deref() != Some(flight.asset_id.as_str())) {
            self.save_pending();
        }
        self.error = error;
    }

    /// 上一次保存回来以后，当前素材的草稿里还有没写出去的改动（保存期间补进来的补丁或编辑）时接着保存。
    /// 冲突没处理时不发，免得带着旧版本号再撞一次。
    fn save_pending(&mut self) {
        if self.dirty() && self.conflict.is_empty() {
            self.save_metadata();
        }
    }

    fn note_body(&mut self, path: String, markdown: bool, generation: u64, result: Result<support::PreviewText, String>) {
        if generation != self.generation || self.target_path.as_deref() != Some(path.as_str()) {
            eprintln!("Nana 忽略过期的文本预览：{path}");
            return;
        }
        self.loading = false;
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
                .filter(|entry| !EDITABLE_METADATA.contains(&entry.key.as_str()))
                .map(|entry| (entry.key.clone(), entry.value.clone()))
                .collect(),
        }
    }

    /// 保存请求的元数据：只写和 `baseline` 不同的，也就是这次改过的。后端按键覆盖写入，没动过的不回写：
    /// 旧格式的标签组（`{ label, tags }`）不会因为只改了评分就被改写成展平的字符串，小数评分、只有旧键 note 的注释
    /// 也照原样留着；补丁写进来的字段按原始 JSON 类型写，数组和数字不会变成字符串。
    fn to_metadata_map(&self, baseline: &MetadataDraft) -> BTreeMap<String, Value> {
        let mut metadata = BTreeMap::new();
        if self.rating != baseline.rating {
            metadata.insert("rating".into(), Value::from(self.rating));
        }
        if self.comment != baseline.comment {
            metadata.insert("comment".into(), Value::String(self.comment.trim().to_string()));
        }
        if self.link != baseline.link {
            metadata.insert("link".into(), Value::String(self.link.trim().to_string()));
        }
        if self.tags != baseline.tags {
            metadata.insert("tagGroups".into(), Value::Array(self.tags.iter().cloned().map(Value::String).collect()));
        }
        for (key, value) in &self.custom {
            if baseline.custom.get(key) != Some(value) {
                metadata.insert(key.clone(), value.clone());
            }
        }
        metadata
    }

    /// 保存成功以后的新草稿：以服务器读回的 `server` 为底，只留下发出（`sent`）以后草稿里又改过的字段。
    ///
    /// 算法：评分、注释、链接、标签组逐个比，现在的值和发出时不同就用现在的，否则用服务器的；补丁字段按键同样比，
    /// 发出以后新写的键或改过的值留下。这样保存期间补进来的候选不会被这次结果冲掉，也不会把服务器整理过的值
    /// （例如去掉首尾空白的注释）换回旧的。
    fn rebase(&self, sent: &MetadataDraft, server: &MetadataDraft) -> MetadataDraft {
        fn pick<T: Clone + PartialEq>(now: &T, sent: &T, server: &T) -> T {
            if now != sent { now.clone() } else { server.clone() }
        }
        let mut custom = server.custom.clone();
        for (key, value) in &self.custom {
            if sent.custom.get(key) != Some(value) {
                custom.insert(key.clone(), value.clone());
            }
        }
        MetadataDraft {
            rating: pick(&self.rating, &sent.rating, &server.rating),
            comment: pick(&self.comment, &sent.comment, &server.comment),
            link: pick(&self.link, &sent.link, &server.link),
            tags: pick(&self.tags, &sent.tags, &server.tags),
            custom,
        }
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
    // 元数据保存的失败没有就近显示，交给状态区。
    let saving = matches!(message, InspectMessage::MetadataSaved(_));
    model.inspect.reduce(writable, repo_id.as_deref(), message);
    if saving && !model.inspect.error.is_empty() {
        let failure = format!("保存元数据失败：{}", model.inspect.error);
        model.status.fail(super::status::FailureSource::Metadata, failure);
    }
    bridge::apply(model, follow);
    search::settle(model);
    None
}

#[path = "inspect_support.rs"]
pub(super) mod support;
pub use support::prepare_text;

#[cfg(test)]
#[path = "inspect_tests.rs"]
mod tests;
