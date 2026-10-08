//! 搜索面板和筛选栏的展示数据：候选项、色块颜色、结果芯片、范围和摘要文案、库类型快捷方式。
//!
//! 对应 Vue `useSearchUi.ts` 的 `rebuildFilterOptions`、`filterColorStyle`、`searchResultContext`、
//! `searchResultScopeLabel`、`searchSummary`，以及 `useWorkspaceViewState.ts` 的
//! `activeLibrarySearchShortcuts`。这里只读状态，不改状态。

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use super::super::files::FileContext;
use super::super::inspect::SearchRow;
use super::super::inspect_shortcuts::{matches_entry, SearchShortcut};
use super::super::ShellViewModel;

#[path = "search_collate.rs"]
mod collate;

/// 评分芯片「1 星+」到「5 星+」。
pub(crate) const RATING_OPTIONS: [u8; 5] = [1, 2, 3, 4, 5];

/// 筛选栏四组候选，去空白、去重并按 zh-CN 排序。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FilterOptions {
    pub formats: Vec<String>,
    pub tags: Vec<String>,
    pub colors: Vec<String>,
    pub shapes: Vec<String>,
}

/// Vue `rebuildFilterOptions`：格式和标签来自当前仓库摘要的素材和搜索结果，
/// 颜色和形状来自搜索结果和当前目录条目的元数据。
pub(crate) fn filter_options(model: &ShellViewModel) -> FilterOptions {
    let inspect = &model.inspect;
    let mut tags = Vec::new();
    let mut formats = Vec::new();
    let mut colors = Vec::new();
    let mut shapes = Vec::new();
    let active_repo = model.workspace.active_repo_id.as_deref();
    if active_repo.is_some() && inspect.search_ui.facets_repo.as_deref() == active_repo {
        for facet in &inspect.search_ui.facets {
            tags.extend(facet.tags.iter().cloned());
            formats.push(facet.extension.clone());
        }
    }
    for row in &inspect.results {
        tags.extend(row.tags.iter().cloned());
        formats.push(result_format(row));
        colors.push(metadata_text(&row.metadata, "color"));
        shapes.push(metadata_text(&row.metadata, "shape"));
    }
    for row in &model.files.rows {
        colors.push(metadata_text(&row.metadata, "color"));
        shapes.push(metadata_text(&row.metadata, "shape"));
    }
    FilterOptions {
        formats: unique_sorted(formats),
        tags: unique_sorted(tags),
        colors: unique_sorted(colors),
        shapes: unique_sorted(shapes),
    }
}

/// 去空白、去重，再按 zh-CN 排序。
pub(crate) fn unique_sorted(values: Vec<String>) -> Vec<String> {
    let mut unique: Vec<String> = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || unique.iter().any(|item| item == value) {
            continue;
        }
        unique.push(value.to_string());
    }
    unique.sort_by(|left, right| collate::compare_zh(left, right));
    unique
}

/// 色块颜色：`#RRGGBB` 原样用，认得的颜色名映射到固定颜色，其余用主题强调色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SwatchColor {
    Rgb([u8; 3]),
    Accent,
}

/// Vue `filterColorStyle`。颜色名先按小写查，再按原文查。
pub(crate) fn swatch_color(name: &str) -> SwatchColor {
    let trimmed = name.trim();
    if let Some(rgb) = parse_hex(trimmed) {
        return SwatchColor::Rgb(rgb);
    }
    named_color(&name.to_lowercase()).or_else(|| named_color(name)).map_or(SwatchColor::Accent, SwatchColor::Rgb)
}

fn parse_hex(value: &str) -> Option<[u8; 3]> {
    let digits = value.strip_prefix('#')?;
    if digits.len() != 6 || !digits.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |index: usize| u8::from_str_radix(&digits[index..index + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// Vue `filterColorMap`。
fn named_color(name: &str) -> Option<[u8; 3]> {
    let hex = match name {
        "red" | "红色" => "#e05252",
        "green" | "绿色" => "#4f9d69",
        "blue" | "蓝色" => "#4c7bd9",
        "yellow" | "黄色" => "#d6a93f",
        "purple" | "紫色" => "#8b6bd6",
        "pink" | "粉色" => "#d66b9a",
        "orange" | "橙色" => "#d98b3d",
        "black" | "黑色" => "#333333",
        "white" | "白色" => "#e8e8e8",
        "gray" | "grey" | "灰色" => "#8c9299",
        _ => return None,
    };
    parse_hex(hex)
}

/// 结果的格式：文件名最后一个点之后的部分，小写。没有点时是空串。
pub(crate) fn result_format(row: &SearchRow) -> String {
    let filename = if row.filename.is_empty() { row.path.as_str() } else { row.filename.as_str() };
    filename.rfind('.').map(|index| filename[index + 1..].to_lowercase()).unwrap_or_default()
}

/// Vue `searchResultContext`：格式（没有时写「文件」）、前三个标签、颜色、形状和评分。
pub(crate) fn result_context(row: &SearchRow) -> Vec<String> {
    let format = result_format(row);
    let mut items = vec![if format.is_empty() { "文件".to_string() } else { format }];
    items.extend(row.tags.iter().take(3).cloned());
    items.push(metadata_text(&row.metadata, "color"));
    items.push(metadata_text(&row.metadata, "shape"));
    if let Some(rating) = row.metadata.get("rating").and_then(Value::as_f64).filter(|value| *value > 0.0) {
        items.push(format!("{} 星", js_number_text(rating)));
    }
    items.into_iter().filter(|item| !item.is_empty()).collect()
}

/// Vue `metadataText`：字符串原样，数字和布尔转成文字，其余为空串。
pub(crate) fn metadata_text(metadata: &BTreeMap<String, Value>, key: &str) -> String {
    match metadata.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.as_f64().map(js_number_text).unwrap_or_default(),
        Some(Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    }
}

/// JavaScript `String(number)` 的常见情形：整数不带小数点。
fn js_number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e21 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

/// 当前资源库名。没有活动仓库时用「当前资源库」。
fn repository_name(model: &ShellViewModel) -> String {
    model.workspace.active_repository().map(|item| item.name.clone()).unwrap_or_else(|| "当前资源库".into())
}

/// Vue `searchResultScopeLabel`。
pub(crate) fn scope_label(model: &ShellViewModel) -> String {
    if model.inspect.filters.has_active_filters() {
        format!("{}内筛选", repository_name(model))
    } else {
        "全局搜索".into()
    }
}

/// Vue `searchSummary`。
pub(crate) fn summary(model: &ShellViewModel) -> String {
    let query = &model.inspect.query;
    if model.inspect.filters.has_active_filters() {
        if query.trim().is_empty() {
            "按当前资源库筛选结果。".into()
        } else {
            format!("当前资源库筛选: {query}")
        }
    } else if query.trim().is_empty() {
        "输入关键词、标签或评分条件后，这里会展示跨仓库结果。".into()
    } else {
        format!("当前查询: {query}")
    }
}

/// Vue `activeLibrarySearchShortcuts`：当前视图的文件或搜索结果里有该库类型的条目时，
/// 才显示这个库类型贡献的快捷方式。
pub(crate) fn active_shortcuts(model: &ShellViewModel) -> Vec<SearchShortcut> {
    let shortcuts = &model.inspect.shortcuts;
    if shortcuts.is_empty() {
        return Vec::new();
    }
    let context = FileContext::from_model(model);
    let entries = model.files.visible_rows(&context);
    let mut matched: HashMap<&str, bool> = HashMap::new();
    shortcuts
        .iter()
        .filter(|shortcut| {
            *matched.entry(shortcut.library_kind.as_str()).or_insert_with(|| {
                let kind = shortcut.library_kind.as_str();
                entries.iter().filter(|row| row.kind != "directory").any(|row| matches_entry(kind, &row.metadata))
                    || model.inspect.results.iter().any(|row| matches_entry(kind, &row.metadata))
            })
        })
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "search_presenter_tests.rs"]
mod tests;
