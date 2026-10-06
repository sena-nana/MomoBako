//! 内置原生预览。压缩包列文件，Open XML 抽文本，模型只做结构摘要。
//!
//! 不嵌入 PDF.js、Three.js 或旧版 Office。读不到的格式仍走升级提示。

use std::io::{Cursor, Read};

use zip::ZipArchive;

use crate::plugin_api::{NativeContributionKind, NativePluginContribution};

use super::{InspectEffect, InspectState, PreviewBinding, PreviewBody};

const ARCHIVE_VIEW: &str = "momobako.preview.archive";
const OFFICE_VIEW: &str = "momobako.preview.office";
const MODEL_VIEW: &str = "momobako.preview.model";
const LIST_LIMIT: usize = 200;
const TEXT_LIMIT: usize = 24_000;

/// 壳层启动时就有的预览贡献。后登记的同扩展名贡献优先。
pub fn builtin_bindings() -> Vec<PreviewBinding> {
    vec![
        binding("momobako.preview.archive", "压缩包", ARCHIVE_VIEW, &["zip", "cbz"], 10),
        binding(
            "momobako.preview.office",
            "文档",
            OFFICE_VIEW,
            &["docx", "docm", "dotx", "xlsx", "xlsm", "pptx", "pptm"],
            20,
        ),
        binding("momobako.preview.model", "模型", MODEL_VIEW, &["obj", "gltf", "glb", "stl"], 30),
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
        ARCHIVE_VIEW => list_zip(bytes),
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
    matches!(view_id, ARCHIVE_VIEW | OFFICE_VIEW | MODEL_VIEW)
}

fn binding(plugin_id: &str, label: &str, view_id: &str, extensions: &[&str], order: i32) -> PreviewBinding {
    PreviewBinding {
        extensions: extensions.iter().map(|item| (*item).to_string()).collect(),
        contribution: NativePluginContribution::new(plugin_id, NativeContributionKind::Preview, label, view_id).with_order(order),
    }
}

fn list_zip(bytes: &[u8]) -> Result<String, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| format!("无法读取压缩包：{error}"))?;
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
    if lines.is_empty() {
        return Ok("压缩包里没有文件。".into());
    }
    Ok(format!("压缩包 · {} 个文件\n{}", lines.iter().filter(|line| line.as_str() != "…").count(), lines.join("\n")))
}

fn read_office(bytes: &[u8]) -> Result<String, String> {
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

fn read_model(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() >= 4 && &bytes[..4] == b"glTF" {
        return summarize_glb(bytes);
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
