//! 内置原生预览。压缩包列文件，PDF 画页面，Office 抽文本，模型能解析的网格会光栅化。
//!
//! 扩展名到预览视图的对应在 [`builtin_view`]：原来由 `preview-archive`、`office-preview`、
//! `three-model-preview` 插件前端绘制的扩展名都在这里。插件不带预览，清单里的预览声明宿主不读。
//! PDF 按页树数页、翻页（`native_preview_pdf*.rs`）。解不开的滤镜记成该页失败，不假装成空白成功。
//! zip、cbz 和 7z 只显示当前层的真实名字，下一层仍用已经解出的名字（`native_preview_archive`）。
//! 点开名单里已经取出的一条时，文本、图片、PDF、Office 和模型走现有预览。
//! Office 只抽文本；能光栅的模型用现有软件光栅，FBX 与 BLEND 只留文件头。
//! 现有解码器能在内存里解开的音频走唱片舞台，不打开声卡。
//! 未压缩 AVI 用纯 Rust 解码画出真实帧，PCM 丢掉，播放条不接管。其它视频和必须另找解码器的格式停在名单上。
//! 取出的 PDF 用自绘页；取出的 zip、cbz 和 7z 再用当前层真实名字列这一层。rar 和 cbr 不进入子目录，也不打开包内文件。
//! Open XML 仍走 zip。二进制 Office 只刮取连续的 UTF-16LE 文本，不还原版式。不嵌 pdf.js。
//! OBJ、STL、glTF/GLB、3MF、VRM 能取出三角形就软件光栅成页图，编码成 PNG 画进预览框。
//! FBX 与 BLEND 只看文件头。不嵌入 Three.js，也不调用 ffmpeg。

use std::io::{Cursor, Read};

use zip::ZipArchive;

use super::bridge::{MeshData, NativeLoad, PageFrame};
use super::{InspectEffect, InspectState, PreviewBody};

mod archive;

mod mesh;
mod pdf;

const ARCHIVE_VIEW: &str = "momobako.preview.archive";
const PDF_VIEW: &str = "momobako.preview.pdf";
const OFFICE_VIEW: &str = "momobako.preview.office";
const MODEL_VIEW: &str = "momobako.preview.model";
const TEXT_LIMIT: usize = 24_000;
const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const OLE_MIN_CHARS: usize = 2;

/// 扩展名有内置绘制时返回它的视图和种类名：压缩包、PDF、Office 和模型。不分大小写，前导的点去掉。
/// Office 的种类名给顶栏第一枚芯片用。
pub fn builtin_view(extension: &str) -> Option<(&'static str, &'static str)> {
    let extension = extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    if extension == "pdf" {
        return Some((PDF_VIEW, "PDF"));
    }
    if matches!(extension.as_str(), "zip" | "cbz" | "7z" | "rar" | "cbr") {
        return Some((ARCHIVE_VIEW, "压缩包"));
    }
    if let Some(label) = office_kind_label(&extension) {
        return Some((OFFICE_VIEW, label));
    }
    if matches!(
        extension.as_str(),
        "fbx" | "obj" | "glb" | "gltf" | "vrm" | "stl" | "3mf" | "blend"
    ) {
        return Some((MODEL_VIEW, "模型"));
    }
    None
}

/// Office 顶栏种类：文字是 Word，表格是 Excel，演示是 PowerPoint。
/// 不是这三组扩展名时返回 `None`，不给内置 Office 视图。
fn office_kind_label(extension: &str) -> Option<&'static str> {
    const WORD: &[&str] = &["docx", "docm", "doc", "dotx", "dotm", "dot"];
    const SHEET: &[&str] = &["xlsx", "xlsm", "xlsb", "xls", "xltx", "xltm", "xlt"];
    const SLIDE: &[&str] = &[
        "pptx", "pptm", "ppt", "ppsx", "ppsm", "pps", "potx", "potm", "pot",
    ];
    if WORD.contains(&extension) {
        Some("Word")
    } else if SHEET.contains(&extension) {
        Some("Excel")
    } else if SLIDE.contains(&extension) {
        Some("PowerPoint")
    } else {
        None
    }
}

/// 内置视图开始读取文件：先标读取中，读到的字节交给 [`load`]。
pub fn begin(state: &mut InspectState, repo_id: &str, path: &str, view_id: String, label: String) {
    state.error.clear();
    state.loading = true;
    state.archive = None;
    state.archive_directory.clear();
    state.archive_selected = None;
    state.archive_nests.clear();
    state.archive_entry = None;
    state.archive_request = None;
    state.archive_notice = None;
    state.body = PreviewBody::Native {
        view_id: view_id.clone(),
        label,
        content: String::new(),
    };
    state.effects.push(InspectEffect::LoadNative {
        repo_id: repo_id.to_string(),
        path: path.to_string(),
        view_id,
        generation: state.generation,
    });
}

/// 按 view 把字节变成可显示的文本。页面和网格在 `load` 里。
#[cfg(test)]
pub fn read(view_id: &str, bytes: &[u8]) -> Result<String, String> {
    load(view_id, bytes).map(|item| item.text)
}

/// 文本、PDF 页面和可选网格一次读出。
pub fn load(view_id: &str, bytes: &[u8]) -> Result<NativeLoad, String> {
    match view_id {
        ARCHIVE_VIEW => archive::list(bytes),
        PDF_VIEW => pdf::load(bytes),
        OFFICE_VIEW => read_office(bytes).map(NativeLoad::text_only),
        MODEL_VIEW => mesh::load(bytes),
        other => {
            eprintln!("Nana 没有这个原生预览：{other}");
            Err(format!("没有原生预览 {other}"))
        }
    }
}

/// 旋转或缩放之后重画网格。没有三角形时返回明确失败帧。
pub(super) fn paint_mesh(mesh: &MeshData, yaw: f32, zoom: f32) -> PageFrame {
    mesh::raster(mesh, yaw, zoom)
}

/// 模型左下角的尺寸：三轴包围盒跨度，每轴保留两位小数。
pub(super) fn model_extent_label(mesh: &MeshData) -> String {
    mesh::extent_label(mesh)
}

/// 代次或路径过期时保留当前预览。
pub fn note_loaded(
    state: &mut InspectState,
    path: String,
    generation: u64,
    result: Result<NativeLoad, String>,
) {
    if generation != state.generation || state.target_path.as_deref() != Some(path.as_str()) {
        eprintln!("Nana 忽略过期的原生预览：{path}");
        return;
    }
    state.loading = false;
    match result {
        Ok(loaded) => {
            if let PreviewBody::Native { content: slot, .. } = &mut state.body {
                *slot = loaded.text;
            }
            state.archive = loaded.archive;
            state.archive_directory.clear();
            state.archive_selected = None;
            state.archive_nests.clear();
            state.archive_entry = None;
            state.archive_request = None;
            state.archive_notice = None;
            super::bridge::install(state, loaded.frames, loaded.mesh, loaded.paged);
            state.error.clear();
        }
        Err(error) => {
            eprintln!("Nana 原生预览失败：{error}");
            state.archive = None;
            state.archive_directory.clear();
            state.archive_selected = None;
            state.archive_nests.clear();
            state.archive_entry = None;
            state.archive_request = None;
            state.archive_notice = None;
            state.body = PreviewBody::Failed(error.clone());
            state.error = error;
        }
    }
}

/// 选中 zip/7z 当前层已经列出的目录。不进入，不重新打开压缩包。
pub(super) fn select_directory(state: &mut InspectState, path: &str) {
    archive::select(state, path);
}

/// 进入 zip/7z 里已经解出的下一层。不重新读取压缩包。
pub(super) fn enter_directory(state: &mut InspectState, path: &str) {
    archive::enter(state, path);
}

/// 打开名单里的一个文件。这里只校验名字和类型，字节由副作用读取。
pub(super) fn open_archive_file(state: &mut InspectState, path: &str) {
    archive::request_file(state, path);
}

/// 包内文件读完以后画进预览。过期或取不出时仍停在当前名单上。
pub(super) fn note_archive_entry(
    state: &mut InspectState,
    path: String,
    entry: String,
    through: Vec<String>,
    generation: u64,
    result: Result<super::bridge::ArchiveEntryPreview, String>,
) {
    archive::note_entry(state, path, entry, through, generation, result);
}

/// 包内 PDF 翻页。不是包内 PDF 时交给原来的页甲板。
pub(super) fn turn_archive_pdf(state: &mut InspectState, delta: i32) -> bool {
    archive::turn_pdf(state, delta)
}

/// 包内模型的旋转和缩放。不是这种预览时交给原来的网格。
pub(super) fn orbit_archive_model(state: &mut InspectState, yaw: f32, zoom: f32) -> bool {
    archive::orbit_model(state, yaw, zoom)
}

/// 从压缩包字节里只取出已经列出的那一条。`through` 是先取出的嵌套 zip/7z。
pub(crate) fn preview_archive_entry(
    bytes: &[u8],
    through: &[String],
    entry: &str,
) -> Result<super::bridge::ArchiveEntryPreview, String> {
    archive::preview_chain(bytes, through, entry)
}

/// Open XML 仍按 zip 抽文本。OLE 头先刮 UTF-16LE，避免被当成打不开的压缩包。
fn read_office(bytes: &[u8]) -> Result<String, String> {
    if bytes.starts_with(&OLE_MAGIC) {
        return scrape_ole_text(bytes);
    }
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|error| format!("无法读取文档：{error}"))?;
    if let Some(xml) = zip_text(&mut archive, "word/document.xml")? {
        return plain_or_empty(&xml_plain(&xml), "文档没有可提取的文本。");
    }
    if let Some(xml) = zip_text(&mut archive, "xl/sharedStrings.xml")? {
        return plain_or_empty(&xml_plain(&xml), "表格没有可提取的文本。");
    }
    let mut slides = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|error| format!("无法读取文档条目：{error}"))?;
        let name = file.name().replace('\\', "/");
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
            slides.push(name);
        }
    }
    slides.sort();
    let mut text = String::new();
    for name in slides {
        if let Some(xml) = zip_text(&mut archive, &name)? {
            text.push_str(&xml_plain(&xml));
            text.push('\n');
        }
    }
    plain_or_empty(&text, "演示文稿没有可提取的文本。")
}

/// 二进制 Office 是 OLE 复合文档。按双字节读 UTF-16LE，控制字符切开片段。
///
/// 私用区、非字符和零宽字符也不算可读。短于两个字符的片段丢掉。
/// 剩下的片段按出现顺序拼成纯文本，不还原段落或表格。
fn scrape_ole_text(bytes: &[u8]) -> Result<String, String> {
    let mut pieces = Vec::new();
    let mut run = String::new();
    let mut index = 0;
    while index + 1 < bytes.len() {
        match next_utf16_char(bytes, &mut index) {
            Some(ch) if readable_utf16(ch) => run.push(ch),
            _ => finish_ole_run(&mut run, &mut pieces),
        }
    }
    finish_ole_run(&mut run, &mut pieces);
    if pieces.is_empty() {
        eprintln!("Nana OLE 文档没有可读的 UTF-16 文本");
        return Err("文档没有可读文本".into());
    }
    Ok(limit_text(&pieces.join("\n")))
}

fn next_utf16_char(bytes: &[u8], index: &mut usize) -> Option<char> {
    if *index + 1 >= bytes.len() {
        return None;
    }
    let unit = u16::from_le_bytes([bytes[*index], bytes[*index + 1]]);
    *index += 2;
    if (0xD800..0xDC00).contains(&unit) {
        if *index + 1 >= bytes.len() {
            return None;
        }
        let low = u16::from_le_bytes([bytes[*index], bytes[*index + 1]]);
        if !(0xDC00..0xE000).contains(&low) {
            return None;
        }
        *index += 2;
        let point = 0x10000 + (((unit as u32) - 0xD800) << 10) + ((low as u32) - 0xDC00);
        return char::from_u32(point);
    }
    if (0xDC00..0xE000).contains(&unit) {
        return None;
    }
    char::from_u32(unit as u32)
}

fn readable_utf16(ch: char) -> bool {
    if ch.is_control() || is_private_use(ch) || is_noncharacter(ch) {
        return false;
    }
    let code = ch as u32;
    !matches!(code, 0x00AD | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB)
}

fn is_private_use(ch: char) -> bool {
    matches!(ch as u32, 0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD)
}

fn is_noncharacter(ch: char) -> bool {
    let code = ch as u32;
    (0xFDD0..=0xFDEF).contains(&code) || (code & 0xFFFE) == 0xFFFE
}

fn finish_ole_run(run: &mut String, pieces: &mut Vec<String>) {
    let trimmed = run.trim();
    if trimmed.chars().count() >= OLE_MIN_CHARS {
        pieces.push(trimmed.to_string());
    }
    run.clear();
}

fn plain_or_empty(text: &str, empty: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        eprintln!("Nana 文档没有抽出文本：{empty}");
        Err(empty.into())
    } else {
        Ok(limit_text(trimmed))
    }
}

fn zip_text(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<String>, String> {
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| format!("无法读取文档条目：{error}"))?;
        if file.name().replace('\\', "/") != name {
            continue;
        }
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|error| format!("无法读取 {name}：{error}"))?;
        return Ok(Some(text));
    }
    Ok(None)
}

/// 段落结束换行，其余标签丢掉。只解开文档里常见的五个实体。
fn xml_plain(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        out.push_str(&decode_basic(&rest[..start]));
        rest = &rest[start + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = rest[..end].trim();
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("");
        if closing && matches!(name, "w:p" | "a:p" | "si") && !out.ends_with('\n') {
            out.push('\n');
        }
        rest = &rest[end + 1..];
    }
    out.push_str(&decode_basic(rest));
    out.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_basic(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

fn limit_text(text: &str) -> String {
    if text.chars().count() <= TEXT_LIMIT {
        return text.to_string();
    }
    eprintln!("Nana 原生预览文本在 {TEXT_LIMIT} 字处截断");
    let mut clipped: String = text.chars().take(TEXT_LIMIT).collect();
    clipped.push_str("\n…");
    clipped
}

/// VRM 就是 glb，靠 `glTF` 魔数走同一条摘要。FBX 与 BLEND 在网格解析之前返回。
fn read_model(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() >= 4 && &bytes[..4] == b"glTF" {
        return summarize_glb(bytes);
    }
    if looks_like_zip(bytes) {
        return summarize_3mf(bytes);
    }
    if let Some(summary) = summarize_fbx(bytes) {
        return summary;
    }
    if let Some(summary) = summarize_blend(bytes) {
        return summary;
    }
    let head = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|index| &bytes[index..])
        .unwrap_or(bytes);
    if head.first() == Some(&b'{') {
        return summarize_gltf(bytes);
    }
    if let Some(summary) = summarize_stl(bytes) {
        return Ok(summary);
    }
    summarize_obj(bytes)
}

fn summarize_glb(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < 20 || &bytes[..4] != b"glTF" {
        eprintln!("Nana 模型不是 glb：文件头不匹配");
        return Err("不是 glb 文件".into());
    }
    let chunk_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let chunk_type = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    if chunk_type != 0x4E4F_534A {
        eprintln!("Nana glb 第一段不是 JSON");
        return Err("glb 第一段不是 JSON".into());
    }
    let end = 20 + chunk_len;
    if end > bytes.len() {
        eprintln!("Nana glb JSON 超出文件长度");
        return Err("glb JSON 超出文件长度".into());
    }
    summarize_gltf(&bytes[20..end])
}

fn summarize_gltf(bytes: &[u8]) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|error| {
        eprintln!("Nana gltf JSON 无法解析：{error}");
        format!("gltf JSON 无法解析：{error}")
    })?;
    let meshes = array_len(&value, "meshes");
    let nodes = array_len(&value, "nodes");
    let materials = array_len(&value, "materials");
    let animations = array_len(&value, "animations");
    if meshes == 0 && nodes == 0 {
        eprintln!("Nana gltf 没有网格或节点");
        return Err("gltf 里没有网格或节点".into());
    }
    Ok(format!(
        "网格 {meshes}\n节点 {nodes}\n材质 {materials}\n动画 {animations}"
    ))
}

fn array_len(value: &serde_json::Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(|item| item.as_array())
        .map(Vec::len)
        .unwrap_or(0)
}

fn summarize_stl(bytes: &[u8]) -> Option<String> {
    if bytes.len() >= 84 {
        let count = u32::from_le_bytes(bytes[80..84].try_into().ok()?) as usize;
        if count > 0 && bytes.len() == 84 + count * 50 {
            return Some(format!("STL 二进制\n三角形 {count}"));
        }
    }
    let text = String::from_utf8_lossy(bytes);
    if text.trim_start().to_ascii_lowercase().starts_with("solid") {
        let faces = text
            .lines()
            .filter(|line| line.trim().to_ascii_lowercase().starts_with("facet"))
            .count();
        if faces > 0 {
            return Some(format!("STL 文本\n三角形 {faces}"));
        }
    }
    None
}

fn looks_like_zip(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && matches!(&bytes[..4], b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08")
}

/// 3MF 是 zip。模型 XML 通常在 `3D/3dmodel.model`，这里只数对象和网格。
fn summarize_3mf(bytes: &[u8]) -> Result<String, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        eprintln!("Nana 3MF 无法作为 zip 打开：{error}");
        format!("无法读取 3MF：{error}")
    })?;
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|error| {
            eprintln!("Nana 3MF 条目读取失败：{error}");
            format!("无法读取 3MF 条目：{error}")
        })?;
        names.push(file.name().replace('\\', "/"));
    }
    let model_name = names
        .iter()
        .find(|name| name.eq_ignore_ascii_case("3D/3dmodel.model"))
        .or_else(|| {
            names.iter().find(|name| {
                let lower = name.to_ascii_lowercase();
                lower == "3dmodel.model" || lower.ends_with("/3dmodel.model")
            })
        })
        .cloned();
    let Some(model_name) = model_name else {
        eprintln!("Nana 3MF 缺少 3D/3dmodel.model");
        return Err("3MF 里没有模型".into());
    };
    let Some(xml) = zip_text(&mut archive, &model_name)? else {
        eprintln!("Nana 3MF 读不到 {model_name}");
        return Err("3MF 里没有模型".into());
    };
    let objects = count_xml_tag(&xml, "object");
    let meshes = count_xml_tag(&xml, "mesh");
    if objects == 0 && meshes == 0 {
        eprintln!("Nana 3MF 没有对象或网格");
        return Err("3MF 里没有对象或网格".into());
    }
    Ok(format!("对象 {objects}\n网格 {meshes}"))
}

fn count_xml_tag(xml: &str, name: &str) -> usize {
    let mut count = 0;
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        match rest.chars().next() {
            Some('/' | '!' | '?') => continue,
            _ => {}
        }
        let token = rest
            .split(|c: char| c.is_whitespace() || matches!(c, '>' | '/'))
            .next()
            .unwrap_or("");
        let local = token.rsplit(':').next().unwrap_or(token);
        if local == name {
            count += 1;
        }
    }
    count
}

/// ASCII FBX 以 `;` 或 `FBX` 开头，只数 `Model:`。二进制只认 Kaydara 魔数和字节数。
fn summarize_fbx(bytes: &[u8]) -> Option<Result<String, String>> {
    if bytes.starts_with(b"Kaydara FBX Binary") {
        return Some(Ok(format!("二进制 FBX\n{} 字节", bytes.len())));
    }
    let head = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|index| &bytes[index..])
        .unwrap_or(bytes);
    if head.starts_with(b";") || head.starts_with(b"FBX") {
        let models = bytes
            .windows(6)
            .filter(|window| *window == b"Model:")
            .count();
        return Some(Ok(format!("FBX\n模型 {models}")));
    }
    None
}

/// 版本是 12 字节文件头末尾的三个字符。不解析 DNA。
fn summarize_blend(bytes: &[u8]) -> Option<Result<String, String>> {
    if !bytes.starts_with(b"BLENDER") {
        return None;
    }
    if bytes.len() < 12 {
        eprintln!("Nana BLEND 文件头不足 12 字节");
        return Some(Err("BLEND 文件头不完整".into()));
    }
    let version = &bytes[9..12];
    match std::str::from_utf8(version) {
        Ok(version) if version.chars().all(|ch| ch.is_ascii_graphic()) => {
            Some(Ok(format!("Blender 版本 {version}\n{} 字节", bytes.len())))
        }
        _ => {
            eprintln!("Nana BLEND 文件头没有可读版本");
            Some(Err("BLEND 文件头没有版本".into()))
        }
    }
}

fn summarize_obj(bytes: &[u8]) -> Result<String, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut vertices = 0;
    let mut faces = 0;
    let mut objects = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("v ") {
            vertices += 1;
        } else if line.starts_with("f ") {
            faces += 1;
        } else if let Some(name) = line.strip_prefix("o ") {
            let name = name.trim();
            if !name.is_empty() {
                objects.push(name.to_string());
            }
        }
    }
    if vertices == 0 && faces == 0 {
        eprintln!("Nana OBJ 没有顶点或面");
        return Err("OBJ 里没有顶点或面".into());
    }
    let names = if objects.is_empty() {
        "未命名".into()
    } else {
        objects.join("，")
    };
    Ok(format!("对象 {names}\n顶点 {vertices}\n面 {faces}"))
}

#[cfg(test)]
mod tests;
