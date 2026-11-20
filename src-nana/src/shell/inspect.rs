//! 预览、元数据和搜索。按扩展名分派图片、文本、音视频和内置文档，Three.js 不嵌进 Runtime。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::backend::services::repository::{AssetDetail, PlaybackSessionState};

use super::ShellViewModel;
use super::files::repository_is_writable;

mod bridge;
pub(super) mod native_preview;
pub(super) use bridge::ArchiveEntryKind;
pub use bridge::NativeLoad;
pub(super) use bridge::PageFrame;
pub(crate) use bridge::frame_at;
pub(crate) use bridge::orbit_drag;
pub(super) use bridge::rgba_png_data_url;

mod draft;
mod preview_state;

/// 元数据编辑器自己画的通用字段，覆盖 `docs/api-design.md` 的
/// Generic file metadata keys：评分、注释（含旧键 note）、链接和标签组四个输入，以及入库、创建、修改时间、
/// 尺寸、原始大小、调色板、来源标题和来源链接的只读行。自定义行不再重复画这些键。
const EDITOR_METADATA: &[&str] = &[
    "rating",
    "comment",
    "note",
    "link",
    "tagGroups",
    "addedToLibraryAt",
    "fileCreatedAt",
    "fileModifiedAt",
    "width",
    "height",
    "originalSizeBytes",
    "thumbnailPalette",
    "palette",
    "originTitle",
    "sourceUrl",
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
    Document {
        markdown: bool,
        text: String,
        truncated_at: Option<u64>,
    },
    Media(PlaybackSessionState),
    Native {
        view_id: String,
        label: String,
        content: String,
    },
    Failed(String),
}

pub(super) mod search;
/// 搜索候选只从仓库摘要算出，测试直接造候选时用它。
#[cfg(test)]
pub(crate) use search::AssetFacet;
pub use search::{
    AdvancedField, DateBound, FilterList, MetadataInput, NumberBound, SearchFilters,
    SearchRequestDraft, SearchRow, SortDirection,
};

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
    /// 请求里写了标签组：后端会重排这个素材的索引标签，侧栏计数和标签候选要跟着重读摘要。
    tags_written: bool,
}

#[derive(Clone, Debug)]
pub enum InspectEffect {
    LoadImage {
        repo_id: String,
        path: String,
    },
    LoadText {
        repo_id: String,
        path: String,
        markdown: bool,
        generation: u64,
    },
    LoadNative {
        repo_id: String,
        path: String,
        view_id: String,
        generation: u64,
    },
    /// 从已经列出的压缩包里只取这一条的字节。`path` 是最外层压缩包，`through` 是先取出的嵌套 zip/7z，`entry` 是最内层里的路径。
    LoadArchiveEntry {
        repo_id: String,
        path: String,
        entry: String,
        through: Vec<String>,
        generation: u64,
    },
    /// 读音视频。`virtual_entry` 是在线条目：先经 `momobako.playback.prepare` 准备播放源，再读准备好的本地文件。
    LoadMedia {
        repo_id: String,
        path: String,
        generation: u64,
        virtual_entry: bool,
    },
    /// 音频舞台出来以后再读歌词。`lyric_path` 为空时读同级 `.lrc`。
    LoadLyrics {
        repo_id: String,
        audio_path: String,
        generation: u64,
        lyric_path: Option<String>,
    },
    SaveMetadata {
        repo_id: String,
        asset_id: String,
        expected_version: i64,
        metadata: BTreeMap<String, Value>,
    },
    LoadAsset {
        repo_id: String,
        asset_id: String,
    },
    /// 同一个仓库刷新以后重读素材详情。回来只更新详情卡叠层，不改选中路径。
    ReloadAsset {
        repo_id: String,
        asset_id: String,
    },
    Search {
        generation: u64,
        request: SearchRequestDraft,
    },
    /// 预览的音视频接管了播放条，宿主派发后开始播放（预览页一打开就播放）。
    Autoplay {
        path: String,
        generation: u64,
    },
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
    BodyLoaded {
        path: String,
        markdown: bool,
        generation: u64,
        result: Result<support::PreviewText, String>,
    },
    NativeLoaded {
        path: String,
        generation: u64,
        result: Result<bridge::NativeLoad, String>,
    },
    MediaLoaded {
        path: String,
        generation: u64,
        result: Result<PlaybackSessionState, String>,
        pcm: Option<super::player::PreviewPcm>,
        frames: Option<Vec<support::VideoFrame>>,
        /// 在线条目准备播放源的结果：后端写下的访问、失败能不能重试。本地文件是默认值。
        prep: super::SourcePrep,
    },
    /// 歌词文件读完。空正文是没有歌词，舞台从「读取歌词...」换成「暂无歌词」。
    LyricsLoaded {
        path: String,
        generation: u64,
        text: String,
    },
    /// 在线条目的播放源准备失败以后，预览框里的「重试」。
    RetryMedia,
    TurnPage(i32),
    Orbit {
        yaw: f32,
        zoom: f32,
    },
    /// 选中压缩包当前层里已经列出的目录。不进入，也不再读压缩包。
    SelectArchive(String),
    /// 进入压缩包当前层里已经解出的目录。空路径回到根。不再读压缩包。
    EnterArchive(String),
    /// 打开 zip/7z 名单里的一个文件。取不出或不支持时停在名单上。
    OpenArchiveFile(String),
    /// 包内这一条的预览。取不出时仍停在当前名单上。
    ArchiveEntryLoaded {
        path: String,
        entry: String,
        through: Vec<String>,
        generation: u64,
        result: Result<bridge::ArchiveEntryPreview, String>,
    },
    /// 宿主派发的自动播放。代次或文件对不上就不动。
    Autoplay {
        path: String,
        generation: u64,
    },
    SetQuery(String),
    ToggleFilterBar,
    CloseFilterBar,
    ToggleFilter {
        key: FilterList,
        value: String,
    },
    SetMinimumRating(Option<f64>),
    SetMetadataInput {
        key: MetadataInput,
        value: String,
    },
    SubmitMetadataInput(MetadataInput),
    SetAdvanced {
        field: AdvancedField,
        value: String,
    },
    SetSortDirection(SortDirection),
    ApplyAdvanced,
    ClearFilters,
    ApplyShortcut {
        metadata: String,
        sort_field: String,
        sort_direction: SortDirection,
    },
    RunSearch,
    SearchFinished {
        generation: u64,
        result: Result<Vec<SearchRow>, String>,
    },
    OpenHit {
        repo_id: String,
        asset_id: String,
    },
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

/// 压缩包名单还在时，旁边那一块内部文件提示。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ArchiveFileNotice {
    /// 扩展名不能在名单旁画出来。里面是文件名。
    Inline(String),
    /// 取出或解码失败。里面是原因。
    Failed(String),
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
    /// 素材详情带来的本机绝对路径。预览页打开和定位在列表里找不到这条时用它。
    local_absolute_path: Option<String>,
    /// 当前音视频预览失败在准备播放源上（在线条目），预览框给「重试」。
    pub(super) media_retry: bool,
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
    /// zip/7z 已经读出的条目名。点进下一层只用它，不再打开压缩包。
    pub(super) archive: Option<bridge::ArchiveCatalog>,
    /// 压缩包预览当前停在的内部目录。空字符串是根。
    pub(super) archive_directory: String,
    /// 单击选中、还没进入的目录。路径从压缩包根算起，和进入时用的是同一套。
    pub(super) archive_selected: Option<String>,
    /// 从外到内已经点开的嵌套 zip/7z。最外层磁盘压缩包仍在 [`Self::archive`]。
    pub(super) archive_nests: Vec<bridge::ArchiveNest>,
    /// 正在看的包内文件。没有时画当前层名单。
    pub(super) archive_entry: Option<bridge::ArchiveEntryPreview>,
    /// 已经发出去、还没回来的包内路径。对不上的结果丢掉。
    pub(super) archive_request: Option<String>,
    /// 名单还在、没有在途读取时旁边写的提示。空的时候写「选择内部文件进行预览」。
    pub(super) archive_notice: Option<ArchiveFileNotice>,
    /// 音频预览的歌词行。空、且不在读取中时舞台写「暂无歌词」。
    pub(super) lyrics: Vec<super::inspect_lyrics::LyricLine>,
    /// 歌词文件还在读。舞台写「读取歌词...」，晚到的结果按代次丢掉。
    pub(super) lyrics_pending: bool,
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
            local_absolute_path: None,
            media_retry: false,
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
            archive: None,
            archive_directory: String::new(),
            archive_selected: None,
            archive_nests: Vec::new(),
            archive_entry: None,
            archive_request: None,
            archive_notice: None,
            lyrics: Vec::new(),
            lyrics_pending: false,
        }
    }
}

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

    /// 选中了文件、详情还没读回。
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
    pub(crate) fn current_asset_id(&self) -> Option<&str> {
        self.asset_id.as_deref()
    }

    /// 正在看的路径。预览页打开时就是这一条。
    pub(crate) fn open_path(&self) -> Option<&str> {
        self.target_path.as_deref()
    }

    /// 正在看的素材所在的仓库（素材详情读回时记下的）。
    pub(crate) fn current_repo_id(&self) -> Option<&str> {
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

    /// 已经读回或保存过的注释。编辑器里还没写回的草稿不在这里。
    pub(super) fn saved_comment(&self) -> &str {
        &self.baseline.comment
    }

    /// 已经读回或保存过的链接。编辑器里还没写回的草稿不在这里。
    pub(super) fn saved_link(&self) -> &str {
        &self.baseline.link
    }

    pub(super) fn draft_tags(&self) -> &[String] {
        &self.draft.tags
    }

    /// 当前素材编辑器四个输入以外的元数据（含还没存完的补丁），原始 JSON 类型。素材详情读回之前是空的。
    pub(crate) fn draft_custom(&self) -> &BTreeMap<String, Value> {
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
        self.archive = None;
        self.archive_directory.clear();
        self.archive_selected = None;
        self.archive_nests.clear();
        self.archive_entry = None;
        self.archive_request = None;
        self.archive_notice = None;
        self.lyrics.clear();
        self.lyrics_pending = false;
        bridge::clear_deck(self);
        self.asset_id = None;
        self.virtual_asset = false;
        self.local_absolute_path = None;
        self.media_retry = false;
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
        self.media_retry = false;
        self.error.clear();
        self.asset_id = None;
        self.virtual_asset = false;
        self.local_absolute_path = None;
        self.palette.clear();
        self.facts = FileFacts::default();
        self.archive = None;
        self.archive_directory.clear();
        self.archive_selected = None;
        self.archive_nests.clear();
        self.archive_entry = None;
        self.archive_request = None;
        self.archive_notice = None;
        self.lyrics.clear();
        self.lyrics_pending = false;
        self.timers.clear_metadata();
        bridge::clear_deck(self);
    }

    /// 素材详情已经对上 `path` 时，返回虚拟标记和本机绝对路径。详情还没回来时是 `None`。
    pub(super) fn noted_host_facts(&self, path: &str) -> Option<(bool, Option<String>)> {
        if self.asset_id.is_none() || self.target_path.as_deref() != Some(path) {
            return None;
        }
        Some((self.virtual_asset, self.local_absolute_path.clone()))
    }

    /// 草稿和基线回到空白，冲突一起清掉：它们属于上一个素材，不能留到下一个素材的编辑器里。
    /// 先调 [`Self::flush_pending_metadata`]；还剩没写出的改动（上一次保存还没回来时补进来的）只能丢下，记日志。
    fn reset_draft(&mut self) {
        let sent = self
            .in_flight
            .as_ref()
            .filter(|flight| self.asset_id.as_deref() == Some(flight.asset_id.as_str()));
        if self.dirty() && sent.is_none_or(|flight| flight.sent != self.draft) {
            eprintln!(
                "Nana 换选时丢下还没写出的元数据改动：{}",
                self.asset_id.as_deref().unwrap_or("没有素材")
            );
        }
        self.baseline = MetadataDraft::default();
        self.draft = MetadataDraft::default();
        self.conflict.clear();
        self.conflict_detail = None;
    }

    pub(super) fn reduce(
        &mut self,
        writable: bool,
        active_repo: Option<&str>,
        message: InspectMessage,
    ) {
        let hint = tags::clock_hint(&message);
        match message {
            InspectMessage::SetComment(value) => self.draft.comment = value,
            InspectMessage::SetLink(value) => self.draft.link = value,
            InspectMessage::SetRating(rating) => {
                let rating = rating.clamp(0, 5);
                self.draft.rating = if self.draft.rating == rating {
                    0
                } else {
                    rating
                };
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
            InspectMessage::BodyLoaded {
                path,
                markdown,
                generation,
                result,
            } => {
                self.note_body(path, markdown, generation, result);
            }
            InspectMessage::NativeLoaded {
                path,
                generation,
                result,
            } => native_preview::note_loaded(self, path, generation, result),
            InspectMessage::MediaLoaded {
                path,
                generation,
                result,
                frames,
                prep,
                ..
            } => {
                support::note_media(self, path, generation, result, frames, &prep);
            }
            InspectMessage::LyricsLoaded {
                path,
                generation,
                text,
            } => support::note_lyrics(self, path, generation, text),
            InspectMessage::RetryMedia => support::retry_media(self),
            InspectMessage::TurnPage(delta) => {
                if !native_preview::turn_archive_pdf(self, delta) {
                    bridge::turn(self, delta);
                }
            }
            InspectMessage::Orbit { yaw, zoom } => {
                if !native_preview::orbit_archive_model(self, yaw, zoom) {
                    bridge::orbit(self, yaw, zoom);
                }
            }
            InspectMessage::SelectArchive(path) => native_preview::select_directory(self, &path),
            InspectMessage::EnterArchive(path) => native_preview::enter_directory(self, &path),
            InspectMessage::OpenArchiveFile(path) => native_preview::open_archive_file(self, &path),
            InspectMessage::ArchiveEntryLoaded {
                path,
                entry,
                through,
                generation,
                result,
            } => {
                native_preview::note_archive_entry(self, path, entry, through, generation, result);
            }
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
}

/// 播放器改了进度或状态后，预览页的视频换到对应画面。
pub(crate) fn sync_preview_frame(model: &mut ShellViewModel) {
    bridge::show_video_frame(model);
}

mod refresh_asset;

pub(super) fn reduce_message(
    model: &mut ShellViewModel,
    message: super::ShellMessage,
) -> Option<super::ShellMessage> {
    search::observe(model, &message);
    let super::ShellMessage::Inspect(message) = message else {
        return Some(message);
    };
    let writable = model
        .workspace
        .active_repository()
        .is_some_and(|repository| {
            repository_is_writable(&repository.status, &repository.capabilities)
        });
    let repo_id = model.workspace.active_repo_id.clone();
    let follow = bridge::follow(&message);
    let access = support::preview_access(&model.inspect, &message);
    // 元数据保存的失败没有就近显示，交给状态区。
    let saving = matches!(message, InspectMessage::MetadataSaved(_));
    let retagged = model.inspect.retagged_repository(&message);
    model.inspect.reduce(writable, repo_id.as_deref(), message);
    if saving && !model.inspect.error.is_empty() {
        let failure = format!("保存元数据失败：{}", model.inspect.error);
        model
            .status
            .fail(super::status::FailureSource::Metadata, failure);
    }
    match access {
        support::PreviewAccess::None => {}
        support::PreviewAccess::Recorded { repo_id, path, at } => {
            model.note_entry_access(&repo_id, &path, &at)
        }
        support::PreviewAccess::Record { path } => {
            let repo = model
                .inspect
                .repo_id
                .clone()
                .or(repo_id)
                .unwrap_or_default();
            model.request_entry_access(&repo, &path);
        }
    }
    // 标签变了：静默重读仓库摘要，侧栏计数、筛选栏和标签菜单的候选跟着换（摘要里这项素材换成新的）。
    // 文件行上的元数据和索引标签由文件表面就地换（`shell/files/reduce.rs`）。
    if let Some(repo_id) = retagged
        .filter(|repo_id| model.workspace.active_repo_id.as_deref() == Some(repo_id.as_str()))
    {
        model
            .workspace
            .effects
            .push(super::WorkspaceEffect::LoadSnapshotSilent { repo_id });
    }
    bridge::apply(model, follow);
    search::settle(model);
    None
}

pub(super) mod support;
pub use support::prepare_text;

/// 包内文件读完以后的消息。类型留在这个模块里，派发侧不用单独点开预览结构。
pub(crate) fn archive_entry_loaded(
    path: String,
    entry: String,
    through: Vec<String>,
    generation: u64,
    result: Result<bridge::ArchiveEntryPreview, String>,
) -> InspectMessage {
    InspectMessage::ArchiveEntryLoaded {
        path,
        entry,
        through,
        generation,
        result,
    }
}

#[cfg(test)]
mod tests;
