//! 压缩包列表和 ASMR 作品信息。
//!
//! 列表来自已经读出的压缩包条目。目录单击只选中，400 毫秒内再点同一目录才进入下一层，用的仍是这些名字。
//! 文件单击仍打开这一条。行首种类：目录写 DIR，文件用扩展名前四位大写，没有扩展名写 FILE。
//! 点开文件以后名单留在左边，右边舞台用现有预览画这一条。再点另一条，舞台换成那一条。
//! 没打开文件时舞台写提示：没在取文件时写当前目录和「选择内部文件进行预览」；正在取写「准备内部文件」；
//! 不能内联写文件名和「无法内联预览该文件类型」；取出失败写「无法预览内部文件」和原因。
//! 没有条目时工具条仍写「0 项」，列表里只写「空目录」。
//! 作品信息只画元数据里已有的歌词状态和收听进度，没有歌词正文就不造一段歌词。有 http(s) 封面地址时
//! 标题下多一枚「保存封面」（正在保存写「保存中」）。行的顺序和文案按 ASMR 资源库插件的元数据面板；
//! 只有 ASMR 资源库扩展开着（[`asmr_enabled`]）、条目是 ASMR 条目时才有。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::Instant;

use nana_ui::ButtonKind;
use nana_ui::runtime::view::{AnyView, IntoView, Signal, fields, text, untrack, widget};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, EmptyState, JustifySpec, LabeledValue, LengthSpec, List, ListItem,
    ListItemRole, RadiusTier, SemanticColorRole, SettingsCard, Stack, Text,
    TextHorizontalAlignment,
};
use serde_json::Value;

use crate::backend::services::repository::PluginManifest;

use super::inspect::{ArchiveFileNotice, FileFacts, InspectState};
use super::{InspectMessage, ShellMessage, ShellViewModel};

use super::files::hardlink_label;

mod archive;

pub(super) use archive::*;

const ARCHIVE_VIEW: &str = "momobako.preview.archive";
/// 和文件列表 `row_activation` 一样：间隔严格小于 400 毫秒才算连点。
const ARCHIVE_DOUBLE_CLICK_MS: u128 = 400;

thread_local! {
    /// 上一次点过的压缩包目录和时刻，认连点用。
    static LAST_ARCHIVE_DIR: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
}

/// 压缩包目录的一次 Activate。第一次只选中；同一目录、间隔严格小于 400 毫秒的第二次才进入。
pub(super) fn archive_directory_activation(path: &str) -> InspectMessage {
    let open = LAST_ARCHIVE_DIR
        .with(|slot| directory_double_click(&mut slot.borrow_mut(), path, Instant::now()));
    if open {
        InspectMessage::EnterArchive(path.to_string())
    } else {
        InspectMessage::SelectArchive(path.to_string())
    }
}

/// 这次是不是连点。记下这一次的路径和时刻；凑成连点以后也记下，下一次按与这一次的间隔算。
pub(super) fn directory_double_click(
    slot: &mut Option<(String, Instant)>,
    path: &str,
    now: Instant,
) -> bool {
    let open = slot.as_ref().is_some_and(|(previous, at)| {
        previous == path && now.duration_since(*at).as_millis() < ARCHIVE_DOUBLE_CLICK_MS
    });
    *slot = Some((path.to_string(), now));
    open
}

const MODEL_VIEW: &str = "momobako.preview.model";
const PDF_VIEW: &str = "momobako.preview.pdf";
const OFFICE_VIEW: &str = "momobako.preview.office";

/// 作品信息的行：键和标签，按显示顺序排。值由 [`work_value`] 按键取（有的键有后备键），取不到的行不画。
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
/// 扩展属于这个插件，停用插件以后作品信息、补全候选、作品队列和文件列表摘要都不出现。
pub(super) fn asmr_enabled(plugins: &[PluginManifest]) -> bool {
    plugins.iter().any(|plugin| {
        plugin.enabled
            && !matches!(plugin.status.as_str(), "disabled" | "unavailable" | "error")
            && plugin
                .contributes
                .get("libraryExtension")
                .and_then(|extension| extension.get("libraryKind"))
                .and_then(Value::as_str)
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

/// 预览数出页以后的第三项。翻页控件才写「当前 / 总数」。
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

/// 预览列上方的种类、扩展名和第三项。
/// 已经数出页时第三项是「N 页 PDF」。还没有页数时写条目上的大小；
/// 大小是空的写「准备文档」，不编数字。Nana 不跑 office-convert，没有转换器名字可写。
pub(super) fn document_bar(model: &ShellViewModel, kind: &str) -> Option<DocumentBar> {
    let detail = model
        .inspect
        .page_nav()
        .map(|(_, total)| ready_page_label(total))
        .unwrap_or_else(|| pending_document_detail(&model.inspect.facts.size_label));
    document_bar_parts(kind, &document_extension(model), Some(detail))
}

/// 还没有页数时顶栏第三项。空白大小写成「准备文档」。
fn pending_document_detail(size_label: &str) -> String {
    let size = size_label.trim();
    if size.is_empty() {
        "准备文档".to_string()
    } else {
        size.to_string()
    }
}

/// 包内 PDF 的顶栏。扩展名和页数来自这一条，不拿外层压缩包的扩展名。
pub(super) fn document_bar_parts(
    kind: &str,
    extension: &str,
    pages: Option<String>,
) -> Option<DocumentBar> {
    let kind = kind.trim().to_string();
    let extension = extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_uppercase();
    if kind.is_empty() && extension.is_empty() && pages.is_none() {
        return None;
    }
    Some(DocumentBar {
        kind,
        extension,
        pages,
    })
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
    widget(row)
        .key("inspect-doc-toolbar")
        .children(chips)
        .into_any()
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
    let color = if emphasis {
        SemanticColorRole::Text
    } else {
        SemanticColorRole::Muted
    };
    let weight = if emphasis { 600 } else { 400 };
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .padding_xy(8.0, 4.0)
            .surface(SemanticColorRole::Surface)
            .border(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((widget(
        Text::new(label.to_string())
            .color(color)
            .font_size(12.0)
            .font_weight(weight),
    ),))
    .key(key.to_string())
    .into_any()
}

/// 预览右侧事实卡的四行：类型、大小、硬链接（空时不占位）和修改时间。
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct FactsView {
    pub(super) extension: String,
    pub(super) size: String,
    pub(super) hardlink: String,
    pub(super) modified: String,
}

impl FactsView {
    /// 从文件事实取四行的文字。
    pub(super) fn of(facts: &FileFacts) -> Self {
        Self {
            extension: if facts.extension.trim().is_empty() {
                "文件".to_string()
            } else {
                facts.extension.clone()
            },
            size: if facts.size_label.trim().is_empty() {
                "未知".to_string()
            } else {
                facts.size_label.clone()
            },
            hardlink: hardlink_label(facts.hardlink_state.as_deref()).to_string(),
            modified: super::files::local_time::format_or(&facts.modified_at, "未记录"),
        }
    }
}

/// 预览右侧的类型、大小、硬链接和修改时间。标签在上，数值在下，数值按字段绑定。
/// 统计没有「文件」标题，页数在翻页控件里，尺寸只留在标签组后面。
pub(super) fn fact_column(facts: Signal<FactsView>) -> AnyView {
    let read = move |pick: fn(&FactsView) -> String| move || facts.with(|view| pick(view));
    widget(
        Stack::column(12.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children((
        stacked_value(
            "类型",
            read(|view| view.extension.clone()),
            "inspect-stat-type",
            false,
            || true,
        ),
        stacked_value(
            "大小",
            read(|view| view.size.clone()),
            "inspect-stat-size",
            true,
            || true,
        ),
        stacked_value(
            "硬链接",
            read(|view| view.hardlink.clone()),
            "inspect-stat-hardlink",
            true,
            move || facts.with(|view| !view.hardlink.is_empty()),
        ),
        stacked_value(
            "修改时间",
            read(|view| view.modified.clone()),
            "inspect-stat-modified",
            true,
            || true,
        ),
    ))
    .key("inspect-file-facts")
    .into_any()
}

/// 一行统计：标签在上（11px 粗体弱色、字距 0.4），数值在下（14px 正文色，长值随处折行），
/// 两者间距 6。`divided` 时行顶一条 border-soft 发丝线，线下留 12px；`shown` 为假时不占位。
fn stacked_value(
    label: &str,
    value: impl Fn() -> String + Send + Clone + 'static,
    key: &str,
    divided: bool,
    shown: impl Fn() -> bool + Send + 'static,
) -> AnyView {
    let key = key.to_string();
    let mut column = Stack::column(6.0)
        .width(LengthSpec::Fill)
        .min_width(LengthSpec::Px(0.0));
    if divided {
        let mut style = column.node_style();
        style.border = Some(SemanticColorRole::BorderSoft);
        let layout = std::sync::Arc::make_mut(&mut style.layout);
        layout.border_sides.top_width = Some(1.0);
        layout.padding_top = Some(LengthSpec::Px(12.0));
        column = column.style(style);
    }
    let mut name = Text::new(label)
        .color(SemanticColorRole::Faint)
        .font_size(11.0)
        .font_weight(700)
        .line_height(17.05);
    std::sync::Arc::make_mut(&mut name.style.layout)
        .text_ext
        .letter_spacing = Some(0.4);
    let mut text = Text::new(untrack(&value))
        .color(SemanticColorRole::Text)
        .font_size(14.0)
        .line_height(21.7);
    {
        let layout = std::sync::Arc::make_mut(&mut text.style.layout);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.text_ext.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    widget(column)
        .visible(shown)
        .key(key.clone())
        .children((
            widget(name).key(format!("{key}-label")),
            widget(text)
                .prop::<String, fields::text::value>(value)
                .key(format!("{key}-value")),
        ))
        .into_any()
}

/// 1024 以下写 B，其余换成 KB、MB、GB、TB，10 以上取整、以下留一位小数。无效数字记日志并写「未记录」。
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
    let text = if size >= 10.0 {
        format!("{size:.0}")
    } else {
        format!("{size:.1}")
    };
    format!("{text} {}", units[index])
}

fn plain_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

/// 资源库扩展的一个区块：作品信息或歌词。元数据变了时整块按新内容重建。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct LibrarySection {
    pub title: &'static str,
    pub key: &'static str,
    pub rows: Vec<LibraryRow>,
    /// 作品信息上的「保存封面」。没有 http(s) 地址时不画。
    pub cover: Option<CoverButton>,
}

/// 作品信息上的封面按钮。`label` 是「保存封面」或「保存中」。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct CoverButton {
    pub url: String,
    pub label: String,
    pub disabled: bool,
}

/// 区块里的一行：标签和值，或者一段正文（歌词）。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum LibraryRow {
    Value {
        key: String,
        label: &'static str,
        value: String,
    },
    Body(String),
}

/// ASMR 条目的作品信息，有歌词正文时再加一块歌词。不是 ASMR 条目时没有：
/// 这一块是 ASMR 扩展的元数据面板，只挂在 ASMR 条目上。歌词状态没有时写「未检测」，不补歌词正文。
/// `custom` 是按 [`super::inspect::support::display_value`] 画成文字的元数据，数组已经连成「甲，乙」。
pub(super) fn library_sections(custom: &BTreeMap<String, String>) -> Vec<LibrarySection> {
    if !matches_asmr(custom) {
        return Vec::new();
    }
    let rows = WORK_ROWS
        .iter()
        .filter_map(|(key, label)| {
            let value = work_value(custom, key)?;
            Some(LibraryRow::Value {
                key: (*key).to_string(),
                label,
                value,
            })
        })
        .collect::<Vec<_>>();
    let mut sections = vec![LibrarySection {
        title: "作品信息",
        key: "inspect-asmr-works",
        rows,
        cover: None,
    }];
    if let Some(body) = lyric_body(custom) {
        sections.push(LibrarySection {
            title: "歌词",
            key: "inspect-asmr-lyric",
            rows: vec![LibraryRow::Body(body)],
            cover: None,
        });
    }
    sections
}

/// 一个资源库区块：眉题、标题和各行，放在设置卡片里。
pub(super) fn library_card(section: LibrarySection) -> AnyView {
    let key = section.key;
    let mut body = vec![
        text("ASMR Metadata".to_string())
            .key(format!("{key}-eyebrow"))
            .into_any(),
        text(section.title.to_string())
            .key(format!("{key}-title"))
            .into_any(),
    ];
    if let Some(cover) = section.cover {
        body.push(cover_button(cover));
    }
    body.extend(section.rows.into_iter().map(|row| {
        match row {
            LibraryRow::Value { key, label, value } => widget(LabeledValue::new(label, value))
                .key(format!("inspect-asmr-{key}"))
                .into_any(),
            LibraryRow::Body(body) => text(body).key("inspect-asmr-lyric-body").into_any(),
        }
    }));
    widget(SettingsCard::new(section.title))
        .children(body)
        .key(key)
        .into_any()
}

/// 「保存封面」/「保存中」。停用时不发消息；归约里还会再拦一次。
fn cover_button(cover: CoverButton) -> AnyView {
    let url = cover.url;
    let disabled = cover.disabled;
    widget(
        Button::new(cover.label)
            .kind(ButtonKind::Ghost)
            .disabled(disabled),
    )
    .key("inspect-asmr-save-cover")
    .on_cx(move |_, _: &Activate, cx| {
        if disabled {
            return;
        }
        cx.dispatch_program_all(ShellMessage::Asmr(super::AsmrMessage::SaveCover(
            url.clone(),
        )));
    })
    .into_any()
}

/// 是不是 ASMR 条目：库类型、作品 ID 或 RJ 号任一存在。
pub(super) fn matches_asmr(custom: &BTreeMap<String, String>) -> bool {
    present(custom, "libraryKind").as_deref() == Some("asmr")
        || present(custom, "workId").is_some()
        || present(custom, "rjCode").is_some()
}

/// 作品信息一行的值，逐键取：作品 ID 没有时用 RJ 号，标签没有时用 `tags`，销量、评分各有后备键；
/// 歌词状态没有时是「未检测」；时长、价格和计数各按自己的格式写，解析不出数字时这行不画，不把原文当成数字。
fn work_value(custom: &BTreeMap<String, String>, key: &str) -> Option<String> {
    let value = match key {
        "workId" => present(custom, "workId").or_else(|| present(custom, "rjCode")),
        "scenarioTags" => present(custom, "scenarioTags").or_else(|| present(custom, "tags")),
        "lyricStatus" => Some(present(custom, "lyricStatus").unwrap_or_else(|| "未检测".into())),
        "listeningStatus" => listening_line(custom),
        "lastListenedAt" => {
            present(custom, key).map(|time| super::files::local_time::format_or(&time, &time))
        }
        "trackDurationMs" => number(custom, key).and_then(format_duration),
        "price" => number(custom, key).map(|price| format!("{} JPY", format_measure(price))),
        "sales" => number(custom, "sales")
            .or_else(|| number(custom, "dlCount"))
            .map(format_measure),
        "rateAverage" => number(custom, "rateAverage")
            .or_else(|| number(custom, "ratingAverage"))
            .map(format_measure),
        "reviewCount" => number(custom, key).map(format_measure),
        _ => present(custom, key),
    };
    value.filter(|value| !value.is_empty())
}

/// zh-CN 数字写法：整数部分按千分位分组，小数最多三位、去掉末尾的 0。
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
    if fraction.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}.{fraction}")
    }
}

/// 数字字段：数字或数字文字。解析不出或不是有限数时记日志、当作没有。
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
    custom
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 文件列表上一行的 ASMR 摘要：RJ 号（没有时是作品 ID）、作品名（没有时是标题）、
/// 收听状态、进度、有歌词状态时「歌词」，用「 · 」连起来。不是 ASMR 条目时是空串；扩展插件开没开由调用方看。
///
/// 标题和文件名一样时不算作品名：后端给每个素材种下的 `title` 就是文件名，卡片上已经有了，不再重复一遍。
pub(super) fn file_summary(row: &super::files::FileRow) -> String {
    if row.kind == "directory" || !super::inspect_shortcuts::matches_entry("asmr", &row.metadata) {
        return String::new();
    }
    let text = |key: &str| match row.metadata.get(key) {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    };
    let first = |keys: &[&str]| {
        keys.iter()
            .map(|key| text(key))
            .find(|value| !value.is_empty())
            .unwrap_or_default()
    };
    let title = Some(text("title"))
        .filter(|title| *title != row.name)
        .unwrap_or_default();
    let work = Some(text("workTitle"))
        .filter(|work| !work.is_empty())
        .unwrap_or(title);
    let progress = text("listeningProgress")
        .parse::<f64>()
        .ok()
        .filter(|progress| progress.is_finite())
        .map(|progress| format!("{}%", progress.round()));
    let lyric = if text("lyricStatus").is_empty() {
        String::new()
    } else {
        "歌词".to_string()
    };
    [
        first(&["rjCode", "workId"]),
        work,
        listening_label(&text("listeningStatus")),
        progress.unwrap_or_default(),
        lyric,
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ")
}

/// 收听状态的标签：认得的三种翻成中文，别的原样。
pub(super) fn listening_label(status: &str) -> String {
    match status {
        "unlistened" => "未收听".into(),
        "listening" => "收听中".into(),
        "listened" => "已听完".into(),
        other => other.into(),
    }
}

/// 「收听状态」一行：标签，进度大于 0 时再接「· N%」（四舍五入到整数）。
fn listening_line(custom: &BTreeMap<String, String>) -> Option<String> {
    let label = present(custom, "listeningStatus")
        .map(|status| listening_label(&status))
        .unwrap_or_default();
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
mod tests;
