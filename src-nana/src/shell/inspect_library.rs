//! 压缩包列表和 ASMR 作品信息。
//!
//! 列表来自已经读出的压缩包条目，不另开解压预览。
//! 作品信息只画元数据里已有的歌词状态和收听进度，没有歌词正文就不造一段歌词。行的顺序和文案照 Vue
//! `library-asmr` 的元数据面板；只有 ASMR 资源库扩展开着（[`asmr_enabled`]）、条目是 ASMR 条目时才有。

use std::collections::BTreeMap;

use nana_ui::runtime::view::{fields, text, untrack, widget, AnyView, IntoView, Signal};
use nana_ui::runtime::{AlignSpec, EmptyState, LabeledValue, LengthSpec, List, ListItem, RadiusTier, SemanticColorRole, SettingsCard, Stack, Text};
use serde_json::Value;

use crate::backend::services::repository::PluginManifest;

use super::ShellViewModel;

use super::files::hardlink_label;
use super::inspect::FileFacts;

const ARCHIVE_VIEW: &str = "momobako.preview.archive";
const MODEL_VIEW: &str = "momobako.preview.model";
const PDF_VIEW: &str = "momobako.preview.pdf";
const OFFICE_VIEW: &str = "momobako.preview.office";

/// 作品信息的行：键和标签，顺序照 Vue。值由 [`work_value`] 按键取（有的键有后备键），取不到的行不画。
const WORK_ROWS: &[(&str, &str)] = &[
    ("workId", "作品 ID"),
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
    ("listeningStatus", "收听状态"),
    ("lastListenedAt", "最近收听"),
    ("trackDurationMs", "音轨时长"),
    ("price", "价格"),
    ("sales", "销量"),
    ("rateAverage", "评分"),
    ("reviewCount", "评论数"),
];

const LYRIC_BODY_KEYS: &[&str] = &["lyrics", "lyric", "lyricText", "lyricBody"];

/// ASMR 资源库扩展开着没有：有启用、状态可用、声明了 `libraryExtension.libraryKind = "asmr"` 的插件。
/// Vue 里扩展由这个插件的前端注册，停用插件以后作品信息、补全候选、作品队列和文件列表摘要都不出现。
pub(super) fn asmr_enabled(plugins: &[PluginManifest]) -> bool {
    plugins.iter().any(|plugin| {
        plugin.enabled
            && !matches!(plugin.status.as_str(), "disabled" | "unavailable" | "error")
            && plugin.contributes.get("libraryExtension").and_then(|extension| extension.get("libraryKind")).and_then(Value::as_str)
                == Some("asmr")
    })
}

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
        "tags",
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

/// PDF 读取中写「载入 PDF」，Office 写「转换文档」。
pub(super) fn is_pdf(view_id: &str) -> bool {
    view_id == PDF_VIEW
}

/// Vue 预览就绪后的第三项。翻页控件才写「当前 / 总数」。
pub(super) fn ready_page_label(total: usize) -> String {
    format!("{total} 页 PDF")
}

/// PDF 和 Office 预览列上方那条顶栏的内容：种类、扩展名和页数。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct DocumentBar {
    kind: String,
    extension: String,
    pages: Option<String>,
}

/// 预览列上方的种类、扩展名和页数。页数用已经算出的总页数，写成「N 页 PDF」。三样都没有时没有顶栏。
pub(super) fn document_bar(model: &ShellViewModel, kind: &str) -> Option<DocumentBar> {
    let kind = kind.trim().to_string();
    let extension = document_extension(model);
    let pages = model.inspect.page_nav().map(|(_, total)| ready_page_label(total));
    if kind.is_empty() && extension.is_empty() && pages.is_none() {
        return None;
    }
    Some(DocumentBar { kind, extension, pages })
}

/// 顶栏：一排描边小标签，第一枚是种类，正文色半粗。
pub(super) fn document_toolbar_view(bar: &DocumentBar) -> AnyView {
    let mut chips = Vec::new();
    if !bar.kind.is_empty() {
        chips.push(toolbar_chip(&bar.kind, "inspect-doc-kind", true));
    }
    if !bar.extension.is_empty() {
        chips.push(toolbar_chip(&bar.extension, "inspect-doc-ext", false));
    }
    if let Some(pages) = &bar.pages {
        chips.push(toolbar_chip(pages, "inspect-doc-pages", false));
    }
    let row = super::workbench::with_bottom_divider(
        Stack::row(8.0)
            .align(AlignSpec::Center)
            .padding_xy(0.0, 8.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    );
    widget(row).key("inspect-doc-toolbar").children(chips).into_any()
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

/// 预览右侧事实卡的四行：类型、大小、硬链接（空时不占位）和修改时间。
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct FactsView {
    extension: String,
    size: String,
    hardlink: String,
    modified: String,
}

impl FactsView {
    /// 从文件事实取四行的文字。
    pub(super) fn of(facts: &FileFacts) -> Self {
        Self {
            extension: if facts.extension.trim().is_empty() { "文件".to_string() } else { facts.extension.clone() },
            size: if facts.size_label.trim().is_empty() { "未知".to_string() } else { facts.size_label.clone() },
            hardlink: hardlink_label(facts.hardlink_state.as_deref()).to_string(),
            modified: super::files::local_time::format_or(&facts.modified_at, "未记录"),
        }
    }
}

/// 预览右侧的类型、大小、硬链接和修改时间。标签在上，数值在下，数值按字段绑定。
/// Vue 统计没有「文件」标题，页数在翻页控件里，尺寸只留在标签组后面。
pub(super) fn fact_column(facts: Signal<FactsView>) -> AnyView {
    let read = move |pick: fn(&FactsView) -> String| move || facts.with(|view| pick(view));
    widget(Stack::column(12.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((
            stacked_value("类型", read(|view| view.extension.clone()), "inspect-stat-type", false, || true),
            stacked_value("大小", read(|view| view.size.clone()), "inspect-stat-size", true, || true),
            stacked_value("硬链接", read(|view| view.hardlink.clone()), "inspect-stat-hardlink", true, move || {
                facts.with(|view| !view.hardlink.is_empty())
            }),
            stacked_value("修改时间", read(|view| view.modified.clone()), "inspect-stat-modified", true, || true),
        ))
        .key("inspect-file-facts")
        .into_any()
}

/// Vue `asset-meta__row`：标签在上（11px 粗体弱色、字距 0.4），数值在下（14px 正文色，长值随处折行），
/// 两者间距 6。`divided` 时行顶一条 border-soft 发丝线，线下留 12px；`shown` 为假时不占位。
fn stacked_value(
    label: &str,
    value: impl Fn() -> String + Send + Clone + 'static,
    key: &str,
    divided: bool,
    shown: impl Fn() -> bool + Send + 'static,
) -> AnyView {
    let key = key.to_string();
    let mut column = Stack::column(6.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0));
    if divided {
        let mut style = column.node_style();
        style.border = Some(SemanticColorRole::BorderSoft);
        let layout = std::sync::Arc::make_mut(&mut style.layout);
        layout.border_top_width = Some(1.0);
        layout.padding_top = Some(LengthSpec::Px(12.0));
        column = column.style(style);
    }
    let mut name = Text::new(label).color(SemanticColorRole::Faint).font_size(11.0).font_weight(700).line_height(17.05);
    std::sync::Arc::make_mut(&mut name.style.layout).letter_spacing = Some(0.4);
    let mut text = Text::new(untrack(&value)).color(SemanticColorRole::Text).font_size(14.0).line_height(21.7);
    {
        let layout = std::sync::Arc::make_mut(&mut text.style.layout);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    widget(column)
        .visible(shown)
        .key(key.clone())
        .children((
            widget(name).key(format!("{key}-label")),
            widget(text).prop::<String, fields::text::value>(value).key(format!("{key}-value")),
        ))
        .into_any()
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

/// 资源库扩展的一个区块：作品信息或歌词。元数据变了时整块按新内容重建。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct LibrarySection {
    pub title: &'static str,
    pub key: &'static str,
    pub rows: Vec<LibraryRow>,
}

/// 区块里的一行：标签和值，或者一段正文（歌词）。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum LibraryRow {
    Value { key: String, label: &'static str, value: String },
    Body(String),
}

/// ASMR 条目的作品信息，有歌词正文时再加一块歌词。不是 ASMR 条目（Vue `matchAsmrEntry` 不认）时没有：
/// Vue 的这一块是 ASMR 扩展的元数据面板，只挂在 ASMR 条目上。歌词状态没有时写「未检测」，不补歌词正文。
/// `custom` 是按 [`super::inspect::support::display_value`] 画成文字的元数据，数组已经连成「甲，乙」。
pub(super) fn library_sections(custom: &BTreeMap<String, String>) -> Vec<LibrarySection> {
    if !matches_asmr(custom) {
        return Vec::new();
    }
    let rows = WORK_ROWS
        .iter()
        .filter_map(|(key, label)| {
            let value = work_value(custom, key)?;
            Some(LibraryRow::Value { key: (*key).to_string(), label, value })
        })
        .collect::<Vec<_>>();
    let mut sections = vec![LibrarySection { title: "作品信息", key: "inspect-asmr-works", rows }];
    if let Some(body) = lyric_body(custom) {
        sections.push(LibrarySection { title: "歌词", key: "inspect-asmr-lyric", rows: vec![LibraryRow::Body(body)] });
    }
    sections
}

/// 一个资源库区块：眉题、标题和各行，放在设置卡片里。
pub(super) fn library_card(section: LibrarySection) -> AnyView {
    let key = section.key;
    let mut body = vec![
        text("ASMR Metadata".to_string()).key(format!("{key}-eyebrow")).into_any(),
        text(section.title.to_string()).key(format!("{key}-title")).into_any(),
    ];
    body.extend(section.rows.into_iter().map(|row| match row {
        LibraryRow::Value { key, label, value } => widget(LabeledValue::new(label, value)).key(format!("inspect-asmr-{key}")).into_any(),
        LibraryRow::Body(body) => text(body).key("inspect-asmr-lyric-body").into_any(),
    }));
    widget(SettingsCard::new(section.title)).children(body).key(key).into_any()
}

/// 和 Vue `matchAsmrEntry` 一样：库类型、作品 ID 或 RJ 号任一存在。
pub(super) fn matches_asmr(custom: &BTreeMap<String, String>) -> bool {
    present(custom, "libraryKind").as_deref() == Some("asmr") || present(custom, "workId").is_some() || present(custom, "rjCode").is_some()
}

/// 作品信息一行的值，照 Vue 元数据面板逐键取：作品 ID 没有时用 RJ 号，标签没有时用 `tags`，销量、评分各有后备键；
/// 歌词状态没有时是「未检测」；时长、价格和计数按 Vue 的格式化，解析不出数字时这行不画，不把原文当成数字。
fn work_value(custom: &BTreeMap<String, String>, key: &str) -> Option<String> {
    let value = match key {
        "workId" => present(custom, "workId").or_else(|| present(custom, "rjCode")),
        "scenarioTags" => present(custom, "scenarioTags").or_else(|| present(custom, "tags")),
        "lyricStatus" => Some(present(custom, "lyricStatus").unwrap_or_else(|| "未检测".into())),
        "listeningStatus" => listening_line(custom),
        "lastListenedAt" => present(custom, key).map(|time| super::files::local_time::format_or(&time, &time)),
        "trackDurationMs" => number(custom, key).and_then(format_duration),
        "price" => number(custom, key).map(|price| format!("{} JPY", format_measure(price))),
        "sales" => number(custom, "sales").or_else(|| number(custom, "dlCount")).map(format_measure),
        "rateAverage" => number(custom, "rateAverage").or_else(|| number(custom, "ratingAverage")).map(format_measure),
        "reviewCount" => number(custom, key).map(format_measure),
        _ => present(custom, key),
    };
    value.filter(|value| !value.is_empty())
}

/// Vue `toLocaleString("zh-CN")`：整数部分按千分位分组，小数最多三位、去掉末尾的 0。
fn format_measure(number: f64) -> String {
    let rounded = (number * 1000.0).round() / 1000.0;
    let fixed = format!("{:.3}", rounded.abs());
    let (whole, fraction) = fixed.split_once('.').unwrap_or((fixed.as_str(), ""));
    let mut grouped = String::new();
    for (index, ch) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let fraction = fraction.trim_end_matches('0');
    let sign = if rounded < 0.0 { "-" } else { "" };
    if fraction.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}.{fraction}") }
}

/// 数字字段：数字或数字文字（Vue `metadataNumber`）。解析不出或不是有限数时记日志、当作没有。
fn number(custom: &BTreeMap<String, String>, key: &str) -> Option<f64> {
    let text = present(custom, key)?;
    match text.parse::<f64>() {
        Ok(number) if number.is_finite() => Some(number),
        _ => {
            eprintln!("Nana 作品字段 {key} 不是数字：{text}");
            None
        }
    }
}

fn lyric_body(custom: &BTreeMap<String, String>) -> Option<String> {
    LYRIC_BODY_KEYS.iter().find_map(|key| present(custom, key))
}

fn present(custom: &BTreeMap<String, String>, key: &str) -> Option<String> {
    custom.get(key).map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

/// 收听状态的标签（Vue `listeningStatusLabel`）：认得的三种翻成中文，别的原样。
pub(super) fn listening_label(status: &str) -> String {
    match status {
        "unlistened" => "未收听".into(),
        "listening" => "收听中".into(),
        "listened" => "已听完".into(),
        other => other.into(),
    }
}

/// 「收听状态」一行：标签，进度大于 0 时再接「· N%」（四舍五入到整数，Vue `formatListeningStatus`）。
fn listening_line(custom: &BTreeMap<String, String>) -> Option<String> {
    let label = present(custom, "listeningStatus").map(|status| listening_label(&status)).unwrap_or_default();
    let progress = number(custom, "listeningProgress").filter(|progress| *progress > 0.0);
    match progress {
        None => (!label.is_empty()).then_some(label),
        Some(progress) if label.is_empty() => Some(format!("{}%", progress.round())),
        Some(progress) => Some(format!("{label} · {}%", progress.round())),
    }
}

/// 毫秒画成「分:秒」。不大于 0 时没有这行。
fn format_duration(ms: f64) -> Option<String> {
    if ms <= 0.0 {
        return None;
    }
    let seconds = (ms / 1000.0).round() as u64;
    Some(format!("{}:{:02}", seconds / 60, seconds % 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asmr(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut custom = BTreeMap::from([("libraryKind".to_string(), "asmr".to_string())]);
        custom.extend(pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())));
        custom
    }

    fn rows(sections: &[LibrarySection]) -> Vec<(String, String)> {
        sections
            .first()
            .map(|section| {
                section
                    .rows
                    .iter()
                    .filter_map(|row| match row {
                        LibraryRow::Value { label, value, .. } => Some((label.to_string(), value.clone())),
                        LibraryRow::Body(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn asmr_match_without_lyric_status_says_undetected() {
        let custom = asmr(&[]);
        assert!(matches_asmr(&custom));
        assert_eq!(rows(&library_sections(&custom)), [("歌词".to_string(), "未检测".to_string())]);
        assert!(lyric_body(&custom).is_none());
        assert_eq!(format_measure(1200.0), "1,200");
        assert_eq!(format_measure(1234.5678), "1,234.568");
        assert_eq!(format_measure(4.5), "4.5");
    }

    /// 作品字段、歌词状态和收听进度都不让非 ASMR 条目长出「作品信息」：Vue 的这一块只挂在 ASMR 条目上。
    #[test]
    fn non_asmr_entries_have_no_work_section() {
        let mut custom = BTreeMap::new();
        custom.insert("lyricStatus".to_string(), "local".to_string());
        custom.insert("workTitle".to_string(), "夜曲".to_string());
        custom.insert("price".to_string(), "1200".to_string());
        custom.insert("lyrics".to_string(), "第一行".to_string());
        assert!(library_sections(&custom).is_empty());
    }

    /// 行的顺序和文案照 Vue 元数据面板；数组连成「甲，乙」以后原样显示，数字字段解析不出时这行不画。
    #[test]
    fn work_rows_follow_vue_order_and_formats() {
        let custom = asmr(&[
            ("rjCode", "RJ01234567"),
            ("workTitle", "夜曲"),
            ("voiceActors", "甲，乙"),
            ("tags", "治愈"),
            ("asmrEntryKind", "audio"),
            ("listeningStatus", "listening"),
            ("listeningProgress", "40.6"),
            ("trackDurationMs", "125000"),
            ("price", "1320"),
            ("dlCount", "2400"),
            ("ratingAverage", "4.56"),
            ("reviewCount", "多"),
        ]);
        let shown = rows(&library_sections(&custom));
        let expected = [
            ("作品 ID", "RJ01234567"),
            ("标题", "夜曲"),
            ("声优", "甲，乙"),
            ("标签", "治愈"),
            ("歌词", "未检测"),
            ("条目类型", "audio"),
            ("收听状态", "收听中 · 41%"),
            ("音轨时长", "2:05"),
            ("价格", "1,320 JPY"),
            ("销量", "2,400"),
            ("评分", "4.56"),
        ]
        .map(|(label, value)| (label.to_string(), value.to_string()));
        assert_eq!(shown, expected);
    }

    #[test]
    fn lyric_body_is_its_own_section_and_progress_formats() {
        let custom = asmr(&[("listeningStatus", "listening"), ("listeningProgress", "40"), ("lyrics", "第一行")]);
        assert_eq!(listening_line(&custom).as_deref(), Some("收听中 · 40%"));
        assert_eq!(library_sections(&custom).len(), 2);
        assert_eq!(format_duration(125000.0).as_deref(), Some("2:05"));
        assert!(format_duration(0.0).is_none());
        assert_eq!(listening_line(&asmr(&[("listeningStatus", "unlistened"), ("listeningProgress", "0")])).as_deref(), Some("未收听"));
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
    fn original_size_formats_like_vue_and_rejects_bad_numbers() {
        assert_eq!(format_original_size(None), "未记录");
        assert_eq!(format_original_size(Some(1536.0)), "1.5 KB");
        assert_eq!(format_original_size(Some(500.0)), "500 B");
        assert_eq!(format_original_size(Some(f64::NAN)), "未记录");
    }
}
