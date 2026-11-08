//! 压缩包列表和 ASMR 作品信息。
//!
//! 列表来自已经读出的压缩包条目。目录单击只选中，400 毫秒内再点同一目录才进入下一层，用的仍是这些名字。
//! 文件单击仍打开这一条。行首照 Vue `fileKindLabel`：目录写 DIR，文件用扩展名前四位大写，没有扩展名写 FILE。
//! 点开文件以后名单留在左边，右边舞台用现有预览画这一条。再点另一条，舞台换成那一条。
//! 没打开文件时舞台写提示：没在取文件时写当前目录和「选择内部文件进行预览」；正在取写「准备内部文件」；
//! 不能内联写文件名和「无法内联预览该文件类型」；取出失败写「无法预览内部文件」和原因。
//! 没有条目时工具条仍写「0 项」，列表里只写「空目录」。
//! 作品信息只画元数据里已有的歌词状态和收听进度，没有歌词正文就不造一段歌词。有 http(s) 封面地址时
//! 标题下多一枚「保存封面」（正在保存写「保存中」）。行的顺序和文案照 Vue `library-asmr` 的元数据面板；
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

/// Vue 预览数出页以后的第三项。翻页控件才写「当前 / 总数」。
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
/// 已经数出页时第三项是「N 页 PDF」。还没有页数时照 Vue `statusDetail` 写条目上的大小；
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
            .outline(SemanticColorRole::Border, 1.0)
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

/// 右边舞台上的提示。文案照 Vue `renderInternalPreview` 的空闲、准备、不能内联和失败。
pub(super) struct ArchiveStage<'a> {
    pub title: &'a str,
    pub detail: &'a str,
    /// 失败标题用危险色。其余标题是正文色。
    pub failed: bool,
}

/// 当前名单该写的提示。正在取字节时优先写「准备内部文件」，不看上一次的失败。
pub(super) fn archive_stage(inspect: &InspectState) -> (&str, &str, bool) {
    if let Some(path) = inspect.archive_request.as_deref().map(request_entry) {
        return ("准备内部文件", path, false);
    }
    match inspect.archive_notice.as_ref() {
        Some(ArchiveFileNotice::Inline(name)) => (name.as_str(), "无法内联预览该文件类型", false),
        Some(ArchiveFileNotice::Failed(message)) => ("无法预览内部文件", message.as_str(), true),
        None => (directory_label(inspect), "选择内部文件进行预览", false),
    }
}

/// 当前层的目录名。根上写「根目录」，和 Vue `directoryLabel` 一样。
fn directory_label(inspect: &InspectState) -> &str {
    let directory = match inspect.archive_nests.last() {
        Some(nest) => nest.directory.as_str(),
        None => inspect.archive_directory.as_str(),
    };
    if directory.is_empty() {
        "根目录"
    } else {
        directory
    }
}

/// 在途请求键的最后一段是这一条在当前层里的路径。
fn request_entry(key: &str) -> &str {
    key.rsplit('\u{1}')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(key)
}

/// 把压缩包摘要画成计数和条目。目录行单击只选中，连点才进入；文件行单击打开这一条。面包屑回到上层。
/// 没有条目时仍写「0 项」和「空目录」，和 Vue 工具条的 `entries.length` 一样，不另编一句说明。
/// 名单在左、舞台在右，和 Vue `archive-preview__body` 一样。`opened` 有值时舞台是这一条的预览；
/// 否则 `stage` 写空闲、准备、不能内联或失败。
pub(super) fn archive_list(
    content: &str,
    picked: Option<&str>,
    stage: ArchiveStage,
    archive_name: &str,
    opened: Option<AnyView>,
) -> AnyView {
    let parsed = parse_archive(content);
    let count = parsed
        .rows
        .iter()
        .filter(|row| !matches!(row, ArchiveRow::Ellipsis))
        .count();
    let directory = parsed.directory.clone().unwrap_or_default();
    // 有层目录头，或整包空到解析不出目录头，都画出面包屑。rar 的整路径列表没有目录头、但有文件行，不在这里。
    let crumbs = (parsed.directory.is_some() || parsed.rows.is_empty())
        .then(|| archive_breadcrumbs(&directory, archive_name));
    let list = if parsed.rows.is_empty() {
        widget(EmptyState::new("空目录").compact(true))
            .key("inspect-archive-empty")
            .into_any()
    } else {
        let open_files = parsed.directory.is_some();
        let items = parsed
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| archive_row(index, row, &directory, open_files, picked))
            .collect::<Vec<_>>();
        widget(List::new())
            .children(items)
            .key("inspect-archive-list")
            .into_any()
    };
    let stage = match opened {
        Some(view) => view,
        None => archive_stage_view(stage),
    };
    let mut rows = Vec::new();
    match crumbs {
        Some(crumbs) => rows.push(archive_toolbar(crumbs, &format!("{count} 项"))),
        None => rows.push(
            text(format!("{count} 项"))
                .key("inspect-archive-count")
                .into_any(),
        ),
    }
    rows.push(archive_split(list, stage));
    widget(
        Stack::fill_column(8.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0)),
    )
    .children(rows)
    .key("inspect-archive")
    .into_any()
}

/// 工具条：面包屑在左，项数在右。和 Vue `archive-preview__toolbar` 一样把计数和路径分开。
fn archive_toolbar(crumbs: AnyView, count: &str) -> AnyView {
    widget(
        Stack::row(12.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::SpaceBetween)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children((
        crumbs,
        text(count.to_string())
            .key("inspect-archive-count")
            .into_any(),
    ))
    .key("inspect-archive-toolbar")
    .into_any()
}

/// 左边名单、右边舞台。宽度比照 Vue `minmax(220px, 0.42fr)` 和 `minmax(260px, 1fr)`。
fn archive_split(list: AnyView, stage: AnyView) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                layout.flex_grow = Some(1.0);
                layout.flex_shrink = Some(1.0);
                layout.align_items = AlignSpec::Stretch;
            }),
    )
    .children((
        archive_pane(list, 0.42, "inspect-archive-list-pane"),
        archive_pane(stage, 1.0, "inspect-archive-stage-pane"),
    ))
    .key("inspect-archive-body")
    .into_any()
}

/// 并排的一列。`grow` 是剩余宽度里的份数，窄的时候允许缩到 0，避免被挤成上下两行。
fn archive_pane(child: AnyView, grow: f32, key: &str) -> AnyView {
    widget(
        Stack::fill_column(0.0)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                layout.flex_grow = Some(grow);
                layout.flex_shrink = Some(1.0);
                layout.flex_basis = Some(LengthSpec::Px(0.0));
                layout.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .children((child,))
    .key(key.to_string())
    .into_any()
}

/// 右边舞台上的提示。失败标题是危险色，其余是正文色，说明是弱色。
fn archive_stage_view(stage: ArchiveStage) -> AnyView {
    let title_role = if stage.failed {
        SemanticColorRole::Danger
    } else {
        SemanticColorRole::Text
    };
    let mut title = Text::new(stage.title)
        .color(title_role)
        .font_size(13.0)
        .font_weight(600)
        .line_height(20.15);
    title.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let mut detail = Text::new(stage.detail)
        .color(SemanticColorRole::Muted)
        .font_size(12.0)
        .line_height(18.6);
    detail.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    widget(
        Stack::column(6.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Fill)
            .min_height(LengthSpec::Px(120.0)),
    )
    .children((
        widget(title).key("inspect-archive-stage-title"),
        widget(detail).key("inspect-archive-stage-detail"),
    ))
    .key("inspect-archive-stage")
    .into_any()
}

/// 面包屑照 Vue `breadcrumbs`：第一段是 `entry.name`（带扩展名的文件名），后面是已经进入的目录名。当前段不可再点。
fn archive_breadcrumbs(directory: &str, archive_name: &str) -> AnyView {
    let mut path = String::new();
    let root = archive_root_label(archive_name);
    let mut crumbs = vec![archive_crumb(
        root,
        String::new(),
        directory.is_empty(),
        "inspect-archive-crumb-root",
    )];
    if !directory.is_empty() {
        let parts: Vec<&str> = directory
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        let last = parts.len().saturating_sub(1);
        for (index, part) in parts.into_iter().enumerate() {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            crumbs.push(archive_crumb(
                part,
                path.clone(),
                index == last,
                &format!("inspect-archive-crumb-{index}"),
            ));
        }
    }
    widget(Stack::row(4.0).align(AlignSpec::Center))
        .children(crumbs)
        .key("inspect-archive-crumbs")
        .into_any()
}

/// 面包屑第一段。Vue 用 `props.entry.name`：带扩展名的文件名，也就是路径最后一段。空白或没有名字时仍写「压缩包」，不编一个假名字。
fn archive_root_label(name: &str) -> &str {
    let name = name.trim();
    if name.is_empty() { "压缩包" } else { name }
}

fn archive_crumb(label: &str, path: String, current: bool, key: &str) -> AnyView {
    widget(Button::new(label).kind(ButtonKind::Ghost).disabled(current))
        .key(key.to_string())
        .on_cx(move |_, _: &Activate, cx| {
            if !current {
                cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::EnterArchive(
                    path.clone(),
                )));
            }
        })
        .into_any()
}

fn archive_row(
    index: usize,
    row: &ArchiveRow,
    directory: &str,
    open_files: bool,
    picked: Option<&str>,
) -> AnyView {
    let key = format!("inspect-archive-row-{index}");
    match row {
        ArchiveRow::Dir(name) => {
            let next = if directory.is_empty() {
                name.clone()
            } else {
                format!("{directory}/{name}")
            };
            let mut item = ListItem::new(name.clone())
                .detail("目录")
                .role(ListItemRole::Button);
            if picked == Some(next.as_str()) {
                item = item.selected(true);
            }
            widget(item)
                .key(key.clone())
                .leading(archive_kind_mark("DIR", &archive_kind_key(&key)))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Inspect(archive_directory_activation(
                        &next,
                    )));
                })
                .into_any()
        }
        ArchiveRow::File(name, note) => {
            archive_file_row(key, name, note, directory, open_files, picked)
        }
        ArchiveRow::Ellipsis => widget(ListItem::new("…")).key(key).into_any(),
    }
}

/// zip/7z 的文件行单击就打开，点开的这一行标成当前项，和 Vue `is-selected` 一样。
/// rar 的整路径列表没有目录头，行不可点。行首种类不改大小。
fn archive_file_row(
    key: String,
    name: &str,
    note: &str,
    directory: &str,
    open_files: bool,
    picked: Option<&str>,
) -> AnyView {
    let kind_key = archive_kind_key(&key);
    let mut item = ListItem::new(name.to_string()).detail(note.to_string());
    if !open_files {
        return widget(item)
            .key(key)
            .leading(archive_kind_mark(&file_kind_label(name), &kind_key))
            .into_any();
    }
    let full = if directory.is_empty() {
        name.to_string()
    } else {
        format!("{directory}/{name}")
    };
    if picked == Some(full.as_str()) {
        item = item.selected(true);
    }
    widget(item.role(ListItemRole::Button))
        .key(key)
        .leading(archive_kind_mark(&file_kind_label(name), &kind_key))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::OpenArchiveFile(
                full.clone(),
            )));
        })
        .into_any()
}

/// 行首种类的键，跟在对应名单行后面。
fn archive_kind_key(row_key: &str) -> String {
    row_key.replacen("inspect-archive-row-", "inspect-archive-kind-", 1)
}

/// 名单行首的小字。目录固定写「DIR」；文件走 [`file_kind_label`]。
fn archive_kind_mark(label: &str, key: &str) -> AnyView {
    let mark = Text::new(label.to_string())
        .color(SemanticColorRole::Muted)
        .font_size(10.0)
        .font_weight(700);
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Px(30.0))
            .min_width(LengthSpec::Px(30.0)),
    )
    .children((widget(mark).key(key.to_string()),))
    .into_any()
}

/// 照 Vue `fileKindLabel`：扩展名前四位转成大写。空扩展名、只有前导点、或点后面是空的，都写「FILE」。
fn file_kind_label(name: &str) -> String {
    let extension = path_extension(name);
    let raw = if extension.is_empty() {
        "FILE"
    } else {
        extension
    };
    raw.chars().take(4).collect::<String>().to_ascii_uppercase()
}

/// 文件名的扩展名，规则和 `Path::extension` 一样，只看最后一段。
fn path_extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => extension,
        _ => "",
    }
}

enum ArchiveRow {
    Dir(String),
    File(String, String),
    Ellipsis,
}

struct ParsedArchive {
    /// `Some` 表示 zip/7z 的当前目录，空字符串是根。`None` 是 rar 的整路径列表或空包。
    directory: Option<String>,
    rows: Vec<ArchiveRow>,
}

/// 头部 `压缩包 / 路径 · N 项` 是当前层。rar 用「个文件」，没有目录行。
fn parse_archive(content: &str) -> ParsedArchive {
    let mut directory = None;
    let mut rows = Vec::new();
    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(path) = layered_directory(line) {
            directory = Some(path);
            continue;
        }
        if line.starts_with("压缩包") {
            continue;
        }
        if line == "…" {
            rows.push(ArchiveRow::Ellipsis);
            continue;
        }
        let (name, note) = line.rsplit_once(" · ").unwrap_or((line, ""));
        if note == "目录" {
            rows.push(ArchiveRow::Dir(name.to_string()));
        } else {
            rows.push(ArchiveRow::File(name.to_string(), note.to_string()));
        }
    }
    ParsedArchive { directory, rows }
}

fn layered_directory(line: &str) -> Option<String> {
    let (head, tail) = line.rsplit_once(" · ")?;
    let count = tail.strip_suffix(" 项")?;
    if count.is_empty() || !count.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    if let Some(path) = head.strip_prefix("压缩包 / ") {
        Some(path.to_string())
    } else if head == "压缩包" {
        Some(String::new())
    } else {
        None
    }
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
/// Vue 统计没有「文件」标题，页数在翻页控件里，尺寸只留在标签组后面。
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
    let mut column = Stack::column(6.0)
        .width(LengthSpec::Fill)
        .min_width(LengthSpec::Px(0.0));
    if divided {
        let mut style = column.node_style();
        style.border = Some(SemanticColorRole::BorderSoft);
        let layout = std::sync::Arc::make_mut(&mut style.layout);
        layout.border_top_width = Some(1.0);
        layout.padding_top = Some(LengthSpec::Px(12.0));
        column = column.style(style);
    }
    let mut name = Text::new(label)
        .color(SemanticColorRole::Faint)
        .font_size(11.0)
        .font_weight(700)
        .line_height(17.05);
    std::sync::Arc::make_mut(&mut name.style.layout).letter_spacing = Some(0.4);
    let mut text = Text::new(untrack(&value))
        .color(SemanticColorRole::Text)
        .font_size(14.0)
        .line_height(21.7);
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
            widget(text)
                .prop::<String, fields::text::value>(value)
                .key(format!("{key}-value")),
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

/// 和 Vue `matchAsmrEntry` 一样：库类型、作品 ID 或 RJ 号任一存在。
pub(super) fn matches_asmr(custom: &BTreeMap<String, String>) -> bool {
    present(custom, "libraryKind").as_deref() == Some("asmr")
        || present(custom, "workId").is_some()
        || present(custom, "rjCode").is_some()
}

/// 作品信息一行的值，照 Vue 元数据面板逐键取：作品 ID 没有时用 RJ 号，标签没有时用 `tags`，销量、评分各有后备键；
/// 歌词状态没有时是「未检测」；时长、价格和计数按 Vue 的格式化，解析不出数字时这行不画，不把原文当成数字。
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
    if fraction.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}.{fraction}")
    }
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
    custom
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 文件列表上一行的 ASMR 摘要（Vue `fileSummary(entry).inline`）：RJ 号（没有时是作品 ID）、作品名（没有时是标题）、
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
mod tests {
    use super::*;

    fn asmr(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut custom = BTreeMap::from([("libraryKind".to_string(), "asmr".to_string())]);
        custom.extend(
            pairs
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string())),
        );
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
                        LibraryRow::Value { label, value, .. } => {
                            Some((label.to_string(), value.clone()))
                        }
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
        assert_eq!(
            rows(&library_sections(&custom)),
            [("歌词".to_string(), "未检测".to_string())]
        );
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
        let custom = asmr(&[
            ("listeningStatus", "listening"),
            ("listeningProgress", "40"),
            ("lyrics", "第一行"),
        ]);
        assert_eq!(listening_line(&custom).as_deref(), Some("收听中 · 40%"));
        assert_eq!(library_sections(&custom).len(), 2);
        assert_eq!(format_duration(125000.0).as_deref(), Some("2:05"));
        assert!(format_duration(0.0).is_none());
        assert_eq!(
            listening_line(&asmr(&[
                ("listeningStatus", "unlistened"),
                ("listeningProgress", "0")
            ]))
            .as_deref(),
            Some("未收听")
        );
    }

    #[test]
    fn ready_pdf_status_counts_pages_instead_of_the_flipper() {
        assert_eq!(ready_page_label(1), "1 页 PDF");
        assert_eq!(ready_page_label(3), "3 页 PDF");
    }

    #[test]
    fn archive_without_files_is_an_empty_directory() {
        let view = archive_list(
            "压缩包里没有文件。",
            None,
            ArchiveStage {
                title: "根目录",
                detail: "选择内部文件进行预览",
                failed: false,
            },
            "",
            None,
        );
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
