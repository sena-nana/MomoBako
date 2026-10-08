//! 缩略图色板。
//!
//! 颜色只认元数据里的 `thumbnailPalette` 或 `palette`，最多五枚 `#RRGGBB`。
//! 列表、预览和元数据三处共用这一份解析。

use std::collections::BTreeMap;

use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::Stack;
use serde_json::Value;

use crate::backend::services::repository::MetadataEntry;

/// 从文件条目的元数据映射读出色板。
pub(super) fn from_metadata_map(metadata: &BTreeMap<String, Value>) -> Vec<String> {
    let value = metadata.get("thumbnailPalette").or_else(|| metadata.get("palette"));
    hex_colors(value)
}

/// 从素材详情的元数据条目读出色板。
pub(super) fn from_metadata_entries(entries: &[MetadataEntry]) -> Vec<String> {
    let value = entries
        .iter()
        .find(|entry| entry.key == "thumbnailPalette")
        .or_else(|| entries.iter().find(|entry| entry.key == "palette"))
        .map(|entry| &entry.value);
    hex_colors(value)
}

/// 画出色板。没有合法颜色时不占一行。
pub(super) fn swatches(colors: &[String], prefix: &str) -> Option<AnyView> {
    if colors.is_empty() {
        return None;
    }
    let chips: Vec<AnyView> = colors
        .iter()
        .map(|color| text(color.clone()).key(format!("{prefix}-{color}")).into_any())
        .collect();
    Some(widget(Stack::row(6.0)).children(chips).key(format!("{prefix}-row")).into_any())
}

fn hex_colors(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| item.as_str())
        .filter(|color| is_hex(color))
        .take(5)
        .map(ToString::to_string)
        .collect()
}

fn is_hex(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|byte| byte.is_ascii_hexdigit())
}
