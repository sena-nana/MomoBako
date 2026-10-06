//! 内置原生预览。压缩包列文件，PDF 与 Office 抽文本，模型只做结构摘要。
//!
//! PDF 不画页面，只读未压缩内容流里的文字。Open XML 仍走 zip。
//! 二进制 Office 只刮取连续的 UTF-16LE 文本，不还原版式。
//! 3MF 只数对象和网格。VRM 与 glb 一样认 `glTF` 魔数。
//! FBX 与 BLEND 只看文件头，不解析网格或 DNA。不嵌入 Three.js。
//! 读不到的格式仍走升级提示。

use std::io::{Cursor, Read};

use unarc_rs::unified::{ArchiveFormat, UnifiedArchive};
use zip::ZipArchive;

use crate::plugin_api::{NativeContributionKind, NativePluginContribution};

use super::{InspectEffect, InspectState, PreviewBinding, PreviewBody};

#[path = "native_preview_pdf.rs"]
mod pdf;

const ARCHIVE_VIEW: &str = "momobako.preview.archive";
const PDF_VIEW: &str = "momobako.preview.pdf";
const OFFICE_VIEW: &str = "momobako.preview.office";
const MODEL_VIEW: &str = "momobako.preview.model";
const LIST_LIMIT: usize = 200;
const TEXT_LIMIT: usize = 24_000;
const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const OLE_MIN_CHARS: usize = 2;

/// 壳层启动时就有的预览贡献。后登记的同扩展名贡献优先。
pub fn builtin_bindings() -> Vec<PreviewBinding> {
    vec![
        binding("momobako.preview.archive", "压缩包", ARCHIVE_VIEW, &["zip", "cbz", "7z", "rar", "cbr"], 10),
        binding("momobako.preview.pdf", "PDF", PDF_VIEW, &["pdf"], 15),
        binding(
            "momobako.preview.office",
            "文档",
            OFFICE_VIEW,
            &[
                "docx", "docm", "dotx", "xlsx", "xlsm", "pptx", "pptm", "doc", "xls", "ppt", "dot", "xlt", "pps",
            ],
            20,
        ),
        binding(
            "momobako.preview.model",
            "模型",
            MODEL_VIEW,
            &["obj", "gltf", "glb", "stl", "3mf", "vrm", "fbx", "blend"],
            30,
        ),
    ]
}

/// 已知贡献开始读取文件。未知 view 只留下标签，不假装已经能画。
pub fn begin(state: &mut InspectState, repo_id: &str, path: &str, view_id: String, label: String) {
    state.error.clear();
    if !renders(&view_id) {
        state.loading = false;
        state.activity.clear();
        state.body = PreviewBody::Native {
            content: format!("原生预览 · {label} · {view_id}"),
            view_id,
            label,
        };
        return;
    }
    state.loading = true;
    state.activity = format!("正在读取{label}…");
    state.body = PreviewBody::Native { view_id: view_id.clone(), label, content: String::new() };
    state.effects.push(InspectEffect::LoadNative {
        repo_id: repo_id.to_string(),
        path: path.to_string(),
        view_id,
        generation: state.generation,
    });
}

/// 按 view 把字节变成可显示的文本。
pub fn read(view_id: &str, bytes: &[u8]) -> Result<String, String> {
    match view_id {
        ARCHIVE_VIEW => list_archive(bytes),
        PDF_VIEW => pdf::read(bytes),
        OFFICE_VIEW => read_office(bytes),
        MODEL_VIEW => read_model(bytes),
        other => {
            eprintln!("Nana 没有这个原生预览：{other}");
            Err(format!("没有原生预览 {other}"))
        }
    }
}

/// 代次或路径过期时保留当前预览。
pub fn note_loaded(state: &mut InspectState, path: String, generation: u64, result: Result<String, String>) {
    if generation != state.generation || state.target_path.as_deref() != Some(path.as_str()) {
        eprintln!("Nana 忽略过期的原生预览：{path}");
        return;
    }
    state.loading = false;
    state.activity.clear();
    match result {
        Ok(content) => {
            if let PreviewBody::Native { content: slot, .. } = &mut state.body {
                *slot = content;
            }
            state.error.clear();
        }
        Err(error) => {
            eprintln!("Nana 原生预览失败：{error}");
            state.body = PreviewBody::Failed(error.clone());
            state.error = error;
        }
    }
}

fn renders(view_id: &str) -> bool {
    matches!(view_id, ARCHIVE_VIEW | PDF_VIEW | OFFICE_VIEW | MODEL_VIEW)
}

fn binding(plugin_id: &str, label: &str, view_id: &str, extensions: &[&str], order: i32) -> PreviewBinding {
    PreviewBinding {
        extensions: extensions.iter().map(|item| (*item).to_string()).collect(),
        contribution: NativePluginContribution::new(plugin_id, NativeContributionKind::Preview, label, view_id).with_order(order),
    }
}

/// 先按 zip 列文件。zip 打不开时再试 7z，然后是 rar。
fn list_archive(bytes: &[u8]) -> Result<String, String> {
    match ZipArchive::new(Cursor::new(bytes)) {
        Ok(archive) => list_zip(archive),
        Err(error) => {
            eprintln!("Nana 压缩包按 zip 打开失败：{error}");
            list_unarc(bytes)
        }
    }
}

fn list_zip(mut archive: ZipArchive<Cursor<&[u8]>>) -> Result<String, String> {
    let mut lines = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|error| format!("无法读取压缩包条目：{error}"))?;
        let name = file.name().replace('\\', "/");
        if name.is_empty() || name.ends_with('/') || file.is_dir() {
            continue;
        }
        lines.push(format!("{name} · {} 字节", file.size()));
        if lines.len() == LIST_LIMIT {
            eprintln!("Nana 压缩包列表在 {LIST_LIMIT} 条处截断");
            lines.push("…".into());
            break;
        }
    }
    Ok(archive_summary(lines))
}

fn list_unarc(bytes: &[u8]) -> Result<String, String> {
    match list_unarc_format(bytes, ArchiveFormat::SevenZ) {
        Ok(text) => Ok(text),
        Err(error) => {
            eprintln!("Nana 压缩包按 7z 打开失败：{error}");
            match list_unarc_format(bytes, ArchiveFormat::Rar) {
                Ok(text) => Ok(text),
                Err(rar) => {
                    eprintln!("Nana 压缩包按 rar 打开失败：{rar}");
                    Err(rar)
                }
            }
        }
    }
}

fn list_unarc_format(bytes: &[u8], format: ArchiveFormat) -> Result<String, String> {
    let mut archive = UnifiedArchive::open_with_format(Cursor::new(bytes), format).map_err(|error| format!("无法读取压缩包：{error}"))?;
    let mut entries = Vec::new();
    for item in archive.entries_iter() {
        let entry = item.map_err(|error| format!("无法读取压缩包条目：{error}"))?;
        let name = entry.name().replace('\\', "/");
        if name.is_empty() || name.ends_with('/') || entry.is_directory() {
            continue;
        }
        entries.push((name, entry.original_size()));
    }
    // unarc 的 7z 目录标记没有露到 is_directory，名字又常常不带斜杠。
    let names: Vec<String> = entries.iter().map(|(name, _)| name.clone()).collect();
    let mut lines = Vec::new();
    for (name, size) in entries {
        if names.iter().any(|other| other.starts_with(&format!("{name}/"))) {
            continue;
        }
        lines.push(format!("{name} · {size} 字节"));
        if lines.len() == LIST_LIMIT {
            eprintln!("Nana 压缩包列表在 {LIST_LIMIT} 条处截断");
            lines.push("…".into());
            break;
        }
    }
    Ok(archive_summary(lines))
}

fn archive_summary(lines: Vec<String>) -> String {
    if lines.is_empty() {
        return "压缩包里没有文件。".into();
    }
    let count = lines.iter().filter(|line| line.as_str() != "…").count();
    format!("压缩包 · {count} 个文件\n{}", lines.join("\n"))
}

/// Open XML 仍按 zip 抽文本。OLE 头先刮 UTF-16LE，避免被当成打不开的压缩包。
fn read_office(bytes: &[u8]) -> Result<String, String> {
    if bytes.starts_with(&OLE_MAGIC) {
        return scrape_ole_text(bytes);
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| format!("无法读取文档：{error}"))?;
    if let Some(xml) = zip_text(&mut archive, "word/document.xml")? {
        return plain_or_empty(&xml_plain(&xml), "文档没有可提取的文本。");
    }
    if let Some(xml) = zip_text(&mut archive, "xl/sharedStrings.xml")? {
        return plain_or_empty(&xml_plain(&xml), "表格没有可提取的文本。");
    }
    let mut slides = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|error| format!("无法读取文档条目：{error}"))?;
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
    if trimmed.is_empty() { Ok(empty.into()) } else { Ok(limit_text(trimmed)) }
}

fn zip_text(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<String>, String> {
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| format!("无法读取文档条目：{error}"))?;
        if file.name().replace('\\', "/") != name {
            continue;
        }
        let mut text = String::new();
        file.read_to_string(&mut text).map_err(|error| format!("无法读取 {name}：{error}"))?;
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
        let name = tag.trim_start_matches('/').split_whitespace().next().unwrap_or("");
        if closing && matches!(name, "w:p" | "a:p" | "si") && !out.ends_with('\n') {
            out.push('\n');
        }
        rest = &rest[end + 1..];
    }
    out.push_str(&decode_basic(rest));
    out.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n")
}

fn decode_basic(text: &str) -> String {
    text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
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
    let head = bytes.iter().position(|byte| !byte.is_ascii_whitespace()).map(|index| &bytes[index..]).unwrap_or(bytes);
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
    Ok(format!("网格 {meshes}\n节点 {nodes}\n材质 {materials}\n动画 {animations}"))
}

fn array_len(value: &serde_json::Value, key: &str) -> usize {
    value.get(key).and_then(|item| item.as_array()).map(Vec::len).unwrap_or(0)
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
        let faces = text.lines().filter(|line| line.trim().to_ascii_lowercase().starts_with("facet")).count();
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
        let token = rest.split(|c: char| c.is_whitespace() || matches!(c, '>' | '/')).next().unwrap_or("");
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
    let head = bytes.iter().position(|byte| !byte.is_ascii_whitespace()).map(|index| &bytes[index..]).unwrap_or(bytes);
    if head.starts_with(b";") || head.starts_with(b"FBX") {
        let models = bytes.windows(6).filter(|window| *window == b"Model:").count();
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
        Ok(version) if version.chars().all(|ch| ch.is_ascii_graphic()) => Some(Ok(format!("Blender 版本 {version}\n{} 字节", bytes.len()))),
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
    let names = if objects.is_empty() { "未命名".into() } else { objects.join("，") };
    Ok(format!("对象 {names}\n顶点 {vertices}\n面 {faces}"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::super::PreviewBody;
    use super::*;

    fn sample_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut archive = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for (name, bytes) in entries {
                archive.start_file(*name, options).expect("entry");
                if !name.ends_with('/') {
                    archive.write_all(bytes).expect("bytes");
                }
            }
            archive.finish().expect("finish");
        }
        cursor.into_inner()
    }

    #[test]
    fn zip_lists_files_and_docx_extracts_text() {
        let zip = sample_zip(&[("dir/", b""), ("a.txt", b"hi")]);
        let listed = read(ARCHIVE_VIEW, &zip).expect("zip");
        assert!(listed.contains("a.txt · 2 字节"));
        assert!(!listed.contains("dir/"));

        let xml = "<w:p><w:t>你好 &amp; 桃</w:t></w:p>".as_bytes();
        let docx = sample_zip(&[("word/document.xml", xml)]);
        assert_eq!(read(OFFICE_VIEW, &docx).expect("docx"), "你好 & 桃");
        assert!(read(ARCHIVE_VIEW, b"not-a-zip").is_err());
    }

    #[test]
    fn pdf_extracts_uncompressed_text_and_three_mf_counts_meshes() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<< /Length 14 >>\nstream\n(MomoBako) Tj\nendstream\nendobj\n%%EOF\n";
        assert!(read(PDF_VIEW, pdf).expect("pdf").contains("MomoBako"));
        let tj = b"%PDF-1.4\n<< /Length 22 >>\nstream\n[(Mo) 20 (moBako)] TJ\nendstream\n%%EOF\n";
        assert!(read(PDF_VIEW, tj).expect("tj").contains("MomoBako"));
        assert!(read(PDF_VIEW, b"not-a-pdf").is_err());
        assert!(read(PDF_VIEW, b"%PDF-1.4\n<< /Filter /FlateDecode >>\nstream\nxxxx\nendstream\n").is_err());

        let xml = br#"<?xml version="1.0"?>
<model xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
  <resources><object id="1" type="model"><mesh></mesh></object></resources>
  <build><item objectid="1"/></build>
</model>"#;
        let three_mf = sample_zip(&[("3D/3dmodel.model", xml)]);
        let summary = read(MODEL_VIEW, &three_mf).expect("3mf");
        assert!(summary.contains("对象 1"), "{summary}");
        assert!(summary.contains("网格 1"), "{summary}");
        assert!(read(MODEL_VIEW, &sample_zip(&[("a.txt", b"hi")])).is_err());
    }

    #[test]
    fn sevenz_lists_files_and_skips_directories() {
        // sevenz-rust2 写出的未加密 7z：目录 dir，文件 dir/a.txt=hi、b.txt=yo。
        const SEVEN_Z: &[u8] = &[
            0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c, 0x00, 0x04, 0x2c, 0x4a, 0xb6, 0xf6, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x99, 0x8e, 0x1f, 0x0e, 0x01, 0x00, 0x01, 0x68, 0x69, 0x00, 0x01, 0x00, 0x01, 0x79, 0x6f, 0x00, 0x01, 0x04, 0x06,
            0x00, 0x02, 0x09, 0x06, 0x06, 0x0a, 0x01, 0x15, 0x5f, 0xd9, 0x30, 0xd4, 0x31, 0x67, 0x7b, 0x00, 0x07, 0x0b, 0x02, 0x00, 0x01, 0x21, 0x21, 0x01,
            0x16, 0x01, 0x21, 0x21, 0x01, 0x16, 0x0c, 0x02, 0x02, 0x00, 0x08, 0x0a, 0x01, 0xac, 0x2a, 0x93, 0xd8, 0x89, 0xac, 0x29, 0x62, 0x00, 0x00, 0x05,
            0x03, 0x0e, 0x01, 0x80, 0x11, 0x29, 0x00, 0x64, 0x00, 0x69, 0x00, 0x72, 0x00, 0x00, 0x00, 0x64, 0x00, 0x69, 0x00, 0x72, 0x00, 0x2f, 0x00, 0x61,
            0x00, 0x2e, 0x00, 0x74, 0x00, 0x78, 0x00, 0x74, 0x00, 0x00, 0x00, 0x62, 0x00, 0x2e, 0x00, 0x74, 0x00, 0x78, 0x00, 0x74, 0x00, 0x00, 0x00, 0x00,
            0x00,
        ];
        let listed = read(ARCHIVE_VIEW, SEVEN_Z).expect("7z");
        assert!(listed.contains("压缩包 · 2 个文件"), "{listed}");
        assert!(listed.contains("dir/a.txt · 2 字节"), "{listed}");
        assert!(listed.contains("b.txt · 2 字节"), "{listed}");
        assert!(!listed.lines().any(|line| line.starts_with("dir ·")), "{listed}");
    }

    #[test]
    fn bindings_include_pdf_three_mf_and_non_zip_archives() {
        let bindings = builtin_bindings();
        let extensions = |view: &str| {
            bindings.iter().find(|item| item.contribution.view_id == view).expect(view).extensions.clone()
        };
        assert_eq!(extensions(PDF_VIEW), vec!["pdf".to_string()]);
        assert!(extensions(MODEL_VIEW).iter().any(|item| item == "3mf"));
        for extension in ["zip", "cbz", "7z", "rar", "cbr"] {
            assert!(extensions(ARCHIVE_VIEW).iter().any(|item| item == extension), "{extension}");
        }
    }

    #[test]
    fn obj_gltf_and_stl_summaries_count_structure() {
        let obj = read(MODEL_VIEW, b"o Cube\nv 0 0 0\nv 1 0 0\nf 1 2 1\n").expect("obj");
        assert!(obj.contains("Cube"));
        assert!(obj.contains("顶点 2"));
        assert!(obj.contains("面 1"));

        let gltf = br#"{"meshes":[{}],"nodes":[{},{}],"materials":[],"animations":[]}"#;
        let summary = read(MODEL_VIEW, gltf).expect("gltf");
        assert!(summary.contains("网格 1"));
        assert!(summary.contains("节点 2"));

        let mut stl = vec![0_u8; 84 + 50];
        stl[80..84].copy_from_slice(&1_u32.to_le_bytes());
        assert_eq!(read(MODEL_VIEW, &stl).expect("stl"), "STL 二进制\n三角形 1");
        assert!(read(MODEL_VIEW, b"plain").is_err());
    }

    #[test]
    fn ole_utf16_vrm_binding_fbx_and_blend_headers() {
        let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        for unit in "桃箱".encode_utf16() {
            ole.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(read(OFFICE_VIEW, &ole).expect("ole"), "桃箱");
        assert!(read(OFFICE_VIEW, &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]).is_err());

        let bindings = builtin_bindings();
        let extensions = |view: &str| {
            bindings.iter().find(|item| item.contribution.view_id == view).expect(view).extensions.clone()
        };
        for extension in ["doc", "xls", "ppt", "dot", "xlt", "pps"] {
            assert!(extensions(OFFICE_VIEW).iter().any(|item| item == extension), "{extension}");
        }
        for extension in ["vrm", "fbx", "blend"] {
            assert!(extensions(MODEL_VIEW).iter().any(|item| item == extension), "{extension}");
        }

        assert!(read(MODEL_VIEW, b"glTF").is_err());

        let fbx = b"; FBX 7.4\nModel: \"Cube\"";
        let summary = read(MODEL_VIEW, fbx).expect("fbx");
        assert!(summary.contains("模型 1"), "{summary}");
        assert!(!summary.contains("网格") && !summary.contains("顶点"), "{summary}");

        let binary = b"Kaydara FBX Binary";
        let summary = read(MODEL_VIEW, binary).expect("binary fbx");
        assert!(summary.contains("二进制 FBX"), "{summary}");
        assert!(summary.contains(&format!("{} 字节", binary.len())), "{summary}");
        assert!(!summary.contains("网格"), "{summary}");

        let blend = b"BLENDER-v293";
        let summary = read(MODEL_VIEW, blend).expect("blend");
        assert!(summary.contains("293"), "{summary}");
        assert!(summary.contains(&format!("{} 字节", blend.len())), "{summary}");
        assert!(!summary.contains("网格"), "{summary}");
        assert!(read(MODEL_VIEW, b"BLENDER").is_err());
    }

    #[test]
    fn stale_native_result_keeps_the_current_body() {
        let mut state = super::super::InspectState::default();
        state.generation = 2;
        state.target_path = Some("a.zip".into());
        state.body = PreviewBody::Native {
            view_id: ARCHIVE_VIEW.into(),
            label: "压缩包".into(),
            content: String::new(),
        };
        note_loaded(&mut state, "a.zip".into(), 2, Ok("压缩包 · 1 个文件".into()));
        assert!(matches!(state.body, PreviewBody::Native { ref content, .. } if content.contains("1 个文件")));
        note_loaded(&mut state, "old.zip".into(), 2, Ok("过期".into()));
        assert!(matches!(state.body, PreviewBody::Native { ref content, .. } if !content.contains("过期")));
    }
}
