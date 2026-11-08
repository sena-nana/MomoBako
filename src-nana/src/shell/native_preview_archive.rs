//! zip、cbz 和 7z 的当前层，以及名单里单个文件的字节。
//!
//! 列表只留下已经读出的条目名，条数到 [`LIST_LIMIT`] 就停，不把条目内容读进内存。
//! 已有字节数按 Vue `formatByteCount` 显示；目录仍写「目录」。没有大小字段不编数字。
//! 单击目录只选中。连点进入下一层时按这些名字过滤，不重新打开压缩包。
//! 点开一个已经列出的文件时，只取出那一条的真实字节：zip 按中央目录定位这一条；
//! 7z 只在这一条位于自己压缩块开头时读取，固实块里靠后的文件要先解开前面的内容，这种不取。
//! 文本、图片、PDF、Office、模型和能在内存里解开的音频交给现有预览。Office 只抽 Open XML / OLE 文本。
//! 能光栅的模型用现有软件光栅，FBX 与 BLEND 只留文件头。音频只解出时长和声道，不装进播放条，也不打开声卡。
//! 取出的字节若是纯 Rust 能解的未压缩 AVI，按这条预览自己的播放头画已经解开的帧，PCM 丢掉，播放条不接管。其它视频格式停在名单上。
//! 取出的 zip、cbz 和 7z 只用名字再列这一层；到第 5 层后，里面的 zip、cbz 和 7z 行尾写「嵌套深度已达上限」，不再展开。
//! 取不出的条目停在当前名单上，旁边写「无法预览内部文件」和原因。不能内联的类型不取字节，旁边写文件名和
//! 「无法内联预览该文件类型」。点目录也收起已打开的文件，舞台写目录名和同一句。名单空着等选择时写当前目录
//! （根上是「根目录」）和「选择内部文件进行预览」。
//! 固实 7z 在取出阶段就停，不解画面。现有解码器解不开、又必须另找程序的格式不在这里解。
//! rar 和 cbr 仍按整条路径列出，不能进入子目录，也不打开包内文件。

use std::io::{Cursor, Read};

use unarc_rs::unified::{ArchiveFormat, UnifiedArchive};
use zip::ZipArchive;

use super::super::bridge::{
    ArchiveCatalog, ArchiveEntryKind, ArchiveEntryPreview, ArchiveName, ArchiveNest, NativeLoad,
};
use super::super::{ArchiveFileNotice, InspectEffect, InspectState, PreviewBody, PreviewKind};
use super::ARCHIVE_VIEW;

#[path = "archive_entry_size.rs"]
mod entry_size;
#[path = "archive_nested_depth.rs"]
mod nested_depth;

const LIST_LIMIT: usize = 200;
/// 单条解压后的上限。超过就不读，避免一条大文件把整包内容拖进内存。
const ENTRY_CAP: u64 = 32 * 1024 * 1024;

/// 先按 zip 列当前层。cbz 也是这种 zip。zip 打不开时再试 7z，然后是 rar。rar 不能进入下一层。
pub(super) fn list(bytes: &[u8]) -> Result<NativeLoad, String> {
    match ZipArchive::new(Cursor::new(bytes)) {
        Ok(archive) => list_zip(archive),
        Err(error) => {
            eprintln!("Nana 压缩包按 zip 打开失败：{error}");
            list_unarc(bytes)
        }
    }
}

/// 选中当前层已经列出的目录。不改当前层，不重新打开压缩包，也不编造条目。
/// 已经打开的内部文件收起。舞台写这个目录的名字和「无法内联预览该文件类型」。
pub(super) fn select(state: &mut InspectState, path: &str) {
    let Some(path) = normalize_directory(path) else {
        eprintln!("Nana 压缩包目录无效：{path}");
        return;
    };
    if path.is_empty() {
        eprintln!("Nana 压缩包不能选中空路径");
        return;
    }
    let Some(catalog) = current_catalog(state) else {
        eprintln!("Nana 压缩包没有已解出的条目，不能选中目录");
        return;
    };
    if !catalog.layered {
        eprintln!("Nana 压缩包不选中目录");
        return;
    }
    if !listed_child_directory(state, &path) {
        eprintln!("Nana 压缩包名单里没有这个目录：{path}");
        return;
    }
    // Vue `selectEntry`：清掉已打开的文件和在途读取。目录没有扩展名，
    // `renderInternalPreview` 落到二进制，舞台写目录名和「无法内联预览该文件类型」。
    state.archive_selected = Some(path.clone());
    state.archive_request = None;
    state.archive_entry = None;
    state.archive_notice = Some(ArchiveFileNotice::Inline(
        entry_file_name(&path).to_string(),
    ));
}

/// 进入已经解出的目录。`path` 为空回到最外层根。名字不在已读列表里、或这份是 rar，就保持当前层。
/// 嵌套层的路径从外层算起；回到外层时收起里面的名单。
pub(super) fn enter(state: &mut InspectState, path: &str) {
    if !state.archive_nests.is_empty() && enter_nested(state, path) {
        return;
    }
    enter_outer(state, path);
}

fn enter_outer(state: &mut InspectState, path: &str) {
    let Some(catalog) = state.archive.clone() else {
        eprintln!("Nana 压缩包没有已解出的条目，不能进入目录");
        return;
    };
    if !catalog.layered {
        eprintln!("Nana 压缩包不进入下一层");
        return;
    }
    let Some(path) = normalize_directory(path) else {
        eprintln!("Nana 压缩包目录无效：{path}");
        return;
    };
    if !directory_known(&catalog, &path) {
        eprintln!("Nana 压缩包里没有这个目录：{path}");
        return;
    }
    let text = layer_text(&catalog, &path);
    let updated = match &mut state.body {
        PreviewBody::Native {
            view_id, content, ..
        } if view_id == ARCHIVE_VIEW => {
            *content = text;
            true
        }
        PreviewBody::Native { view_id, .. } => {
            eprintln!("Nana 当前预览不是压缩包，不能进入目录：{view_id}");
            false
        }
        _ => {
            eprintln!("Nana 当前预览不是压缩包，不能进入目录");
            false
        }
    };
    if updated {
        state.archive_directory = path;
        state.archive_selected = None;
        state.archive_nests.clear();
        // 回到某一层时收起已经打开的包内文件，名单还是原来那份。
        state.archive_entry = None;
        state.archive_request = None;
        state.archive_notice = None;
    }
}

/// 在嵌套名单里进目录，或沿着面包屑回到外层。路径对不上就停住，不改当前层。
fn enter_nested(state: &mut InspectState, path: &str) -> bool {
    let Some(path) = normalize_directory(path) else {
        eprintln!("Nana 压缩包目录无效：{path}");
        return true;
    };
    match resolve_nested(state, &path) {
        Some(NestTarget::Outer(directory)) => enter_outer(state, &directory),
        Some(NestTarget::Layer { keep, directory }) => apply_nest(state, keep, directory),
        None => eprintln!("Nana 压缩包里没有这个目录：{path}"),
    }
    true
}

enum NestTarget {
    Outer(String),
    Layer { keep: usize, directory: String },
}

/// 面包屑路径落在哪一层。`keep` 是留下的嵌套层数，目录相对这一层的根。
fn resolve_nested(state: &InspectState, path: &str) -> Option<NestTarget> {
    if path.is_empty() {
        return Some(NestTarget::Outer(String::new()));
    }
    let mut chain = String::new();
    for (index, nest) in state.archive_nests.iter().enumerate() {
        if !chain.is_empty() {
            chain.push('/');
        }
        chain.push_str(&nest.entry);
        if path == chain {
            return Some(NestTarget::Layer {
                keep: index + 1,
                directory: String::new(),
            });
        }
        let prefix = format!("{chain}/");
        let Some(rest) = path.strip_prefix(&prefix) else {
            break;
        };
        let deeper = index + 1 < state.archive_nests.len();
        if deeper {
            let next = &state.archive_nests[index + 1].entry;
            if rest == next || rest.starts_with(&format!("{next}/")) {
                continue;
            }
        }
        if directory_known(&nest.catalog, rest) {
            return Some(NestTarget::Layer {
                keep: index + 1,
                directory: rest.to_string(),
            });
        }
        return None;
    }
    let catalog = state.archive.as_ref()?;
    if catalog.layered && directory_known(catalog, path) {
        Some(NestTarget::Outer(path.to_string()))
    } else {
        None
    }
}

fn apply_nest(state: &mut InspectState, keep: usize, directory: String) {
    state.archive_nests.truncate(keep);
    let full = nest_location(&state.archive_nests, &directory);
    let text = {
        let catalog = &state.archive_nests.last().expect("嵌套层还在").catalog;
        nested_depth::limit_layer_text(nest_layer_text(catalog, &directory, &full), keep)
    };
    let nest = state.archive_nests.last_mut().expect("嵌套层还在");
    nest.directory = directory;
    nest.text = text;
    state.archive_selected = None;
    state.archive_entry = None;
    state.archive_request = None;
    state.archive_notice = None;
}

/// 点开名单里的一个文件。名字不在当前层、类型不能预览、或这是 rar，就停在名单上。
/// 字节在副作用里再读，这里不打开压缩包。
pub(super) fn request_file(state: &mut InspectState, path: &str) {
    if current_catalog(state).is_none() {
        eprintln!("Nana 压缩包没有已解出的条目，不能打开文件");
        return;
    }
    if !current_catalog(state).is_some_and(|catalog| catalog.layered) {
        eprintln!("Nana 压缩包不打开包内文件");
        return;
    }
    let Some(path) = normalize_file(path) else {
        eprintln!("Nana 压缩包文件路径无效");
        return;
    };
    let Some(relative) = relative_listed(state, &path) else {
        eprintln!("Nana 压缩包名单里没有这个文件：{path}");
        return;
    };
    if nested_depth::refuse_past_depth(state, &path) {
        return;
    }
    if !previewable(&relative) {
        // Vue `selectEntry` 先把这一条标成当前项，再写文件名和「无法内联预览该文件类型」，不去取字节。
        state.archive_selected = Some(path);
        state.archive_request = None;
        state.archive_notice = Some(ArchiveFileNotice::Inline(
            entry_file_name(&relative).to_string(),
        ));
        eprintln!("Nana 压缩包里的这个类型不能预览：{relative}");
        return;
    }
    let (Some(repo_id), Some(archive_path)) = (state.repo_id.clone(), state.target_path.clone())
    else {
        state.archive_selected = Some(path);
        state.archive_request = None;
        state.archive_notice = Some(ArchiveFileNotice::Failed("缺少资源库或压缩包".into()));
        eprintln!("Nana 打开包内文件缺少资源库或压缩包");
        return;
    };
    let through = state
        .archive_nests
        .iter()
        .map(|nest| nest.entry.clone())
        .collect::<Vec<_>>();
    // 点开的文件行是当前项。目录那一行因此不再亮。进入下一层时 `push_nest` 再清掉。
    state.archive_selected = Some(path);
    state.archive_notice = None;
    state.archive_request = Some(request_key(&through, &relative));
    state.effects.push(InspectEffect::LoadArchiveEntry {
        repo_id,
        path: archive_path,
        entry: relative,
        through,
        generation: state.generation,
    });
}

/// 包内文件的字节到了。代次、压缩包路径或请求的条目对不上就丢掉。取不出时回到当前名单，不编一段正文。
pub(super) fn note_entry(
    state: &mut InspectState,
    path: String,
    entry: String,
    through: Vec<String>,
    generation: u64,
    result: Result<ArchiveEntryPreview, String>,
) {
    let current = generation == state.generation
        && state.target_path.as_deref() == Some(path.as_str())
        && state.archive_request.as_deref() == Some(request_key(&through, &entry).as_str());
    if !current {
        eprintln!("Nana 忽略过期的包内文件：{entry}");
        return;
    }
    state.archive_request = None;
    match result {
        Ok(preview) => accept_entry(state, preview),
        Err(error) => {
            eprintln!("Nana 包内文件不能预览：{entry}：{error}");
            state.archive_entry = None;
            state.archive_notice = Some(ArchiveFileNotice::Failed(error));
        }
    }
}

fn accept_entry(state: &mut InspectState, preview: ArchiveEntryPreview) {
    state.archive_notice = None;
    if let ArchiveEntryKind::Nested { catalog } = preview.kind {
        // 只留下这一层的名字。容器字节在列出时已经丢掉。
        push_nest(state, preview.path, catalog);
        return;
    }
    let mut preview = preview;
    if !state.archive_nests.is_empty() {
        preview.path = format!("{}/{}", nest_chain(&state.archive_nests), preview.path);
    }
    state.archive_entry = Some(preview);
    state.error.clear();
}

/// 只取出已经列出的那一条。文本、图片、PDF、Office、模型、能在内存里解开的音频、未压缩 AVI 和嵌套 zip、cbz、7z 都用这一条的真实字节。
pub(super) fn preview_entry(bytes: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    preview_chain(bytes, &[], path)
}

/// `through` 里每一段都是上一层里的 zip、cbz 或 7z，只解开这一段，再从里面取下一段。
pub(super) fn preview_chain(
    bytes: &[u8],
    through: &[String],
    path: &str,
) -> Result<ArchiveEntryPreview, String> {
    if !previewable(path) {
        eprintln!("Nana 压缩包里的这个类型不能预览：{path}");
        return Err(format!("不能预览 {path}"));
    }
    let raw = extract_chain(bytes, through, path)?;
    present(&raw, path)
}

/// 一层只解开当前这一条。嵌套容器解开后用来取下一条或列名字，不把里面每个文件都留在内存里。
fn extract_chain(bytes: &[u8], through: &[String], path: &str) -> Result<Vec<u8>, String> {
    if through.is_empty() {
        return extract_listed(bytes, path);
    }
    let mut current = extract_listed(bytes, &through[0])?;
    for name in &through[1..] {
        current = extract_listed(&current, name)?;
    }
    extract_listed(&current, path)
}

fn present(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    let extension = extension_of(path);
    match super::super::support::classify(&extension) {
        PreviewKind::Image => present_image(raw, path),
        PreviewKind::Markdown => present_text(raw, path, &extension, true),
        PreviewKind::Text => present_text(raw, path, &extension, false),
        PreviewKind::Native { view_id, .. } if view_id == super::PDF_VIEW => present_pdf(raw, path),
        PreviewKind::Native { view_id, .. } if view_id == super::OFFICE_VIEW => {
            present_office(raw, path)
        }
        PreviewKind::Native { view_id, .. } if view_id == super::MODEL_VIEW => {
            present_model(raw, path)
        }
        PreviewKind::Native { view_id, .. }
            if view_id == super::ARCHIVE_VIEW && nestable(&extension) =>
        {
            present_nest(raw, path)
        }
        PreviewKind::Media if audible(&extension) => present_audio(raw, path),
        PreviewKind::Media if avi_name(&extension) => present_avi(raw, path),
        _ => {
            eprintln!("Nana 压缩包里的这个类型不能预览：{path}");
            Err(format!("不能预览 {path}"))
        }
    }
}

fn present_image(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    let pixels = crate::decode_preview_pixels(raw).map_err(|error| {
        eprintln!("Nana 包内图片解码失败：{path}：{error}");
        error
    })?;
    Ok(ArchiveEntryPreview {
        path: path.to_string(),
        kind: ArchiveEntryKind::Image {
            width: pixels.width,
            height: pixels.height,
            rgba: pixels.rgba,
        },
    })
}

fn present_text(
    raw: &[u8],
    path: &str,
    extension: &str,
    markdown: bool,
) -> Result<ArchiveEntryPreview, String> {
    let read = super::super::prepare_text(raw);
    Ok(ArchiveEntryPreview {
        path: path.to_string(),
        kind: ArchiveEntryKind::Text {
            markdown,
            extension: extension.to_string(),
            text: read.text,
            truncated_at: read.truncated_at,
        },
    })
}

/// 用现有 Open XML / OLE 抽取。失败时留下原因，预览框写「无法预览该文档」，不还原版式。
fn present_office(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    match super::read_office(raw) {
        Ok(text) => Ok(ArchiveEntryPreview {
            path: path.to_string(),
            kind: ArchiveEntryKind::Office { text, error: None },
        }),
        Err(error) => {
            eprintln!("Nana 包内文档抽取失败：{path}：{error}");
            Ok(ArchiveEntryPreview {
                path: path.to_string(),
                kind: ArchiveEntryKind::Office {
                    text: String::new(),
                    error: Some(error),
                },
            })
        }
    }
}

/// 用现有软件光栅。FBX 和 BLEND 没有网格，只留下文件头摘要。解析失败用现有模型失败状态。
fn present_model(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    match super::mesh::load(raw) {
        Ok(loaded) => {
            let frame = loaded
                .frames
                .into_iter()
                .next()
                .filter(|frame| frame.error.is_none() && !frame.rgba.is_empty());
            let extent = loaded
                .mesh
                .as_ref()
                .map(super::mesh::extent_label)
                .unwrap_or_default();
            Ok(ArchiveEntryPreview {
                path: path.to_string(),
                kind: ArchiveEntryKind::Model {
                    text: loaded.text,
                    frame,
                    mesh: loaded.mesh,
                    yaw: 0.0,
                    zoom: 1.0,
                    error: None,
                    extent,
                },
            })
        }
        Err(error) => {
            eprintln!("Nana 包内模型解析失败：{path}：{error}");
            Ok(ArchiveEntryPreview {
                path: path.to_string(),
                kind: ArchiveEntryKind::Model {
                    text: String::new(),
                    frame: None,
                    mesh: None,
                    yaw: 0.0,
                    zoom: 1.0,
                    error: Some(error),
                    extent: String::new(),
                },
            })
        }
    }
}

/// 用现有音频解码在内存里解这一条。WAV 走自有解析，mp3、flac、ogg 走 symphonia，
/// m4a、aac、opus 在前面失败后走现有媒体基础拉样本。解出的 PCM 只用来确认有样本，随即丢掉，
/// 不装进播放条，也不调用播放。附带的画面不在这里画。解不开时留下现有失败原因。
fn present_audio(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    let extension = extension_of(path);
    match crate::shell::player::decode_media("archive", &extension, raw) {
        Ok(parts) => match audio_facts(&parts) {
            Some((duration_ms, sample_rate, channels)) => Ok(ArchiveEntryPreview {
                path: path.to_string(),
                kind: ArchiveEntryKind::Audio {
                    duration_ms,
                    sample_rate,
                    channels,
                    error: None,
                },
            }),
            None => {
                eprintln!("Nana 包内音频没有解出样本：{path}");
                Ok(failed_audio(path, "音频无法播放"))
            }
        },
        Err(error) => {
            eprintln!("Nana 包内音频解码失败：{path}：{error}");
            Ok(failed_audio(path, &error))
        }
    }
}

/// 有非空 PCM、采样率和声道，才算无声解开。封面之类的画面不参与判断。
fn audio_facts(parts: &crate::shell::MediaParts) -> Option<(u64, u32, u16)> {
    let pcm = parts.pcm.as_ref()?;
    if pcm.pcm.is_empty() || pcm.sample_rate == 0 || pcm.channels == 0 {
        return None;
    }
    Some((pcm.duration_ms, pcm.sample_rate, pcm.channels))
}

/// 用现有纯 Rust 路径解未压缩 AVI。解出画面就留下，播放头从片头开始；PCM 由解码函数丢掉，这里不装进播放条。
/// 不是这一类、或解不开，返回错误，调用方停在名单上。不调用系统媒体基础。
fn present_avi(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    match super::super::support::uncompressed_avi_frames(raw) {
        Ok(Some(frames)) => Ok(ArchiveEntryPreview {
            path: path.to_string(),
            kind: ArchiveEntryKind::Video {
                frames,
                position_ms: 0,
            },
        }),
        Ok(None) => {
            eprintln!("Nana 包内视频不是能解开的未压缩 AVI，停在名单上：{path}");
            Err(format!("不能预览 {path}"))
        }
        Err(error) => {
            eprintln!("Nana 包内 AVI 解码失败，停在名单上：{path}：{error}");
            Err(error)
        }
    }
}

/// 把包内 AVI 这条预览自己的播放头拨到 `time_ms`。播放条保持原样，不改成播放会话。
///
/// 已经挂着宿主纹理记号（`video:`）时，记号和像素跟着换到不晚于这个时刻的已解帧，真窗口重新上传。
/// 没有记号时只改播放头：不把未注册的记号挂回去，离屏继续画解码像素。没有这条视频时只记日志。
pub(super) fn place_video(model: &mut super::super::super::ShellViewModel, time_ms: u64) {
    let host = model
        .preview_token
        .as_deref()
        .is_some_and(|token| token.starts_with("video:"));
    let uploaded = {
        let Some(entry) = model.inspect.archive_entry.as_mut() else {
            eprintln!("Nana 包内没有正在看的文件，播放头留在原地");
            return;
        };
        let path = entry.path.clone();
        let ArchiveEntryKind::Video {
            frames,
            position_ms,
        } = &mut entry.kind
        else {
            eprintln!("Nana 包内这一条不是视频，播放头留在原地：{path}");
            return;
        };
        *position_ms = time_ms;
        if !host {
            return;
        }
        let Some(frame) = super::super::bridge::frame_at(frames, time_ms)
            .filter(|frame| frame.width > 0 && frame.height > 0 && !frame.rgba.is_empty())
        else {
            eprintln!("Nana 包内 AVI 在 {time_ms} 毫秒没有可画的画面：{path}");
            return;
        };
        Some((
            path,
            frame.time_ms,
            frame.width,
            frame.height,
            frame.rgba.clone(),
        ))
    };
    let Some((path, frame_time, width, height, rgba)) = uploaded else {
        return;
    };
    model.preview_pixels = Some(super::super::super::PreviewPixels {
        width,
        height,
        rgba,
    });
    model.preview_token = Some(format!("video:{path}:{frame_time}:{width}"));
}

fn failed_audio(path: &str, error: &str) -> ArchiveEntryPreview {
    ArchiveEntryPreview {
        path: path.to_string(),
        kind: ArchiveEntryKind::Audio {
            duration_ms: 0,
            sample_rate: 0,
            channels: 0,
            error: Some(error.to_string()),
        },
    }
}

/// 用现有自绘 PDF 画这一条的页。解析失败留下失败原因，不另找一份 PDF。
fn present_pdf(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    match super::pdf::load(raw) {
        Ok(loaded) => Ok(ArchiveEntryPreview {
            path: path.to_string(),
            kind: ArchiveEntryKind::Pdf {
                frames: loaded.frames,
                index: 0,
                error: None,
            },
        }),
        Err(error) => {
            eprintln!("Nana 包内 PDF 解析失败：{path}：{error}");
            Ok(ArchiveEntryPreview {
                path: path.to_string(),
                kind: ArchiveEntryKind::Pdf {
                    frames: Vec::new(),
                    index: 0,
                    error: Some(error),
                },
            })
        }
    }
}

/// 用现有名单列出这一层的真实名字。列完就丢掉容器字节。打不开或不能按层进入时停在上一层。
fn present_nest(raw: &[u8], path: &str) -> Result<ArchiveEntryPreview, String> {
    let loaded = match list(raw) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("Nana 嵌套压缩包列不出来：{path}：{error}");
            return Err(error);
        }
    };
    let Some(catalog) = loaded.archive else {
        eprintln!("Nana 嵌套压缩包没有名单：{path}");
        return Err(format!("{path} 没有名单"));
    };
    if !catalog.layered {
        eprintln!("Nana 嵌套压缩包不能按层打开：{path}");
        return Err(format!("{path} 不能按层打开"));
    }
    Ok(ArchiveEntryPreview {
        path: path.to_string(),
        kind: ArchiveEntryKind::Nested { catalog },
    })
}

/// 包内 PDF 的翻页。不是这种预览时返回假，让原来的页甲板处理。
pub(super) fn turn_pdf(state: &mut InspectState, delta: i32) -> bool {
    let Some(entry) = state.archive_entry.as_mut() else {
        return false;
    };
    let ArchiveEntryKind::Pdf {
        frames,
        index,
        error,
    } = &mut entry.kind
    else {
        return false;
    };
    if error.is_some() || frames.is_empty() {
        eprintln!("Nana 包内 PDF 没有可翻的页");
        return true;
    }
    let len = frames.len() as i32;
    let next = *index as i32 + delta;
    if next < 0 || next >= len {
        eprintln!("Nana PDF 页码越界：{next}");
        return true;
    }
    *index = next as usize;
    true
}

/// 包内模型绕竖直轴旋转，或按比例缩放。没有网格时不假装已经转过。不是模型时返回假。
pub(super) fn orbit_model(state: &mut InspectState, yaw: f32, zoom: f32) -> bool {
    let Some(entry) = state.archive_entry.as_mut() else {
        return false;
    };
    let data = {
        let ArchiveEntryKind::Model { mesh, error, .. } = &entry.kind else {
            return false;
        };
        if error.is_some() {
            eprintln!("Nana 包内模型不能旋转");
            return true;
        }
        let Some(data) = mesh.clone() else {
            eprintln!("Nana 包内模型没有可旋转的网格");
            return true;
        };
        data
    };
    let ArchiveEntryKind::Model {
        frame,
        yaw: slot_yaw,
        zoom: slot_zoom,
        ..
    } = &mut entry.kind
    else {
        return false;
    };
    *slot_yaw += yaw;
    if zoom > 0.0 {
        *slot_zoom = (*slot_zoom * zoom).clamp(0.25, 8.0);
    }
    *frame = Some(super::paint_mesh(&data, *slot_yaw, *slot_zoom));
    true
}

fn previewable(path: &str) -> bool {
    let extension = extension_of(path);
    match super::super::support::classify(&extension) {
        PreviewKind::Text | PreviewKind::Markdown | PreviewKind::Image => true,
        PreviewKind::Media if audible(&extension) || avi_name(&extension) => true,
        PreviewKind::Native { view_id, .. }
            if view_id == super::PDF_VIEW
                || view_id == super::OFFICE_VIEW
                || view_id == super::MODEL_VIEW =>
        {
            true
        }
        PreviewKind::Native { view_id, .. }
            if view_id == super::ARCHIVE_VIEW && nestable(&extension) =>
        {
            true
        }
        _ => false,
    }
}

/// 现有解码器不靠 ffmpeg、也不靠打开声卡就能试的音频。视频不在这里。
fn audible(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "wav" | "mp3" | "flac" | "ogg" | "m4a" | "aac" | "opus"
    )
}

/// 只按扩展名放行去取字节。取出来不是未压缩 AVI 就回到名单，不走系统解码。
fn avi_name(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("avi")
}

/// zip、cbz 和 7z 再列一层。Vue `archive_format_for_path` 把 cbz 当成 Zip，这里只列已经取出的字节。cbr 和 rar 仍停在名单上。
fn nestable(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "zip" | "cbz" | "7z"
    )
}

/// 名单上显示的名字：路径的最后一段。没有斜杠时就是整段。
fn entry_file_name(path: &str) -> &str {
    path.rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
}

fn extension_of(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_string())
        .unwrap_or_default()
}

/// zip 打得开就只读这一条。打不开再试 7z。不试 rar，也不读取其它条目的内容。
fn extract_listed(bytes: &[u8], path: &str) -> Result<Vec<u8>, String> {
    match ZipArchive::new(Cursor::new(bytes)) {
        Ok(archive) => extract_zip(archive, path),
        Err(error) => {
            eprintln!("Nana 压缩包按 zip 取出失败：{error}");
            extract_sevenz(bytes, path)
        }
    }
}

fn extract_zip(mut archive: ZipArchive<Cursor<&[u8]>>, path: &str) -> Result<Vec<u8>, String> {
    let mut found = None;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|error| {
            eprintln!("Nana 压缩包条目读取失败：{error}");
            format!("无法读取压缩包条目：{error}")
        })?;
        let Some((name, directory)) = normalize_entry_name(file.name(), file.is_dir()) else {
            continue;
        };
        if directory || name != path {
            continue;
        }
        if file.size() > ENTRY_CAP {
            eprintln!(
                "Nana 压缩包这一条有 {} 字节，超过 {ENTRY_CAP}，不读进内存：{path}",
                file.size()
            );
            return Err(format!("{path} 太大，不能预览"));
        }
        found = Some(index);
        break;
    }
    let Some(index) = found else {
        eprintln!("Nana 压缩包里没有这一条：{path}");
        return Err(format!("压缩包里没有 {path}"));
    };
    let mut file = archive.by_index(index).map_err(|error| {
        eprintln!("Nana 压缩包条目读取失败：{error}");
        format!("无法读取压缩包条目：{error}")
    })?;
    read_capped(&mut file, path)
}

fn extract_sevenz(bytes: &[u8], path: &str) -> Result<Vec<u8>, String> {
    let mut archive = UnifiedArchive::open_with_format(Cursor::new(bytes), ArchiveFormat::SevenZ)
        .map_err(|error| {
        eprintln!("Nana 压缩包按 7z 打开失败：{error}");
        format!("无法读取压缩包：{error}")
    })?;
    let mut matched = None;
    for item in archive.entries_iter() {
        let entry = item.map_err(|error| {
            eprintln!("Nana 压缩包条目读取失败：{error}");
            format!("无法读取压缩包条目：{error}")
        })?;
        let Some((name, directory)) = normalize_entry_name(entry.name(), entry.is_directory())
        else {
            continue;
        };
        if directory || name != path {
            continue;
        }
        if entry.original_size() > ENTRY_CAP {
            eprintln!(
                "Nana 压缩包这一条有 {} 字节，超过 {ENTRY_CAP}，不读进内存：{path}",
                entry.original_size()
            );
            return Err(format!("{path} 太大，不能预览"));
        }
        // 固实块里只有块内第一个文件带压缩大小。后面的文件要先把前面的都解开，这里不取。
        if entry.original_size() > 0 && entry.compressed_size() == 0 {
            eprintln!("Nana 7z 这一条不在压缩块开头，不能单独取出：{path}");
            return Err(format!("{path} 不能单独取出"));
        }
        matched = Some(entry);
        break;
    }
    let Some(entry) = matched else {
        eprintln!("Nana 压缩包里没有这一条：{path}");
        return Err(format!("压缩包里没有 {path}"));
    };
    let data = archive.read(&entry).map_err(|error| {
        eprintln!("Nana 压缩包取出失败：{path}：{error}");
        format!("无法取出 {path}：{error}")
    })?;
    if data.len() as u64 > ENTRY_CAP {
        eprintln!(
            "Nana 压缩包这一条解开后有 {} 字节，超过 {ENTRY_CAP}，丢掉：{path}",
            data.len()
        );
        return Err(format!("{path} 太大，不能预览"));
    }
    Ok(data)
}

fn read_capped(file: &mut impl Read, path: &str) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    file.take(ENTRY_CAP)
        .read_to_end(&mut buf)
        .map_err(|error| {
            eprintln!("Nana 压缩包取出失败：{path}：{error}");
            format!("无法取出 {path}：{error}")
        })?;
    Ok(buf)
}

fn current_catalog(state: &InspectState) -> Option<&ArchiveCatalog> {
    state
        .archive_nests
        .last()
        .map(|nest| &nest.catalog)
        .or(state.archive.as_ref())
}

/// 点开的路径在当前层名单里时，返回相对这一层根的条目名。
fn relative_listed(state: &InspectState, path: &str) -> Option<String> {
    let relative = if state.archive_nests.is_empty() {
        path.to_string()
    } else {
        path.strip_prefix(&format!("{}/", nest_chain(&state.archive_nests)))?
            .to_string()
    };
    if relative.is_empty() {
        return None;
    }
    let catalog = current_catalog(state)?;
    catalog
        .names
        .iter()
        .any(|name| !name.directory && name.path == relative)
        .then_some(relative)
}

fn request_key(through: &[String], entry: &str) -> String {
    let mut key = through.join("\u{1}");
    key.push('\u{1}');
    key.push_str(entry);
    key
}

fn push_nest(state: &mut InspectState, entry: String, catalog: ArchiveCatalog) {
    state.archive_selected = None;
    state.archive_nests.push(ArchiveNest {
        entry,
        catalog,
        directory: String::new(),
        text: String::new(),
    });
    let full = nest_chain(&state.archive_nests);
    let depth = state.archive_nests.len();
    let text = nested_depth::limit_layer_text(
        nest_layer_text(
            &state.archive_nests.last().expect("刚压入").catalog,
            "",
            &full,
        ),
        depth,
    );
    state.archive_nests.last_mut().expect("刚压入").text = text;
    state.archive_entry = None;
    state.error.clear();
}

fn nest_chain(nests: &[ArchiveNest]) -> String {
    nests
        .iter()
        .map(|nest| nest.entry.as_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn nest_location(nests: &[ArchiveNest], directory: &str) -> String {
    let chain = nest_chain(nests);
    if directory.is_empty() {
        chain
    } else {
        format!("{chain}/{directory}")
    }
}

/// 名单正文仍按这一层的相对目录过滤，头部改成从外层算起的路径。
fn nest_layer_text(catalog: &ArchiveCatalog, directory: &str, full: &str) -> String {
    let relative = layer_text(catalog, directory);
    if relative == "压缩包里没有文件。" {
        return format!("压缩包 / {full} · 0 项");
    }
    let mut lines = relative.lines();
    let Some(head) = lines.next() else {
        return relative;
    };
    let Some((_, tail)) = head.rsplit_once(" · ") else {
        return relative;
    };
    if !tail.ends_with('项') {
        return relative;
    }
    let mut text = format!("压缩包 / {full} · {tail}");
    for line in lines {
        text.push('\n');
        text.push_str(line);
    }
    text
}

/// 和目录一样去掉首尾斜杠，拒绝 `.`、`..` 和盘符。文件路径不能是空的。
fn normalize_file(path: &str) -> Option<String> {
    let normalized = normalize_directory(path)?;
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn list_zip(mut archive: ZipArchive<Cursor<&[u8]>>) -> Result<NativeLoad, String> {
    let mut names = Vec::new();
    let mut index = 0;
    let len = archive.len();
    while index < len && names.len() < LIST_LIMIT {
        let file = archive.by_index(index).map_err(|error| {
            eprintln!("Nana 压缩包条目读取失败：{error}");
            format!("无法读取压缩包条目：{error}")
        })?;
        index += 1;
        let Some((path, directory)) = normalize_entry_name(file.name(), file.is_dir()) else {
            continue;
        };
        names.push(ArchiveName {
            path,
            size: file.size(),
            directory,
        });
    }
    // 到上限就停。多出来的条目不读进列表，避免为了对齐整包目录把剩余名字都留下。
    let truncated = names.len() == LIST_LIMIT && index < len;
    if truncated {
        eprintln!("Nana 压缩包列表在 {LIST_LIMIT} 条处截断");
    }
    finish(names, truncated, true)
}

fn list_unarc(bytes: &[u8]) -> Result<NativeLoad, String> {
    match list_unarc_format(bytes, ArchiveFormat::SevenZ, true) {
        Ok(load) => Ok(load),
        Err(error) => {
            eprintln!("Nana 压缩包按 7z 打开失败：{error}");
            match list_unarc_format(bytes, ArchiveFormat::Rar, false) {
                Ok(load) => Ok(load),
                Err(rar) => {
                    eprintln!("Nana 压缩包按 rar 打开失败：{rar}");
                    Err(rar)
                }
            }
        }
    }
}

fn list_unarc_format(
    bytes: &[u8],
    format: ArchiveFormat,
    layered: bool,
) -> Result<NativeLoad, String> {
    let mut archive = UnifiedArchive::open_with_format(Cursor::new(bytes), format)
        .map_err(|error| format!("无法读取压缩包：{error}"))?;
    let mut names = Vec::new();
    let mut truncated = false;
    for item in archive.entries_iter() {
        if names.len() == LIST_LIMIT {
            truncated = true;
            eprintln!("Nana 压缩包列表在 {LIST_LIMIT} 条处截断");
            break;
        }
        let entry = item.map_err(|error| {
            eprintln!("Nana 压缩包条目读取失败：{error}");
            format!("无法读取压缩包条目：{error}")
        })?;
        let Some((path, directory)) = normalize_entry_name(entry.name(), entry.is_directory())
        else {
            continue;
        };
        names.push(ArchiveName {
            path,
            size: entry.original_size(),
            directory,
        });
    }
    finish(names, truncated, layered)
}

fn finish(
    mut names: Vec<ArchiveName>,
    truncated: bool,
    layered: bool,
) -> Result<NativeLoad, String> {
    // 只在已经留下的名字里认目录。7z 的目录常常不带斜杠，靠后面的 `name/` 前缀认出来。
    mark_prefixes(&mut names);
    let catalog = ArchiveCatalog {
        names,
        layered,
        truncated,
    };
    let text = layer_text(&catalog, "");
    Ok(NativeLoad {
        text,
        frames: Vec::new(),
        mesh: None,
        paged: false,
        archive: Some(catalog),
    })
}

fn normalize_entry_name(raw: &str, directory: bool) -> Option<(String, bool)> {
    let name = raw.replace('\\', "/");
    let directory = directory || name.ends_with('/');
    let path = name.trim_matches('/').to_string();
    if path.is_empty() {
        None
    } else {
        Some((path, directory))
    }
}

fn mark_prefixes(names: &mut [ArchiveName]) {
    let paths: Vec<String> = names.iter().map(|name| name.path.clone()).collect();
    for name in names {
        let prefix = format!("{}/", name.path);
        if paths.iter().any(|other| other.starts_with(&prefix)) {
            name.directory = true;
        }
    }
}

/// 当前目录的直接子项。名字只取路径的下一段，目录排在文件前面。
fn layer_text(catalog: &ArchiveCatalog, directory: &str) -> String {
    if !catalog.layered {
        return flat_text(catalog);
    }
    let rows = current_rows(catalog, directory);
    if rows.is_empty() && directory.is_empty() && !catalog.truncated {
        return "压缩包里没有文件。".into();
    }
    let mut lines = Vec::new();
    if directory.is_empty() {
        lines.push(format!("压缩包 · {} 项", rows.len()));
    } else {
        lines.push(format!("压缩包 / {directory} · {} 项", rows.len()));
    }
    for row in rows {
        match row {
            LayerRow::Dir(name) => lines.push(format!("{name} · 目录")),
            LayerRow::File(name, size) => lines.push(format!(
                "{name} · {}",
                entry_size::format_archive_entry_size(size)
            )),
        }
    }
    if catalog.truncated {
        lines.push("…".into());
    }
    lines.join("\n")
}

fn flat_text(catalog: &ArchiveCatalog) -> String {
    let mut lines = Vec::new();
    for name in &catalog.names {
        if name.directory {
            continue;
        }
        lines.push(format!(
            "{} · {}",
            name.path,
            entry_size::format_archive_entry_size(name.size)
        ));
    }
    if lines.is_empty() && !catalog.truncated {
        return "压缩包里没有文件。".into();
    }
    let count = lines.len();
    if catalog.truncated {
        lines.push("…".into());
    }
    format!("压缩包 · {count} 个文件\n{}", lines.join("\n"))
}

enum LayerRow {
    Dir(String),
    File(String, u64),
}

fn current_rows(catalog: &ArchiveCatalog, directory: &str) -> Vec<LayerRow> {
    let prefix = if directory.is_empty() {
        String::new()
    } else {
        format!("{directory}/")
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for name in &catalog.names {
        let rest = if directory.is_empty() {
            name.path.as_str()
        } else if let Some(rest) = name.path.strip_prefix(&prefix) {
            rest
        } else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        if let Some((head, _)) = rest.split_once('/') {
            push_unique(&mut dirs, head);
        } else if name.directory {
            push_unique(&mut dirs, rest);
        } else {
            files.push((rest.to_string(), name.size));
        }
    }
    files.retain(|(file, _)| !dirs.iter().any(|dir| dir == file));
    dirs.sort_by(|left, right| left.to_lowercase().cmp(&right.to_lowercase()));
    files.sort_by(|left, right| left.0.to_lowercase().cmp(&right.0.to_lowercase()));
    let mut rows = dirs.into_iter().map(LayerRow::Dir).collect::<Vec<_>>();
    rows.extend(
        files
            .into_iter()
            .map(|(name, size)| LayerRow::File(name, size)),
    );
    rows
}

fn push_unique(dirs: &mut Vec<String>, name: &str) {
    if !dirs.iter().any(|dir| dir == name) {
        dirs.push(name.to_string());
    }
}

/// 路径是当前层名单上的直接子目录。只认已经读出的名字，文件、`..` 和更深的未列出路径都不算。
fn listed_child_directory(state: &InspectState, path: &str) -> bool {
    let Some(catalog) = current_catalog(state) else {
        return false;
    };
    let Some(name) = direct_child(&current_full_directory(state), path) else {
        return false;
    };
    let relative = if state.archive_nests.is_empty() {
        if state.archive_directory.is_empty() {
            name.to_string()
        } else {
            format!("{}/{name}", state.archive_directory)
        }
    } else {
        let dir = state
            .archive_nests
            .last()
            .map(|nest| nest.directory.as_str())
            .unwrap_or("");
        if dir.is_empty() {
            name.to_string()
        } else {
            format!("{dir}/{name}")
        }
    };
    directory_known(catalog, &relative)
}

/// 当前名单所在的目录，路径从最外层压缩包根算起。
fn current_full_directory(state: &InspectState) -> String {
    if let Some(nest) = state.archive_nests.last() {
        nest_location(&state.archive_nests, &nest.directory)
    } else {
        state.archive_directory.clone()
    }
}

/// `path` 是 `current` 的下一级名字时返回这个名字。
fn direct_child<'a>(current: &str, path: &'a str) -> Option<&'a str> {
    let name = if current.is_empty() {
        path
    } else {
        path.strip_prefix(current)?.strip_prefix('/')?
    };
    (!name.is_empty() && !name.contains('/')).then_some(name)
}

fn directory_known(catalog: &ArchiveCatalog, path: &str) -> bool {
    if path.is_empty() {
        return true;
    }
    let prefix = format!("{path}/");
    catalog
        .names
        .iter()
        .any(|name| (name.directory && name.path == path) || name.path.starts_with(&prefix))
}

/// 和 Vue `normalizeInternalPath` 一样去掉首尾斜杠。`.`、`..` 和盘符不接受。
fn normalize_directory(path: &str) -> Option<String> {
    let raw = path.trim().replace('\\', "/");
    if raw.starts_with('/') || raw.split('/').next().is_some_and(|part| part.contains(':')) {
        return None;
    }
    let trimmed = raw.trim_matches('/');
    if trimmed.is_empty() {
        return Some(String::new());
    }
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        parts.push(part);
    }
    Some(parts.join("/"))
}
