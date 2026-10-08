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
use crate::host_api::{PlaybackMediaCapabilities, PlaybackMediaPlugin};
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
