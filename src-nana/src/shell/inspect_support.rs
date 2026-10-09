//! 预览扩展名分派、文本字节上限和可解码音频的预览会话。搜索条件解析在 `search_request.rs`。
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
use crate::plugin_api::NativeContributionKind;

use super::{PreviewBinding, PreviewKind};

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

/// 文本预览读出的内容。只显示了开头一段时，`truncated_at` 记实际解码的字节数。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewText {
    pub text: String,
    pub truncated_at: Option<u64>,
}

/// 和 text-preview 一致：只取前 768 KiB，按 BOM 认 UTF-8 / UTF-16，其余按 UTF-8 宽松解码。
///
/// 截断处若切在多字节字符中间，丢掉这半个字符，不在末尾留替换符。
pub fn prepare_text(bytes: &[u8]) -> PreviewText {
    let truncated = bytes.len() > TEXT_BYTE_LIMIT;
    let shown = &bytes[..bytes.len().min(TEXT_BYTE_LIMIT)];
    PreviewText { text: decode_text(shown, truncated), truncated_at: truncated.then_some(shown.len() as u64) }
}

/// 按 BOM 选编码：EF BB BF 是 UTF-8，FF FE / FE FF 是 UTF-16 小端 / 大端，没有 BOM 按 UTF-8。
fn decode_text(bytes: &[u8], truncated: bool) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return utf8_text(rest, truncated);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return utf16_text(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return utf16_text(rest, u16::from_be_bytes);
    }
    utf8_text(bytes, truncated)
}

fn utf8_text(bytes: &[u8], truncated: bool) -> String {
    let bytes = if truncated { trim_partial_utf8(bytes) } else { bytes };
    String::from_utf8_lossy(bytes).into_owned()
}

/// UTF-16 两字节一个码元；截断留下的单个尾字节丢掉，坏的代理对换成替换符。
fn utf16_text(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units = bytes.chunks_exact(2).map(|pair| unit([pair[0], pair[1]]));
    char::decode_utf16(units).map(|item| item.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
}

/// 算法：从末尾往前最多看 3 个字节，找到最后一个字符的首字节，按首字节算出该字符应有的长度；
/// 剩下的字节不够这个长度，说明截断切在了字符中间，把这半个字符去掉。
fn trim_partial_utf8(bytes: &[u8]) -> &[u8] {
    let tail = bytes.len().saturating_sub(3);
    for start in (tail..bytes.len()).rev() {
        let lead = bytes[start];
        if lead & 0b1100_0000 == 0b1000_0000 {
            continue;
        }
        let width = match lead {
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => 1,
        };
        return if bytes.len() - start < width { &bytes[..start] } else { bytes };
    }
    bytes
}

/// 音视频预览先排队读文件。可解码音频成功后才允许播放控制。
pub(super) fn begin_media(state: &mut super::InspectState, repo_id: &str, path: &str) {
    state.loading = true;
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

/// 扩展名是不是视频容器（Vue `isVideoExtension`）。音视频预览失败时按它选标题。
pub(crate) fn is_video_extension(extension: &str) -> bool {
    VIDEO_EXTENSIONS.contains(&normalize_extension(extension).as_str())
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
