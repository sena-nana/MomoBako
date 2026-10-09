//! 文件列表右侧的详情卡片，对应 Vue `FileBrowserPanel.vue` 的 `files-detail`。
//!
//! 多选时写「N 个项目」和文件夹 / 文件数；单选时是缩略图、色板、标题、路径、注释和链接、
//! 类型 / 大小 / 硬链接 / 受保护 / 修改时间各行，文件再接元数据编辑；没有选择时是当前目录的
//! 直属计数；回收站和分类视图没有选择时是居中的提示。卡片自己纵向滚动，播放条不会盖住它。
//!
//! 常驻：外框只建一次，宽度和圆角按投影绑定。里面按「显示什么」（[`DetailKey`]）换一整块：
//! 换了选中项、选中数或卡片种类时回到顶部；同一项里的文字、缩略图和可有可无的行按字段绑定，
//! 元数据编辑区常驻，输入框不因为别的更新重建。

use nana_ui::runtime::view::fields::{self, HiddenWhenEmpty};
use nana_ui::runtime::view::{dynamic, signal, untrack, widget, AnyView, IntoView, Signal};
use nana_ui::runtime::{JustifySpec, LengthSpec, NodeStyle, OverflowSpec, RadiusTier, ScrollAxes, ScrollView, SemanticColorRole, Stack};

use super::super::files::{hardlink_label, local_time, FileContext, FileRow};
use super::super::inspect_metadata_view::{self, MetadataSignals};
use super::super::ShellViewModel;
use super::bind::{fact_row, fill_container, StyleField};
use super::cards::{preview_box, PreviewLook};
use super::style;

/// 详情里现在显示哪一种卡片。和滚动区的键一起决定要不要换一整块。
#[derive(Clone, Debug, PartialEq)]
enum DetailKind {
    /// 选中了不止一项。
    Multi,
    /// 单个条目。文件（不在回收站里）才接元数据编辑，回收站多一行删除时间。
    Entry { key: String, metadata: bool, trash: bool },
    /// 没有选择：当前目录的直属计数。
    Directory,
    /// 回收站或分类视图里还没有选择。
    Empty { smart: bool, trash: bool },
    /// 预览页打开着，工作台藏着：详情不建，元数据编辑区只在预览页里有一份。
    Idle,
}

/// 详情的整块身份：卡片种类，加上滚动区的键（按显示的条目取）。
#[derive(Clone, Debug, PartialEq)]
struct DetailKey {
    kind: DetailKind,
    scroll: String,
}

/// 单个条目卡片的文字。空字符串的行不占位。
#[derive(Clone, Debug, Default, PartialEq)]
struct EntryView {
    title: String,
    /// 和文件名不同时的完整路径。
    subline: String,
    comment: String,
    link: String,
    type_label: String,
    size: String,
    hardlink: String,
    /// 受保护文件夹的提示。
    protected: String,
    modified: String,
    deleted: String,
    /// 色板颜色，原样保留：建色块时跳过不合法的并记日志，一个合法的都没有时整行不占位。
    palette: Vec<String>,
}

/// 多选和目录卡片的标题、路径和三个计数。
#[derive(Clone, Debug, Default, PartialEq)]
struct SummaryView {
    title: String,
    subline: String,
    counts: [String; 3],
}

/// 外框的宽度排法和圆角。
#[derive(Clone, Copy, Debug, PartialEq)]
struct FrameLook {
    /// 窄窗口里详情排到文件卡片下面，整行宽。
    stacked: bool,
    radius: f32,
}

/// 详情的常驻信号。元数据编辑区的信号在 [`MetadataSignals`]，和预览页共用。
#[derive(Clone, Copy)]
pub(super) struct DetailSignals {
    frame: Signal<FrameLook>,
    key: Signal<DetailKey>,
    entry: Signal<EntryView>,
    preview: Signal<PreviewLook>,
    summary: Signal<SummaryView>,
}

impl DetailSignals {
    /// 在常驻作用域里建信号，初值是这一刻的投影。
    pub(super) fn new(model: &ShellViewModel) -> Self {
        let projected = Projection::of(model);
        Self {
            frame: signal(projected.frame),
            key: signal(projected.key),
            entry: signal(projected.entry),
            preview: signal(projected.preview),
            summary: signal(projected.summary),
        }
    }

    /// 写入投影，只写变了的。
    pub(super) fn write(&self, model: &ShellViewModel) {
        let projected = Projection::of(model);
        self.frame.try_set_if_changed(projected.frame);
        self.entry.try_set_if_changed(projected.entry);
        self.preview.try_set_if_changed(projected.preview);
        self.summary.try_set_if_changed(projected.summary);
        self.key.try_set_if_changed(projected.key);
    }
}

/// 详情的全部投影。只投影显示的那一种卡片，其余保持默认值。
struct Projection {
    frame: FrameLook,
    key: DetailKey,
    entry: EntryView,
    preview: PreviewLook,
    summary: SummaryView,
}

impl Projection {
    fn of(model: &ShellViewModel) -> Self {
        let ctx = FileContext::from_model(model);
        let files = &model.files;
        let rows: Vec<&FileRow> = if ctx.smart_folder {
            files.virtual_rows.iter().collect()
        } else if ctx.category_virtual {
            files.rows.iter().filter(|row| row.kind != "directory").collect()
        } else {
            files.rows.iter().collect()
        };
        let chosen = rows
            .iter()
            .copied()
            .filter(|row| files.selected.iter().any(|item| item == &row.path) || files.primary.as_deref() == Some(row.path.as_str()))
            .collect::<Vec<_>>();
        let frame = FrameLook { stacked: model.viewport_width <= super::STACKED_VIEWPORT_PX, radius: super::card_radius(model) };
        let mut projected = Self {
            frame,
            key: DetailKey { kind: DetailKind::Idle, scroll: String::new() },
            entry: EntryView::default(),
            preview: PreviewLook::default(),
            summary: SummaryView::default(),
        };
        if super::previewing(model) {
            return projected;
        }
        if chosen.len() > 1 {
            let folders = chosen.iter().filter(|row| row.kind == "directory").count();
            let count = chosen.len() - folders;
            let subline = [(folders > 0).then(|| format!("{folders} 个文件夹")), (count > 0).then(|| format!("{count} 个文件"))]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            projected.summary = SummaryView { title: format!("{} 个项目", chosen.len()), subline, ..SummaryView::default() };
            projected.key = DetailKey { kind: DetailKind::Multi, scroll: format!("multi-{}", chosen.len()) };
            return projected;
        }
        let current = files
            .primary
            .as_ref()
            .and_then(|primary| rows.iter().copied().find(|row| &row.path == primary))
            .or_else(|| chosen.first().copied());
        if let Some(row) = current {
            let metadata = row.kind != "directory" && !ctx.trash;
            projected.entry = entry_view(model, &ctx, row);
            projected.preview = PreviewLook::of(row, super::cards::builtin_bindings());
            projected.key = DetailKey {
                kind: DetailKind::Entry { key: row.key(), metadata, trash: ctx.trash },
                scroll: format!("entry-{}", row.key()),
            };
            return projected;
        }
        if !ctx.trash && !ctx.is_virtual() {
            projected.summary = directory_view(model, &rows);
            projected.key = DetailKey { kind: DetailKind::Directory, scroll: format!("directory-{}", files.current_path) };
            return projected;
        }
        projected.key = DetailKey { kind: DetailKind::Empty { smart: ctx.smart_folder, trash: ctx.trash }, scroll: "empty".into() };
        projected
    }
}

/// 单个条目：标题、路径、注释和链接、事实行和色板。
fn entry_view(model: &ShellViewModel, ctx: &FileContext, row: &FileRow) -> EntryView {
    let entry = model.browser_entries.iter().find(|entry| entry.path == row.path && entry.kind == row.kind);
    let text = |key: &str| row.metadata.get(key).and_then(|value| value.as_str()).map(str::trim).filter(|value| !value.is_empty());
    let palette = if row.palette.is_empty() && model.inspect.target_path.as_deref() == Some(row.path.as_str()) {
        model.inspect.palette.clone()
    } else {
        row.palette.clone()
    };
    let protected = entry
        .and_then(|entry| entry.folder_metadata.as_ref())
        .filter(|folder| folder.protected)
        .map(|folder| folder.password_tip.clone().filter(|tip| !tip.trim().is_empty()).unwrap_or_else(|| "Eagle 迁移提示".into()))
        .unwrap_or_default();
    let deleted = if ctx.trash {
        local_time::format_or(row.metadata.get("deletedAt").and_then(|value| value.as_str()).unwrap_or(""), "未记录")
    } else {
        String::new()
    };
    EntryView {
        title: row.display_title(),
        subline: if row.path != row.name && !row.path.is_empty() { row.path.clone() } else { String::new() },
        comment: text("comment").or_else(|| text("note")).unwrap_or_default().to_string(),
        link: text("link").unwrap_or_default().to_string(),
        type_label: type_label(row),
        size: if row.size_label.trim().is_empty() { "目录项".into() } else { row.size_label.clone() },
        hardlink: hardlink_label(row.hardlink_state.as_deref()).to_string(),
        protected,
        modified: local_time::format_or(&row.modified_at, "未记录"),
        deleted,
        palette,
    }
}

/// 没有选择：当前目录名（根目录写资源库名）、路径，以及直属计数。
fn directory_view(model: &ShellViewModel, rows: &[&FileRow]) -> SummaryView {
    let folders = rows.iter().filter(|row| row.kind == "directory").count();
    let files = rows.len() - folders;
    let path = model.files.current_path.trim();
    let title = if path.is_empty() {
        let name = model.repository_name.trim();
        if name.is_empty() { "根目录".to_string() } else { name.to_string() }
    } else {
        path.rsplit('/').next().filter(|part| !part.is_empty()).unwrap_or(path).to_string()
    };
    let subline = if path.is_empty() { "根目录".to_string() } else { path.to_string() };
    SummaryView { title, subline, counts: [files.to_string(), folders.to_string(), rows.len().to_string()] }
}

/// 详情卡片。窄窗口里是排在文件卡片下面的整行版式。
pub(super) fn detail_aside(signals: DetailSignals, metadata: MetadataSignals) -> AnyView {
    let frame = signals.frame;
    let initial = frame.get_untracked();
    let body = dynamic(signals.key, move |key: &DetailKey| scroller(key, signals, metadata)).css(fill_container());
    widget(Stack::column(0.0).style(frame_style(initial)))
        .prop::<NodeStyle, StyleField>(move || frame_style(frame.get()))
        .children((body,))
        .key("file-detail")
        .into_any()
}

/// 外框：`--bg-elev` 圆角卡片，裁掉溢出；宽排版定宽 300，窄排版铺满。
fn frame_style(look: FrameLook) -> NodeStyle {
    let frame = Stack::fill_column(0.0)
        .min_height(LengthSpec::Px(0.0))
        .surface(SemanticColorRole::Surface)
        .radius_px(look.radius)
        .with_layout(|layout| {
            layout.overflow_x = OverflowSpec::Hidden;
            layout.overflow_y = OverflowSpec::Hidden;
        });
    let frame = if look.stacked {
        frame.min_width(LengthSpec::Px(0.0))
    } else {
        let width = super::DETAIL_WIDTH;
        frame.width(LengthSpec::Px(width)).min_width(LengthSpec::Px(width)).grow(0.0).shrink(0.0)
    };
    frame.node_style()
}

/// 滚动区和卡片内容。空状态上下居中，其余顶端对齐。滚动区按显示的条目取键：换选中项回到顶部。
fn scroller(key: &DetailKey, signals: DetailSignals, metadata: MetadataSignals) -> AnyView {
    let (body, centered) = match &key.kind {
        DetailKind::Idle => return ().into_any(),
        DetailKind::Multi => (multi_card(signals.summary), false),
        DetailKind::Entry { key: entry, metadata: editable, trash } => {
            (entry_card(signals, entry, *editable, *trash, metadata), false)
        }
        DetailKind::Directory => (directory_card(signals.summary), false),
        DetailKind::Empty { smart, trash } => (empty_card(*smart, *trash), true),
    };
    let mut content = Stack::column(14.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)).padding(19.0);
    if centered {
        content = content.height(LengthSpec::Fill).justify(JustifySpec::Center);
    }
    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
    }))
    .children((widget(content).children((body,)).key("file-detail-content"),))
    .key(format!("file-detail-scroll-{}", style::key_part(&key.scroll)))
    .into_any()
}

/// `files-detail__section`：标题 18px/700（上 4），路径 14px 弱化色（上 8，空时不占位）。
fn section(title: impl Fn() -> String + Send + Clone + 'static, subline: impl Fn() -> String + Send + Clone + 'static, lead: Option<AnyView>, key: &str) -> AnyView {
    let mut rows = vec![
        widget(style::margin_top(style::heading(untrack(&title)), 4.0))
            .prop::<String, fields::text::value>(title)
            .key(format!("{key}-title"))
            .into_any(),
        widget(style::margin_top(style::subline(untrack(&subline)), 8.0))
            .prop::<String, HiddenWhenEmpty<fields::text::value>>(subline)
            .key(format!("{key}-subline"))
            .into_any(),
    ];
    rows.extend(lead);
    widget(Stack::column(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children(rows).key(key.to_string()).into_any()
}

/// 多选：标题写条目数，下面是「N 个文件夹 · N 个文件」。
fn multi_card(summary: Signal<SummaryView>) -> AnyView {
    let title = move || summary.with(|view| view.title.clone());
    let subline = move || summary.with(|view| view.subline.clone());
    card(vec![section(title, subline, None, "file-detail-multi")], "file-detail-card")
}

/// 单个条目：缩略图、色板、标题区、事实行，文件再接元数据编辑。
fn entry_card(signals: DetailSignals, key: &str, editable: bool, trash: bool, metadata: MetadataSignals) -> AnyView {
    let entry = signals.entry;
    let read = move |pick: fn(&EntryView) -> String| move || entry.with(|view| pick(view));
    let preview = signals.preview;
    let thumbnail = widget(Stack::column(0.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .children((preview_box(move || preview.get(), None, 160.0, 34.0, RadiusTier::Xl, key),))
        .key("file-detail-preview")
        .into_any();
    let palette = dynamic(move || entry.with(|view| view.palette.clone()), |colors: &Vec<String>| {
        super::super::palette::swatches(colors, "file-detail-palette").unwrap_or_else(|| ().into_any())
    })
    .visible(move || entry.with(|view| style::any_color(&view.palette)))
    .into_any();
    let lead = widget(Stack::column(8.0).width(LengthSpec::Fill).with_layout(|layout| layout.margin_top = Some(LengthSpec::Px(12.0))))
        .visible(move || entry.with(|view| !view.comment.is_empty() || !view.link.is_empty()))
        .children((
            lead_row("注释", read(|view| view.comment.clone()), "file-detail-comment"),
            lead_row("链接", read(|view| view.link.clone()), "file-detail-link"),
        ))
        .key("file-detail-section-lead")
        .into_any();
    let section = section(read(|view| view.title.clone()), read(|view| view.subline.clone()), Some(lead), "file-detail-section");
    let always = || true;
    let mut stats = vec![
        fact_row("类型", read(|view| view.type_label.clone()), "file-detail-type", true, always),
        fact_row("大小", read(|view| view.size.clone()), "file-detail-size", false, always),
        fact_row("硬链接", read(|view| view.hardlink.clone()), "file-detail-hardlink", false, move || {
            entry.with(|view| !view.hardlink.is_empty())
        }),
        fact_row("受保护", read(|view| view.protected.clone()), "file-detail-protected", false, move || {
            entry.with(|view| !view.protected.is_empty())
        }),
        fact_row("修改时间", read(|view| view.modified.clone()), "file-detail-modified", false, always),
    ];
    if trash {
        stats.push(fact_row("删除时间", read(|view| view.deleted.clone()), "file-detail-deleted", false, always));
    }
    let mut parts = vec![
        thumbnail,
        palette,
        section,
        widget(Stack::column(12.0).width(LengthSpec::Fill)).children(stats).key("file-detail-stats").into_any(),
    ];
    if editable {
        parts.push(inspect_metadata_view::metadata_panel(metadata));
    }
    card(parts, "file-detail-entry")
}

/// 注释和链接：最弱色小标签在上，13px/500 的值在下。没有内容时不占位。
fn lead_row(label: &str, value: impl Fn() -> String + Send + Clone + 'static, key: &str) -> AnyView {
    let shown = value.clone();
    widget(Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .visible(move || !shown().is_empty())
        .children((
            widget(style::row_label(label)).key(format!("{key}-label")),
            widget(style::wrapping(style::text(untrack(&value), 13.0, 500, SemanticColorRole::Text, 19.5)))
                .prop::<String, fields::text::value>(value)
                .key(format!("{key}-value")),
        ))
        .key(key.to_string())
        .into_any()
}

/// 没有选择：当前目录名（根目录写资源库名）、路径，以及直属计数。
fn directory_card(summary: Signal<SummaryView>) -> AnyView {
    let read = move |pick: fn(&SummaryView) -> String| move || summary.with(|view| pick(view));
    let stats = widget(Stack::column(12.0).width(LengthSpec::Fill))
        .children((
            fact_row("直属文件", read(|view| view.counts[0].clone()), "file-detail-files", true, || true),
            fact_row("直属子文件夹", read(|view| view.counts[1].clone()), "file-detail-folders", false, || true),
            fact_row("当前视图总条目", read(|view| view.counts[2].clone()), "file-detail-total", false, || true),
        ))
        .key("file-detail-stats")
        .into_any();
    let section = section(read(|view| view.title.clone()), read(|view| view.subline.clone()), None, "file-detail-directory");
    card(vec![section, stats], "file-detail-card")
}

/// 回收站或分类视图里还没有选择：眉题、标题和下一步提示，上下居中。
fn empty_card(smart: bool, trash: bool) -> AnyView {
    let eyebrow = if smart {
        "智能文件夹"
    } else if trash {
        "回收站"
    } else {
        "文件管理"
    };
    let message = if trash && !smart {
        "在中间列表中选择目标，然后可执行还原或彻底删除。"
    } else {
        "在中间列表中选择目标查看详情。"
    };
    widget(Stack::column(0.0).width(LengthSpec::Fill))
        .children((
            widget(style::eyebrow(eyebrow)).key("file-detail-empty-eyebrow"),
            widget(style::margin_top(style::heading("选择一个文件或文件夹"), 4.0)).key("file-detail-empty-title"),
            widget(style::margin_top(style::subline(message), 8.0)).key("file-detail-empty-copy"),
        ))
        .key("file-detail-empty")
        .into_any()
}

/// `files-detail__card`：各块之间 16。
fn card(parts: Vec<AnyView>, key: &'static str) -> AnyView {
    widget(Stack::column(16.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children(parts).key(key).into_any()
}

fn type_label(row: &FileRow) -> String {
    if row.kind == "directory" {
        "文件夹".into()
    } else {
        row.extension.as_deref().map(str::trim).filter(|text| !text.is_empty()).unwrap_or("文件").to_string()
    }
}
