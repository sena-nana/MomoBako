//! 预览扩展名分派、文本字节上限、WAV 预览会话，以及搜索条件解析。
//!
//! Markdown 先于普通文本。内置贡献负责 ZIP、Open XML 和部分模型；其余文档和模型仍要求升级。

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
const OFFICE_EXTENSIONS: &[&str] = &[
    "pdf", "docx", "docm", "doc", "dotx", "dotm", "dot", "xlsx", "xlsm", "xlsb", "xls", "xltx", "xltm", "xlt", "pptx",
    "pptm", "ppt", "ppsx", "ppsm", "pps", "potx", "potm", "pot",
];
const ARCHIVE_EXTENSIONS: &[&str] = &["zip", "cbz", "7z", "rar", "cbr"];
const MODEL_EXTENSIONS: &[&str] = &["fbx", "obj", "glb", "gltf", "vrm", "stl", "3mf", "blend"];

/// 扩展名分派。Markdown 先于普通文本。插件类型没有原生贡献时要求升级。
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
        return PreviewKind::Native {
            view_id: binding.contribution.view_id.clone(),
            label: binding.contribution.label.clone(),
        };
    }
    if OFFICE_EXTENSIONS.contains(&extension.as_str())
        || ARCHIVE_EXTENSIONS.contains(&extension.as_str())
        || MODEL_EXTENSIONS.contains(&extension.as_str())
    {
        return PreviewKind::Upgrade;
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

/// 音视频预览先排队读文件。WAV 成功后才允许播放控制。
pub(super) fn begin_media(state: &mut super::InspectState, repo_id: &str, path: &str) {
    state.loading = true;
    state.activity = "正在读取音频…".into();
    state.body = super::PreviewBody::Empty;
    state.error.clear();
    state.effects.push(super::InspectEffect::LoadMedia {
        repo_id: repo_id.to_string(),
        path: path.to_string(),
        generation: state.generation,
    });
}

/// 代次或路径过期时保留当前预览。WAV 写入 paused 会话，其它格式失败。
pub(super) fn note_media(
    state: &mut super::InspectState,
    path: String,
    generation: u64,
    result: Result<PlaybackSessionState, String>,
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
            state.body = super::PreviewBody::Media(session);
        }
        Err(error) => {
            eprintln!("Nana 音视频预览失败：{error}");
            let repo_id = state.repo_id.clone().unwrap_or_default();
            state.body = super::PreviewBody::Media(failed_session(&repo_id, error.clone()));
            state.error = error;
            state.activity = state.error.clone();
        }
    }
}

/// 只有 WAV 能进入可控制会话。其它字节仍是没有解码器。
pub(crate) fn preview_media_session(repo_id: &str, bytes: &[u8]) -> Result<PlaybackSessionState, String> {
    match crate::shell::player::wav_duration_ms(bytes) {
        Ok(duration) => Ok(paused_wav_session(repo_id, duration)),
        Err(error) => {
            eprintln!("Nana 音视频预览不能解码：{error}");
            Err("没有原生解码器".into())
        }
    }
}

/// 已装载的 WAV 允许播放、暂停、跳转和音量。失败会话仍拒绝控制。
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

fn paused_wav_session(repo_id: &str, duration_ms: u64) -> PlaybackSessionState {
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

fn parse_paths(value: &str) -> Vec<String> {
    split_list(value)
        .into_iter()
        .map(|item| item.replace('\\', "/").trim_matches('/').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}
