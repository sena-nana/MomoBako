//! 缩略图色板。
//!
//! 颜色只认元数据里的 `thumbnailPalette` 或 `palette`，最多五枚 `#RRGGBB`。列表、预览和元数据
//! 共用这一份解析。画法和 Vue 一致：详情预览下面是 34×12 的药丸（`ThumbnailPalette.vue`），
//! 元数据「调色板」一行是右对齐的 14×14 小方块（`file-metadata-card__palette`），都填真实颜色。

use std::collections::BTreeMap;

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{AlignSpec, JustifySpec, LengthSpec, RadiusTier, SemanticColorRole, Stack};
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

/// `ThumbnailPalette.vue`：一排 34×12 的药丸，间距 8，带 `--border-soft` 描边。没有合法颜色时不占一行。
pub(super) fn swatches(colors: &[String], prefix: &str) -> Option<AnyView> {
    let chips: Vec<AnyView> = colors
        .iter()
        .filter_map(|color| swatch(color, 34.0, 12.0, None, &format!("{prefix}-{}", color.trim_start_matches('#'))))
        .collect();
    if chips.is_empty() {
        return None;
    }
    Some(
        widget(Stack::row(8.0).wrap(true).align(AlignSpec::Center).with_layout(|layout| layout.row_gap = Some(LengthSpec::Px(8.0))))
            .children(chips)
            .key(format!("{prefix}-row"))
            .into_any(),
    )
}

/// 元数据「调色板」一行的值：右对齐的 14×14 小方块，间距 4，`Xs` 圆角。
pub(super) fn metadata_chips(colors: &[String], prefix: &str) -> Option<AnyView> {
    let chips: Vec<AnyView> = colors
        .iter()
        .filter_map(|color| swatch(color, 14.0, 14.0, Some(RadiusTier::Xs), &format!("{prefix}-{}", color.trim_start_matches('#'))))
        .collect();
    if chips.is_empty() {
        return None;
    }
    Some(
        widget(Stack::bar(4.0).justify(JustifySpec::End).align(AlignSpec::Center))
            .children(chips)
            .key(format!("{prefix}-row"))
            .into_any(),
    )
}

/// 一枚色块：底色就是元数据里的颜色，描一圈 1px `--border-soft`。`radius` 为空时画成药丸。
fn swatch(color: &str, width: f32, height: f32, radius: Option<RadiusTier>, key: &str) -> Option<AnyView> {
    let Some(rgba) = super::files_view::style::hex_rgba(color) else {
        eprintln!("Nana 色板颜色无效，跳过：{color}");
        return None;
    };
    let mut frame = super::files_view::style::fixed(Stack::row(0.0), width, height).outline(SemanticColorRole::BorderSoft, 1.0);
    frame = match radius {
        Some(tier) => frame.radius(tier),
        None => frame.radius_px(999.0),
    };
    let frame = frame.with_layout(move |layout| layout.background = Some(rgba));
    Some(widget(frame).key(key.to_string()).into_any())
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
