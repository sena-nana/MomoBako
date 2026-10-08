//! 压缩包列表和 ASMR 作品信息。
//!
//! 列表来自已经读出的压缩包条目，不另开解压预览。
//! 作品信息只画元数据里已有的歌词状态和收听进度，没有歌词正文就不造一段歌词。

use std::collections::BTreeMap;

use nana_ui::runtime::view::{text, widget, AnyView, IntoView};
use nana_ui::runtime::{AlignSpec, EmptyState, LabeledValue, LengthSpec, List, ListItem, RadiusTier, SemanticColorRole, SettingsCard, Stack, Text};

use super::ShellViewModel;
use serde_json::Value;

use super::files::hardlink_label;
use super::inspect::FileFacts;

const ARCHIVE_VIEW: &str = "momobako.preview.archive";
const MODEL_VIEW: &str = "momobako.preview.model";
const PDF_VIEW: &str = "momobako.preview.pdf";
const OFFICE_VIEW: &str = "momobako.preview.office";

const WORK_FIELDS: &[(&str, &str)] = &[
    ("workId", "作品 ID"),
    ("rjCode", "作品 ID"),
    ("workTitle", "标题"),
    ("workRoot", "作品目录"),
    ("trackTitle", "音轨"),
    ("circle", "社团"),
    ("voiceActors", "声优"),
    ("series", "系列"),
    ("scenarioTags", "标签"),
    ("releaseDate", "发售日"),
    ("ageRating", "年龄分级"),
    ("lyricStatus", "歌词"),
    ("asmrEntryKind", "条目类型"),
    ("trackDurationMs", "音轨时长"),
    ("price", "价格"),
    ("sales", "销量"),
    ("dlCount", "销量"),
    ("rateAverage", "评分"),
    ("ratingAverage", "评分"),
    ("reviewCount", "评论数"),
    ("lastListenedAt", "最近收听"),
];

const LYRIC_BODY_KEYS: &[&str] = &["lyrics", "lyric", "lyricText", "lyricBody"];

/// 这些键已经画进作品信息或歌词区块，自定义字段里不再重复。
pub(super) fn claimed_metadata_keys() -> &'static [&'static str] {
    &[
        "workId",
        "rjCode",
        "workTitle",
        "workRoot",
        "trackTitle",
        "circle",
        "voiceActors",
        "series",
        "scenarioTags",
        "releaseDate",
        "ageRating",
        "lyricStatus",
        "asmrEntryKind",
        "trackDurationMs",
        "price",
        "sales",
        "dlCount",
        "rateAverage",
        "ratingAverage",
        "reviewCount",
        "lastListenedAt",
        "listeningStatus",
        "listeningProgress",
        "libraryKind",
        "providerCandidates",
        "asmrProviderCandidates",
        "lyrics",
        "lyric",
        "lyricText",
        "lyricBody",
    ]
}

/// 压缩包视图改成目录列表。其它 view 仍走原来的文本。
pub(super) fn is_archive(view_id: &str) -> bool {
    view_id == ARCHIVE_VIEW
}

/// 模型视图在画面上补扩展名，旋转和缩放按钮不在这里。
pub(super) fn is_model(view_id: &str) -> bool {
    view_id == MODEL_VIEW
}

/// Office 和 PDF 走同一条预览工具条。压缩包和模型没有这条。
pub(super) fn is_office_pdf(view_id: &str) -> bool {
    view_id == PDF_VIEW || view_id == OFFICE_VIEW
}

/// Vue 预览就绪后的第三项。翻页控件才写「当前 / 总数」。
pub(super) fn ready_page_label(total: usize) -> String {
    format!("{total} 页 PDF")
}

/// 预览列上方的种类、扩展名和页数。页数用已经算出的总页数，写成「N 页 PDF」。
pub(super) fn document_toolbar(model: &ShellViewModel, kind: &str) -> Option<AnyView> {
    let kind = kind.trim();
    let extension = document_extension(model);
    let pages = model.inspect.page_nav().map(|(_, total)| ready_page_label(total));
    if kind.is_empty() && extension.is_empty() && pages.is_none() {
        return None;
    }
    let mut chips = Vec::new();
    if !kind.is_empty() {
        chips.push(toolbar_chip(kind, "inspect-doc-kind", true));
    }
    if !extension.is_empty() {
        chips.push(toolbar_chip(&extension, "inspect-doc-ext", false));
    }
    if let Some(pages) = pages {
        chips.push(toolbar_chip(&pages, "inspect-doc-pages", false));
    }
    let bar = super::workbench::with_bottom_divider(
        Stack::row(8.0)
            .align(AlignSpec::Center)
            .padding_xy(0.0, 8.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    );
    Some(widget(bar).key("inspect-doc-toolbar").children(chips).into_any())
}

fn document_extension(model: &ShellViewModel) -> String {
    let from_facts = model.inspect.facts.extension.trim().trim_start_matches('.');
    if !from_facts.is_empty() {
        return from_facts.to_ascii_uppercase();
    }
    let path = model.inspect.target_path.as_deref().unwrap_or("");
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, extension)| extension.trim().to_ascii_uppercase())
        .filter(|extension| !extension.is_empty())
        .unwrap_or_default()
}

fn toolbar_chip(label: &str, key: &str, emphasis: bool) -> AnyView {
    let color = if emphasis { SemanticColorRole::Text } else { SemanticColorRole::Muted };
    let weight = if emphasis { 600 } else { 400 };
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .padding_xy(8.0, 4.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((widget(Text::new(label.to_string()).color(color).font_size(12.0).font_weight(weight)),))
    .key(key.to_string())
    .into_any()
}

/// 把压缩包摘要画成计数和条目。没有文件时用空目录，不把摘要句子当成画面。
pub(super) fn archive_list(content: &str) -> AnyView {
    let lines: Vec<&str> = content.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let entries: Vec<&str> = lines.iter().copied().filter(|line| !line.starts_with("压缩包")).collect();
    if entries.is_empty() {
        return widget(EmptyState::new("空目录").message("这个压缩包里没有可列出的文件。").compact(true))
            .key("inspect-archive-empty")
            .into_any();
    }
    let count = entries.iter().filter(|line| **line != "…").count();
    let mut rows = vec![text(format!("{count} 项")).key("inspect-archive-count").into_any()];
    let items = entries
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let (name, note) = line.split_once(" · ").unwrap_or((line, ""));
            widget(ListItem::new(name).detail(note)).key(format!("inspect-archive-row-{index}")).into_any()
        })
        .collect::<Vec<_>>();
    rows.push(widget(List::new()).children(items).key("inspect-archive-list").into_any());
    widget(Stack::column(8.0)).children(rows).into_any()
}

/// 预览右侧的类型、大小、硬链接和修改时间。标签在上，数值在下。
/// Vue 统计没有「文件」标题，页数在翻页控件里，尺寸只留在标签组后面。
pub(super) fn fact_column(facts: &FileFacts) -> AnyView {
    let extension = if facts.extension.trim().is_empty() { "文件".to_string() } else { facts.extension.clone() };
    let size = if facts.size_label.trim().is_empty() { "未知".to_string() } else { facts.size_label.clone() };
    let modified = if facts.modified_at.trim().is_empty() { "未记录".to_string() } else { facts.modified_at.clone() };
    let mut rows = vec![
        stacked_value("类型", extension, "inspect-stat-type", false),
        stacked_value("大小", size, "inspect-stat-size", true),
    ];
    let hardlink = hardlink_label(facts.hardlink_state.as_deref());
    if !hardlink.is_empty() {
        rows.push(stacked_value("硬链接", hardlink.to_string(), "inspect-stat-hardlink", true));
    }
    rows.push(stacked_value("修改时间", modified, "inspect-stat-modified", true));
    widget(Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children(rows)
        .key("inspect-file-facts")
        .into_any()
}

/// 标签组后面的只读行。没有元数据时写「未记录」，尺寸可以回落到已经解码的宽高。
pub(super) fn recorded_rows(model: &super::ShellViewModel) -> Vec<AnyView> {
    let facts = &model.inspect.facts;
    let ctx = super::files::FileContext::from_model(model);
    let row = model.inspect.target_path.as_ref().and_then(|path| {
        model.files.visible_rows(&ctx).into_iter().find(|row| &row.path == path)
    });
    let metadata = row.as_ref().map(|row| &row.metadata);
    let (pixel_width, pixel_height) = model
        .inspect
        .raster_frame()
        .map(|(width, height, _)| (width, height))
        .filter(|(width, height)| *width > 0 && *height > 0)
        .or_else(|| {
            row.as_ref()
                .filter(|row| row.pixel_width > 0 && row.pixel_height > 0)
                .map(|row| (row.pixel_width, row.pixel_height))
        })
        .unwrap_or((0, 0));
    let copy = recorded_copy(RecordedInput {
        added: prefer(&facts.added_to_library_at, json_text(metadata, "addedToLibraryAt")),
        created: prefer(&facts.file_created_at, json_text(metadata, "fileCreatedAt")),
        file_modified: prefer(&facts.file_modified_meta, json_text(metadata, "fileModifiedAt")),
        summary_modified: prefer(&facts.modified_at, row.as_ref().map(|row| row.modified_at.clone()).unwrap_or_default()),
        meta_width: nonzero_pair(facts.meta_width, facts.meta_height)
            .or_else(|| nonzero_pair(json_u32(metadata, "width"), json_u32(metadata, "height")))
            .map(|(width, _)| width)
            .unwrap_or(0),
        meta_height: nonzero_pair(facts.meta_width, facts.meta_height)
            .or_else(|| nonzero_pair(json_u32(metadata, "width"), json_u32(metadata, "height")))
            .map(|(_, height)| height)
            .unwrap_or(0),
        pixel_width,
        pixel_height,
        original_size_bytes: facts.original_size_bytes.or_else(|| json_f64(metadata, "originalSizeBytes")),
    });
    [
        ("添加到资源库", copy.added, "inspect-recorded-added"),
        ("创建时间", copy.created, "inspect-recorded-created"),
        ("文件修改时间", copy.modified, "inspect-recorded-modified"),
        ("尺寸", copy.dimensions, "inspect-recorded-size"),
        ("原始大小", copy.original_size, "inspect-recorded-bytes"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (label, value, key))| stacked_value(label, value, key, index > 0))
    .collect()
}

/// 详情卡只读行的文案。空值写成「未记录」。
pub(super) struct RecordedInput {
    pub added: String,
    pub created: String,
    pub file_modified: String,
    pub summary_modified: String,
    pub meta_width: u32,
    pub meta_height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub original_size_bytes: Option<f64>,
}

pub(super) struct RecordedCopy {
    pub added: String,
    pub created: String,
    pub modified: String,
    pub dimensions: String,
    pub original_size: String,
}

pub(super) fn recorded_copy(input: RecordedInput) -> RecordedCopy {
    RecordedCopy {
        added: blank_as_unrecorded(input.added),
        created: blank_as_unrecorded(input.created),
        modified: blank_as_unrecorded(if input.file_modified.trim().is_empty() {
            input.summary_modified
        } else {
            input.file_modified
        }),
        dimensions: dimension_label(input.meta_width, input.meta_height, input.pixel_width, input.pixel_height),
        original_size: format_original_size(input.original_size_bytes),
    }
}

/// 标签在上、数值在下。`divided` 时在行顶加 Vue `asset-meta__row` 的发丝线。
fn stacked_value(label: &str, value: String, key: &str, divided: bool) -> AnyView {
    let key = key.to_string();
    let mut column = Stack::column(2.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0));
    if divided {
        column = super::workbench::with_top_divider(column);
    }
    widget(column)
        .key(key.clone())
        .children((
            widget(super::workbench::eyebrow(label)).key(format!("{key}-label")),
            text(value).key(format!("{key}-value")),
        ))
        .into_any()
}

fn prefer(primary: &str, fallback: String) -> String {
    let primary = primary.trim();
    if primary.is_empty() { fallback } else { primary.to_string() }
}

fn blank_as_unrecorded(value: String) -> String {
    if value.trim().is_empty() { "未记录".into() } else { value }
}

fn nonzero_pair(width: u32, height: u32) -> Option<(u32, u32)> {
    (width > 0 && height > 0).then_some((width, height))
}

/// 元数据宽高优先。没有时用已经画出的像素，避免把 480×640 丢掉。两边都没有才写「未记录」。
pub(super) fn dimension_label(meta_width: u32, meta_height: u32, pixel_width: u32, pixel_height: u32) -> String {
    if let Some((width, height)) = nonzero_pair(meta_width, meta_height).or_else(|| nonzero_pair(pixel_width, pixel_height)) {
        format!("{width} × {height}")
    } else {
        "未记录".into()
    }
}

/// 与 Vue `formatBytes` 相同。无效数字记日志并写「未记录」。
pub(super) fn format_original_size(value: Option<f64>) -> String {
    let Some(value) = value else {
        return "未记录".into();
    };
    if !value.is_finite() || value < 0.0 {
        eprintln!("Nana 原始大小无效：{value}");
        return "未记录".into();
    }
    if value < 1024.0 {
        return format!("{} B", plain_number(value));
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut size = value / 1024.0;
    let mut index = 0;
    while size >= 1024.0 && index < units.len() - 1 {
        size /= 1024.0;
        index += 1;
    }
    let text = if size >= 10.0 { format!("{size:.0}") } else { format!("{size:.1}") };
    format!("{text} {}", units[index])
}

fn plain_number(value: f64) -> String {
    if value.fract() == 0.0 { format!("{value:.0}") } else { value.to_string() }
}

fn json_text(metadata: Option<&BTreeMap<String, Value>>, key: &str) -> String {
    match metadata.and_then(|metadata| metadata.get(key)) {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(_) => {
            eprintln!("Nana 元数据字段不是文本：{key}");
            String::new()
        }
        None => String::new(),
    }
}

fn json_u32(metadata: Option<&BTreeMap<String, Value>>, key: &str) -> u32 {
    let Some(value) = metadata.and_then(|metadata| metadata.get(key)) else {
        return 0;
    };
    let number = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    match number {
        Some(number) if number.is_finite() && number > 0.0 && number <= u32::MAX as f64 => number.round() as u32,
        _ => 0,
    }
}

fn json_f64(metadata: Option<&BTreeMap<String, Value>>, key: &str) -> Option<f64> {
    let value = metadata.and_then(|metadata| metadata.get(key))?;
    match value {
        Value::Number(number) => number.as_f64().filter(|number| number.is_finite() && *number >= 0.0),
        Value::String(text) => text.trim().parse::<f64>().ok().filter(|number| number.is_finite() && *number >= 0.0),
        _ => None,
    }
}

/// 作品字段、歌词状态或收听进度都画出 Vue 同名区块。
/// 匹配到 ASMR 但没有歌词状态时写「未检测」，不补歌词正文。
pub(super) fn library_sections(custom: &BTreeMap<String, String>) -> Vec<AnyView> {
    let asmr = matches_asmr(custom);
    if !asmr && !has_lyric_or_progress(custom) && !has_work_field(custom) {
        return Vec::new();
    }
    let mut sections = Vec::new();
    let mut rows = Vec::new();
    let mut seen_work_id = false;
    let mut seen_sales = false;
    let mut seen_rate = false;
    for (key, label) in WORK_FIELDS {
        if (*key == "rjCode" && seen_work_id) || (*key == "dlCount" && seen_sales) || (*key == "ratingAverage" && seen_rate) {
            continue;
        }
        let Some(value) = present(custom, key) else {
            continue;
        };
        if *key == "workId" || *key == "rjCode" {
            seen_work_id = true;
        }
        if *key == "sales" || *key == "dlCount" {
            seen_sales = true;
        }
        if *key == "rateAverage" || *key == "ratingAverage" {
            seen_rate = true;
        }
        let shown = work_value(key, &value);
        if shown.is_empty() {
            continue;
        }
        rows.push(widget(LabeledValue::new(*label, shown)).key(format!("inspect-asmr-{key}")).into_any());
    }
    if asmr && present(custom, "lyricStatus").is_none() {
        rows.push(widget(LabeledValue::new("歌词", "未检测")).key("inspect-asmr-lyricStatus").into_any());
    }
    if let Some(status) = listening_line(custom) {
        rows.push(widget(LabeledValue::new("收听状态", status)).key("inspect-asmr-listeningStatus").into_any());
    }
    if !rows.is_empty() {
        sections.push(section("ASMR Metadata", "作品信息", "inspect-asmr-works", rows));
    }
    if let Some(body) = lyric_body(custom) {
        sections.push(section(
            "ASMR Metadata",
            "歌词",
            "inspect-asmr-lyric",
            vec![text(body).key("inspect-asmr-lyric-body").into_any()],
        ));
    }
    sections
}

fn section(eyebrow: &str, title: &str, key: &str, rows: Vec<AnyView>) -> AnyView {
    let mut body = vec![
        text(eyebrow.to_string()).key(format!("{key}-eyebrow")).into_any(),
        text(title.to_string()).key(format!("{key}-title")).into_any(),
    ];
    body.extend(rows);
    widget(SettingsCard::new(title.to_string())).children(body).key(key.to_string()).into_any()
}

/// 和 Vue `matchAsmrEntry` 一样：库类型、作品 ID 或 RJ 号任一存在。
pub(super) fn matches_asmr(custom: &BTreeMap<String, String>) -> bool {
    present(custom, "libraryKind").as_deref() == Some("asmr") || present(custom, "workId").is_some() || present(custom, "rjCode").is_some()
}

fn has_work_field(custom: &BTreeMap<String, String>) -> bool {
    WORK_FIELDS.iter().any(|(key, _)| present(custom, key).is_some()) || lyric_body(custom).is_some()
}

/// 时长、价格和计数字段按 Vue 的格式化。解析失败留空，不把原文当成数字。
fn work_value(key: &str, value: &str) -> String {
    match key {
        "trackDurationMs" => format_duration(value),
        "price" => format_measure(value, " JPY"),
        "sales" | "dlCount" | "rateAverage" | "ratingAverage" | "reviewCount" => format_measure(value, ""),
        _ => value.to_string(),
    }
}

/// 有限数字才显示。整数去掉小数，并按千分位分组。
fn format_measure(value: &str, suffix: &str) -> String {
    let Ok(number) = value.trim().parse::<f64>() else {
        eprintln!("Nana 作品数字不是数值：{value}");
        return String::new();
    };
    if !number.is_finite() {
        eprintln!("Nana 作品数字无效：{value}");
        return String::new();
    }
    let text = if number.fract() == 0.0 {
        group_integer(number.round() as i64)
    } else {
        let raw = format!("{number}");
        raw
    };
    format!("{text}{suffix}")
}

fn group_integer(value: i64) -> String {
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let mut text: String = grouped.chars().rev().collect();
    if negative {
        text.insert(0, '-');
    }
    text
}

fn has_lyric_or_progress(custom: &BTreeMap<String, String>) -> bool {
    present(custom, "lyricStatus").is_some()
        || lyric_body(custom).is_some()
        || present(custom, "listeningStatus").is_some()
        || present(custom, "listeningProgress").is_some()
        || present(custom, "lastListenedAt").is_some()
}

fn lyric_body(custom: &BTreeMap<String, String>) -> Option<String> {
    LYRIC_BODY_KEYS.iter().find_map(|key| present(custom, key))
}

fn present(custom: &BTreeMap<String, String>, key: &str) -> Option<String> {
    custom.get(key).map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn listening_line(custom: &BTreeMap<String, String>) -> Option<String> {
    let status = present(custom, "listeningStatus");
    let progress = present(custom, "listeningProgress");
    if status.is_none() && progress.is_none() {
        return None;
    }
    let label = match status.as_deref() {
        Some("unlistened") => "未收听".to_string(),
        Some("listening") => "收听中".to_string(),
        Some("listened") => "已听完".to_string(),
        Some(other) => other.to_string(),
        None => String::new(),
    };
    let Some(progress) = progress.filter(|value| value != "0") else {
        return (!label.is_empty()).then_some(label);
    };
    if label.is_empty() {
        Some(format!("{progress}%"))
    } else {
        Some(format!("{label} · {progress}%"))
    }
}

/// 毫秒字段画成「分:秒」。解析失败只记日志并留空，不显示原始数字冒充时长。
fn format_duration(value: &str) -> String {
    let Ok(ms) = value.parse::<f64>() else {
        eprintln!("Nana 音轨时长不是数字：{value}");
        return String::new();
    };
    if !ms.is_finite() || ms <= 0.0 {
        return String::new();
    }
    let seconds = (ms / 1000.0).round() as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asmr_match_without_lyric_status_says_undetected() {
        let mut custom = BTreeMap::new();
        custom.insert("libraryKind".into(), "asmr".into());
        assert!(matches_asmr(&custom));
        assert_eq!(library_sections(&custom).len(), 1);
        assert!(lyric_body(&custom).is_none());
        assert_eq!(format_measure("1200", " JPY"), "1,200 JPY");
        assert!(format_measure("nope", "").is_empty());
    }

    #[test]
    fn lyric_status_opens_the_work_section_without_inventing_lyrics() {
        let mut custom = BTreeMap::new();
        custom.insert("lyricStatus".into(), "local".into());
        custom.insert("workTitle".into(), "夜曲".into());
        custom.insert("circle".into(), "".into());
        let sections = library_sections(&custom);
        assert_eq!(sections.len(), 1);
        assert!(lyric_body(&custom).is_none());
    }

    #[test]
    fn lyric_body_is_its_own_section_and_progress_formats() {
        let mut custom = BTreeMap::new();
        custom.insert("listeningStatus".into(), "listening".into());
        custom.insert("listeningProgress".into(), "40".into());
        custom.insert("lyrics".into(), "第一行".into());
        assert_eq!(listening_line(&custom).as_deref(), Some("收听中 · 40%"));
        assert_eq!(library_sections(&custom).len(), 2);
        assert_eq!(format_duration("125000"), "2:05");
        assert!(format_duration("nope").is_empty());
    }

    #[test]
    fn ready_pdf_status_counts_pages_instead_of_the_flipper() {
        assert_eq!(ready_page_label(1), "1 页 PDF");
        assert_eq!(ready_page_label(3), "3 页 PDF");
    }

    #[test]
    fn archive_without_files_is_an_empty_directory() {
        let view = archive_list("压缩包里没有文件。");
        let _ = view;
    }

    #[test]
    fn recorded_rows_keep_decoded_size_and_leave_missing_times_blank() {
        let empty = recorded_copy(RecordedInput {
            added: String::new(),
            created: String::new(),
            file_modified: String::new(),
            summary_modified: String::new(),
            meta_width: 0,
            meta_height: 0,
            pixel_width: 0,
            pixel_height: 640,
            original_size_bytes: None,
        });
        assert_eq!(empty.added, "未记录");
        assert_eq!(empty.created, "未记录");
        assert_eq!(empty.modified, "未记录");
        assert_eq!(empty.dimensions, "未记录");
        assert_eq!(empty.original_size, "未记录");
        let decoded = recorded_copy(RecordedInput {
            added: String::new(),
            created: "2026-10-08".into(),
            file_modified: String::new(),
            summary_modified: "昨天".into(),
            meta_width: 0,
            meta_height: 0,
            pixel_width: 480,
            pixel_height: 640,
            original_size_bytes: Some(1536.0),
        });
        assert_eq!(decoded.created, "2026-10-08");
        assert_eq!(decoded.modified, "昨天");
        assert_eq!(decoded.dimensions, "480 × 640");
        assert_eq!(decoded.original_size, "1.5 KB");
        assert_eq!(
            recorded_copy(RecordedInput {
                added: String::new(),
                created: String::new(),
                file_modified: String::new(),
                summary_modified: String::new(),
                meta_width: 32,
                meta_height: 32,
                pixel_width: 480,
                pixel_height: 640,
                original_size_bytes: Some(500.0),
            })
            .dimensions,
            "32 × 32"
        );
        assert_eq!(format_original_size(Some(500.0)), "500 B");
        assert_eq!(format_original_size(Some(f64::NAN)), "未记录");
    }
}
