//! 自定义缩略图：选文件、剪贴板和取消。
//!
//! 选文件走编号 6 的 `OpenFileDialog`，和导出、插件包、重定向、添加资源库、下载目录分开。
//! 剪贴板只读已有文本剪贴板：图片路径或 `data:image` 的 base64。图片解码仍交给现有缩略图保存。
//! 取消调用 `clear`，文件没有剩余路径时再 `refresh`，恢复默认缩略图。

use crate::host_bridge;
use crate::shell::{FilesEffect, ShellViewModel};

/// 对话框返回前记住的条目。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingThumbnail {
    pub repo_id: String,
    pub path: String,
    pub kind: String,
}

/// 剪贴板或对话框给出的图片来源。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThumbnailSource {
    Path(String),
    Bytes { bytes: Vec<u8>, media_type: String },
}

/// 排队选文件。没有仓库或路径时不打开对话框。
pub fn begin(model: &mut ShellViewModel, repo_id: String, path: String, kind: String) {
    if repo_id.trim().is_empty() || path.trim().is_empty() {
        eprintln!("Nana 自定义缩略图缺少仓库或路径");
        note(model, "自定义缩略图缺少仓库或路径。");
        return;
    }
    model.input.pending_thumbnail = Some(PendingThumbnail { repo_id, path, kind });
    model.input.queue_thumbnail_dialog();
}

/// 从剪贴板文本取图片并保存。没有图片时记失败，不假装已经写入。
pub fn paste(model: &mut ShellViewModel, repo_id: String, path: String, kind: String) {
    if repo_id.trim().is_empty() || path.trim().is_empty() {
        eprintln!("Nana 自定义缩略图缺少仓库或路径");
        note(model, "自定义缩略图缺少仓库或路径。");
        return;
    }
    let Some(text) = host_bridge::read_clipboard_text().filter(|text| !text.trim().is_empty()) else {
        eprintln!("Nana 剪贴板没有可用于缩略图的文本");
        note(model, "剪贴板里没有图片路径或图片数据。");
        return;
    };
    match parse_clipboard(&text) {
        Ok(source) => queue_save(model, repo_id, path, kind, source),
        Err(error) => {
            eprintln!("Nana 剪贴板不能作为自定义缩略图：{error}");
            note(model, &error);
        }
    }
}

/// 取消自定义。调度器在文件没有剩余缩略图时再刷新默认图。
pub fn clear(model: &mut ShellViewModel, repo_id: String, path: String, kind: String) {
    if repo_id.trim().is_empty() || path.trim().is_empty() {
        eprintln!("Nana 取消自定义缩略图缺少仓库或路径");
        note(model, "自定义缩略图缺少仓库或路径。");
        return;
    }
    model.files.queue_thumbnail(FilesEffect::MutateThumbnail {
        repo_id,
        path,
        kind,
        action: "clear".into(),
        source_path: None,
        image_bytes: None,
    });
}

/// 编号 6 的对话框结果。取消不写入。失败清掉等待中的条目。
pub fn complete(model: &mut ShellViewModel, paths: &[String], failed: Option<String>) -> bool {
    let Some(pending) = model.input.pending_thumbnail.take() else {
        return false;
    };
    if let Some(error) = failed {
        eprintln!("Nana 自定义缩略图对话框失败：{error}");
        note(model, &format!("自定义缩略图选择失败：{error}"));
        return true;
    }
    let Some(path) = paths.iter().map(|item| item.trim()).find(|item| !item.is_empty()) else {
        eprintln!("Nana 取消自定义缩略图选择");
        return true;
    };
    queue_save(model, pending.repo_id, pending.path, pending.kind, ThumbnailSource::Path(path.to_string()));
    true
}

/// 把剪贴板文本收成路径或图片字节。认不出就返回错误。
pub fn parse_clipboard(text: &str) -> Result<ThumbnailSource, String> {
    let text = text.trim().trim_matches('"').trim();
    if text.is_empty() {
        return Err("剪贴板里没有图片路径或图片数据。".into());
    }
    if let Some(source) = parse_data_url(text) {
        return Ok(source);
    }
    let path = text.strip_prefix("file://").unwrap_or(text).trim();
    let lower = path.to_ascii_lowercase();
    let image = ["png", "jpg", "jpeg", "webp", "bmp"].iter().any(|ext| lower.ends_with(&format!(".{ext}")));
    if image {
        return Ok(ThumbnailSource::Path(path.to_string()));
    }
    Err("剪贴板里没有图片路径或图片数据。".into())
}

fn parse_data_url(text: &str) -> Option<ThumbnailSource> {
    let rest = text.strip_prefix("data:image/")?;
    let (media, payload) = rest.split_once(";base64,")?;
    if media.is_empty() || payload.is_empty() {
        return None;
    }
    let bytes = decode_base64(payload).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(ThumbnailSource::Bytes { bytes, media_type: format!("image/{media}") })
}

/// 标准 base64。只给剪贴板的 data URL 用，不引入新的图片解码器。
fn decode_base64(input: &str) -> Result<Vec<u8>, String> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = input.bytes().filter(|byte| !byte.is_ascii_whitespace()).collect();
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return Err("base64 长度无效".into());
    }
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|byte| **byte == b'=').count();
        if pad > 2 {
            return Err("base64 填充无效".into());
        }
        let mut parts = [0u8; 4];
        for (index, byte) in chunk.iter().enumerate() {
            if *byte == b'=' {
                continue;
            }
            parts[index] = value(*byte).ok_or_else(|| "base64 含有无效字符".to_string())?;
        }
        out.push((parts[0] << 2) | (parts[1] >> 4));
        if pad < 2 {
            out.push((parts[1] << 4) | (parts[2] >> 2));
        }
        if pad < 1 {
            out.push((parts[2] << 6) | parts[3]);
        }
    }
    Ok(out)
}

fn queue_save(model: &mut ShellViewModel, repo_id: String, path: String, kind: String, source: ThumbnailSource) {
    let (source_path, image_bytes) = match source {
        ThumbnailSource::Path(path) => (Some(path), None),
        ThumbnailSource::Bytes { bytes, media_type: _ } => (None, Some(bytes)),
    };
    model.files.queue_thumbnail(FilesEffect::MutateThumbnail {
        repo_id,
        path,
        kind,
        action: "save".into(),
        source_path,
        image_bytes,
    });
}

fn note(model: &mut ShellViewModel, error: &str) {
    model.files.note_error(error.to_string());
}
