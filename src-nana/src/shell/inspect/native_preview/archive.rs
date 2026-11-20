//! zip、cbz 和 7z 的当前层，以及名单里单个文件的字节。
//!
//! 列表只留下已经读出的条目名，条数到 [`LIST_LIMIT`] 就停，不把条目内容读进内存。
//! 已有字节数按 1024 进位写成 B、KB、MB、GB；目录仍写「目录」。没有大小字段不编数字。
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

mod entry_size;
mod nested_depth;

mod listing;
mod present;

use listing::*;
pub(super) use present::*;

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
    // 清掉已打开的文件和在途读取。目录没有扩展名，
    // 按二进制处理，舞台写目录名和「无法内联预览该文件类型」。
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
        // 先把这一条标成当前项，再写文件名和「无法内联预览该文件类型」，不去取字节。
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

/// zip、cbz 和 7z 再列一层。cbz 当成 Zip，这里只列已经取出的字节。cbr 和 rar 仍停在名单上。
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
