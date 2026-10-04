//! 预览扩展名分派、文本字节上限、无解码器的播放会话，以及搜索条件解析。
//!
//! Markdown 先于普通文本。PDF、Office、压缩包和三维模型没有原生预览贡献时不在这里解码。

use serde_json::Value;

use crate::backend::services::repository::{MetadataEntry, PlaybackSessionState};
use crate::host_api::{PlaybackMediaCapabilities, PlaybackMediaPlugin, PlaybackSessionController};
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

/// 生产环境没有原生解码器。`load` 失败后会话停在 failed，后续控制也不能变成播放中。
pub(super) struct MissingDecoder;

impl PlaybackMediaPlugin for MissingDecoder {
    fn load(&mut self, _source: &str) -> Result<PlaybackMediaCapabilities, String> {
        Err("没有原生解码器".into())
    }

    fn play(&mut self) -> Result<(), String> {
        Err("没有原生解码器".into())
    }

    fn pause(&mut self) -> Result<(), String> {
        Err("没有原生解码器".into())
    }

    fn seek(&mut self, _position_ms: u64) -> Result<(), String> {
        Err("没有原生解码器".into())
    }

    fn set_volume(&mut self, _volume: f32) -> Result<(), String> {
        Err("没有原生解码器".into())
    }

    fn dispose(&mut self) {}
}

/// 用缺失解码器装载路径。失败写入会话错误，不返回可播放状态。
pub(super) fn fail_media(repo_id: &str, path: &str) -> PlaybackSessionState {
    let mut controller = PlaybackSessionController::new(MissingDecoder, fresh_session(repo_id));
    if let Err(error) = controller.load(path) {
        eprintln!("Nana 音视频预览失败：{error}");
    }
    controller.state().clone()
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
    contributions.iter().find(|binding| {
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
