//! 预览扩展名分派、文本字节上限、可解码音频的预览会话，以及搜索条件解析。
//!
//! Markdown 先于普通文本。压缩包、PDF、文档和模型走内置绘制，不因 Vue 贡献改回升级提示。
//! 能解出的 WAV、mp3、flac、ogg 同时带上 PCM，供预览和播放条共用同一出声游标。

#[path = "video_preview.rs"]
mod video_preview;
pub(crate) use video_preview::VideoFrame;
#[cfg(test)]
pub(super) fn sample_uncompressed() -> Vec<u8> {
    video_preview::sample_uncompressed()
}

use serde_json::Value;

use crate::backend::services::repository::{MetadataEntry, PlaybackSessionState};
use crate::host_api::{PlaybackMediaCapabilities, PlaybackMediaPlugin};
use crate::plugin_api::NativeContributionKind;

use super::{
    DateBound, FilterList, MatchMode, NumberBound, PreviewBinding, PreviewKind, SearchFilters, SearchRequestDraft,
    SortDirection,
};

const TEXT_BYTE_LIMIT: usize = 768 * 1024;

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"];
const MARKDOWN_EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn", "mdx"];
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "text", "log", "csv", "tsv", "json", "jsonl", "yaml", "yml", "toml", "xml", "html", "css", "scss", "sass",
    "less", "js", "jsx", "ts", "tsx", "vue", "rs", "py", "rb", "go", "java", "c", "h", "cpp", "hpp", "cs", "php", "sh",
    "bash", "zsh", "ps1", "bat", "cmd", "ini", "cfg", "conf", "env", "gitignore", "gitattributes",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "m4v"];
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "ogg", "flac", "m4a", "aac", "opus"];
/// 扩展名分派。Markdown 先于普通文本。能画的文档、压缩包和模型不再要求升级。
pub fn classify(extension: &str, contributions: &[PreviewBinding]) -> PreviewKind {
    let extension = normalize_extension(extension);
    if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        return PreviewKind::Image;
    }
    if MARKDOWN_EXTENSIONS.contains(&extension.as_str()) {
        return PreviewKind::Markdown;
    }
    if TEXT_EXTENSIONS.contains(&extension.as_str()) {
        return PreviewKind::Text;
    }
    if VIDEO_EXTENSIONS.contains(&extension.as_str()) || AUDIO_EXTENSIONS.contains(&extension.as_str()) {
        return PreviewKind::Media;
    }
    if let Some(binding) = find_contribution(&extension, contributions) {
        if super::native_preview::renders_view(&binding.contribution.view_id) {
            return PreviewKind::Native {
                view_id: binding.contribution.view_id.clone(),
                label: binding.contribution.label.clone(),
            };
        }
    }
    if let Some((view_id, label)) = super::native_preview::fallback_view(&extension) {
        return PreviewKind::Native { view_id: view_id.to_string(), label: label.to_string() };
    }
    if let Some(binding) = find_contribution(&extension, contributions) {
        return PreviewKind::Native {
            view_id: binding.contribution.view_id.clone(),
            label: binding.contribution.label.clone(),
        };
    }
    PreviewKind::Unsupported
}

/// 文本超过 Vue 的 768KiB 上限时失败，不把截断内容当成完整预览。
pub fn prepare_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > TEXT_BYTE_LIMIT {
        return Err(format!("文本超过 {} 字节", TEXT_BYTE_LIMIT));
    }
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// 音视频预览先排队读文件。可解码音频成功后才允许播放控制。
pub(super) fn begin_media(state: &mut super::InspectState, repo_id: &str, path: &str) {
    state.loading = true;
    state.activity = "正在读取媒体…".into();
    state.body = super::PreviewBody::Empty;
    state.error.clear();
    state.effects.push(super::InspectEffect::LoadMedia {
        repo_id: repo_id.to_string(),
        path: path.to_string(),
        generation: state.generation,
    });
}

/// 代次或路径过期时保留当前预览。可解码音频写入 paused 会话，其余失败。
pub(super) fn note_media(
    state: &mut super::InspectState,
    path: String,
    generation: u64,
    result: Result<PlaybackSessionState, String>,
    frames: Option<Vec<VideoFrame>>,
) {
    if generation != state.generation || state.target_path.as_deref() != Some(path.as_str()) {
        eprintln!("Nana 忽略过期的音视频预览：{path}");
        return;
    }
    state.loading = false;
    state.activity.clear();
    match result {
        Ok(session) => {
            state.error.clear();
            super::bridge::install_video(state, frames);
            state.body = super::PreviewBody::Media(session);
        }
        Err(error) => {
            eprintln!("Nana 音视频预览失败：{error}");
            super::bridge::install_video(state, None);
            let repo_id = state.repo_id.clone().unwrap_or_default();
            state.body = super::PreviewBody::Media(failed_session(&repo_id, error.clone()));
            state.error = error;
            state.activity = state.error.clone();
        }
    }
}

/// 一次预览解出的会话、音轨和可选画面。
#[derive(Clone, Debug)]
pub struct MediaParts {
    pub session: PlaybackSessionState,
    pub pcm: Option<crate::shell::player::PreviewPcm>,
    pub frames: Option<Vec<video_preview::VideoFrame>>,
}

/// WAV 优先。失败后再试 mp3、flac、ogg/Vorbis，然后解视频容器。没有画面的 m4a、aac、opus 走媒体基础音轨。认不出的字节仍是没有解码器。
#[cfg(test)]
pub(crate) fn preview_media_session(repo_id: &str, bytes: &[u8]) -> Result<PlaybackSessionState, String> {
    preview_media_parts(repo_id, bytes).map(|parts| parts.session)
}

/// 会话、PCM 和画面一次解出。播放时由壳层装进 winmm 游标；测试构建不打开设备。
pub(crate) fn preview_media_parts(repo_id: &str, bytes: &[u8]) -> Result<MediaParts, String> {
    match crate::shell::player::pcm_from_bytes(bytes) {
        Ok(pcm) => {
            let duration = pcm.duration_ms;
            Ok(MediaParts { session: paused_audio_session(repo_id, duration), pcm: Some(pcm), frames: None })
        }
        Err(_) => match video_preview::open_clip(repo_id, bytes) {
            Ok((session, pcm, frames)) => Ok(MediaParts { session, pcm, frames: Some(frames) }),
            Err(error) => {
                eprintln!("Nana 视频预览失败：{error}");
                Err(error)
            }
        },
    }
}

/// 已装载的音频允许播放、暂停、跳转和音量。失败会话仍拒绝控制。
pub(super) fn transport_plugin(session: &PlaybackSessionState) -> TransportPlugin {
    if session.error.is_none() && session.duration_ms.is_some() {
        TransportPlugin::Ready
    } else {
        TransportPlugin::Missing
    }
}

pub(super) enum TransportPlugin {
    Ready,
    Missing,
}

impl PlaybackMediaPlugin for TransportPlugin {
    fn load(&mut self, _source: &str) -> Result<PlaybackMediaCapabilities, String> {
        Err("预览控制不重复装载".into())
    }

    fn play(&mut self) -> Result<(), String> {
        match self {
            Self::Ready => Ok(()),
            Self::Missing => Err("没有原生解码器".into()),
        }
    }

    fn pause(&mut self) -> Result<(), String> {
        self.play()
    }

    fn seek(&mut self, _position_ms: u64) -> Result<(), String> {
        self.play()
    }

    fn set_volume(&mut self, _volume: f32) -> Result<(), String> {
        self.play()
    }

    fn dispose(&mut self) {}
}

fn paused_audio_session(repo_id: &str, duration_ms: u64) -> PlaybackSessionState {
    PlaybackSessionState {
        status: "paused".into(),
        duration_ms: Some(duration_ms),
        can_seek: true,
        can_volume: true,
        error: None,
        ..fresh_session(repo_id)
    }
}

fn failed_session(repo_id: &str, error: String) -> PlaybackSessionState {
    PlaybackSessionState {
        status: "failed".into(),
        error: Some(error),
        ..fresh_session(repo_id)
    }
}

fn fresh_session(repo_id: &str) -> PlaybackSessionState {
    PlaybackSessionState {
        session_id: format!("preview-{repo_id}"),
        repo_id: repo_id.to_string(),
        playlist_id: String::new(),
        playlist_item_id: None,
        status: "loading".into(),
        current_time_ms: 0,
        duration_ms: None,
        volume: 1.0,
        can_seek: false,
        can_volume: false,
        error: None,
        updated_at: String::new(),
    }
}

pub(super) fn build_search_request(query: &str, filters: &SearchFilters, active_repo: Option<&str>) -> SearchRequestDraft {
    let mut metadata_filters = Vec::new();
    metadata_filters.extend(filters.colors.iter().cloned().map(|value| ("color".into(), value)));
    metadata_filters.extend(filters.shapes.iter().cloned().map(|value| ("shape".into(), value)));
    metadata_filters.extend(parse_metadata(&filters.metadata_filters));
    let sort_field = filters.sort_field.trim();
    SearchRequestDraft {
        query: query.to_string(),
        repo_id: filters.has_active_filters().then(|| active_repo.map(str::to_string)).flatten(),
        exclude_query: nonempty(&filters.exclude_query),
        tags: normalize_values(&filters.tags),
        formats: normalize_values(&filters.formats),
        metadata_filters,
        exclude_tags: normalize_values(&filters.exclude_tags),
        exclude_formats: normalize_values(&filters.exclude_formats),
        exclude_metadata_filters: parse_metadata(&filters.exclude_metadata_filters),
        exclude_path_prefixes: parse_paths(&filters.exclude_path_prefixes),
        exclude_number_filters: parse_numbers(&filters.exclude_number_filters),
        exclude_date_filters: parse_dates(&filters.exclude_date_filters),
        number_filters: parse_numbers(&filters.number_filters),
        date_filters: parse_dates(&filters.date_filters),
        match_mode: (filters.match_mode == MatchMode::Or).then(|| "or".to_string()),
        sort_field: (!sort_field.is_empty()).then(|| sort_field.to_string()),
        sort_direction: (!sort_field.is_empty()).then(|| match filters.sort_direction {
            SortDirection::Asc => "asc".to_string(),
            SortDirection::Desc => "desc".to_string(),
        }),
        limit: filters.limit,
        min_rating: filters.min_rating,
    }
}

pub(super) fn toggle_filter(filters: &mut SearchFilters, key: FilterList, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    let list = match key {
        FilterList::Tags => &mut filters.tags,
        FilterList::Formats => &mut filters.formats,
        FilterList::Colors => &mut filters.colors,
        FilterList::Shapes => &mut filters.shapes,
        FilterList::ExcludeTags => &mut filters.exclude_tags,
        FilterList::ExcludeFormats => &mut filters.exclude_formats,
    };
    if let Some(index) = list.iter().position(|item| item == value) {
        list.remove(index);
    } else {
        list.push(value.to_string());
        *list = normalize_values(list);
    }
}

fn find_contribution<'a>(extension: &str, contributions: &'a [PreviewBinding]) -> Option<&'a PreviewBinding> {
    contributions.iter().rev().find(|binding| {
        binding.contribution.kind == NativeContributionKind::Preview
            && binding.extensions.iter().any(|item| item.eq_ignore_ascii_case(extension))
    })
}

pub(super) fn extension_of(extension: &str, filename: &str) -> String {
    let extension = normalize_extension(extension);
    if !extension.is_empty() {
        return extension;
    }
    filename.rsplit_once('.').map(|(_, suffix)| normalize_extension(suffix)).unwrap_or_default()
}

fn normalize_extension(extension: &str) -> String {
    extension.trim().trim_start_matches('.').to_ascii_lowercase()
}

pub(super) fn metadata_string(metadata: &[MetadataEntry], key: &str) -> Option<String> {
    metadata.iter().find(|entry| entry.key == key).and_then(|entry| entry.value.as_str().map(str::to_string))
}

pub(super) fn metadata_rating(metadata: &[MetadataEntry]) -> i64 {
    metadata
        .iter()
        .find(|entry| entry.key == "rating")
        .and_then(|entry| entry.value.as_i64().or_else(|| entry.value.as_f64().map(|value| value.round() as i64)))
        .map(|value| value.clamp(0, 5))
        .unwrap_or(0)
}

pub(super) fn metadata_tags(metadata: &[MetadataEntry]) -> Option<Vec<String>> {
    let value = metadata.iter().find(|entry| entry.key == "tagGroups")?.value.as_array()?;
    let mut tags = Vec::new();
    for item in value {
        if let Some(tag) = item.as_str() {
            let tag = tag.trim();
            if !tag.is_empty() && !tags.iter().any(|existing| existing == tag) {
                tags.push(tag.to_string());
            }
        }
    }
    Some(tags)
}

pub(super) fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn normalize_values(values: &[String]) -> Vec<String> {
    let mut unique = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || unique.iter().any(|item| item == value) {
            continue;
        }
        unique.push(value.to_string());
    }
    unique
}

pub(super) fn split_list(value: &str) -> Vec<String> {
    let mut unique = Vec::new();
    for item in value.split([',', '，', '\n']) {
        let item = item.trim();
        if item.is_empty() || unique.iter().any(|existing| existing == item) {
            continue;
        }
        unique.push(item.to_string());
    }
    unique
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

pub(super) fn parse_limit(value: &str) -> Option<usize> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    value.parse::<usize>().ok().filter(|limit| *limit > 0)
}

fn parse_metadata(value: &str) -> Vec<(String, String)> {
    value
        .split(['\n', ',', '，'])
        .filter_map(|item| {
            let item = item.trim();
            let index = item.find('=')?;
            let key = item[..index].trim();
            let filter_value = item[index + 1..].trim();
            if key.is_empty() || filter_value.is_empty() {
                None
            } else {
                Some((key.to_string(), filter_value.to_string()))
            }
        })
        .collect()
}

fn parse_numbers(value: &str) -> Vec<NumberBound> {
    value
        .split(['\n', ',', '，'])
        .filter_map(|item| {
            let item = item.trim();
            let (key, range) = item.split_once('=')?;
            let (min_text, max_text) = range.split_once("..").unwrap_or((range, ""));
            let min = bound_number(min_text);
            let max = bound_number(max_text);
            if key.trim().is_empty() || (min.is_none() && max.is_none()) {
                None
            } else {
                Some(NumberBound { key: key.trim().to_string(), min, max })
            }
        })
        .collect()
}

fn bound_number(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    value.parse::<f64>().ok().filter(|number| number.is_finite()).map(|number| number.to_string())
}

fn parse_dates(value: &str) -> Vec<DateBound> {
    value
        .split(['\n', ',', '，'])
        .filter_map(|item| {
            let item = item.trim();
            let (key, range) = item.split_once('=')?;
            let (from, to) = range.split_once("..").unwrap_or((range, ""));
            let from = nonempty(from);
            let to = nonempty(to);
            if key.trim().is_empty() || (from.is_none() && to.is_none()) {
                None
            } else {
                Some(DateBound { key: key.trim().to_string(), from, to })
            }
        })
        .collect()
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

/// 把 Vue 详情卡里的只读行从元数据抄进事实。缺字段留空，不补当前时间。
pub(super) fn fill_recorded_facts(facts: &mut super::FileFacts, metadata: &[MetadataEntry]) {
    facts.added_to_library_at = metadata_string(metadata, "addedToLibraryAt").unwrap_or_default();
    facts.file_created_at = metadata_string(metadata, "fileCreatedAt").unwrap_or_default();
    facts.file_modified_meta = metadata_string(metadata, "fileModifiedAt").unwrap_or_default();
    facts.meta_width = metadata_u32(metadata, "width");
    facts.meta_height = metadata_u32(metadata, "height");
    facts.original_size_bytes = metadata_f64(metadata, "originalSizeBytes");
}

fn metadata_u32(metadata: &[MetadataEntry], key: &str) -> u32 {
    let Some(value) = metadata.iter().find(|entry| entry.key == key).map(|entry| &entry.value) else {
        return 0;
    };
    let number = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    match number {
        Some(number) if number.is_finite() && number > 0.0 && number <= u32::MAX as f64 => number.round() as u32,
        _ => {
            eprintln!("Nana 元数据尺寸不是正数：{key}");
            0
        }
    }
}

fn metadata_f64(metadata: &[MetadataEntry], key: &str) -> Option<f64> {
    let value = metadata.iter().find(|entry| entry.key == key).map(|entry| &entry.value)?;
    let number = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    match number {
        Some(number) if number.is_finite() && number >= 0.0 => Some(number),
        _ => {
            eprintln!("Nana 原始大小不是非负数字：{key}");
            None
        }
    }
}

fn parse_paths(value: &str) -> Vec<String> {
    split_list(value)
        .into_iter()
        .map(|item| item.replace('\\', "/").trim_matches('/').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}
