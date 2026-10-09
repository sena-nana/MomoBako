//! 已选文件的元数据编辑，对应 Vue `FileMetadataEditor.vue`。
//!
//! 顺序和 Vue 一致：注释、链接两行带图标的输入，评分五颗星，可折叠的标签组，然后是
//! 添加到资源库、创建时间、文件修改时间、尺寸、原始大小、调色板、索引标签、多归属位置、
//! 来源标题和来源链接。改动先进草稿，260ms 后自动保存；画面上不放保存按钮。
//! 自动保存撞上服务器的新版本时，注释前面先写出冲突和「采用服务器版本」，草稿留在输入框里。
//! 文件详情和预览页共用这一块。
//!
//! 常驻：信号（[`MetadataSignals`]）在主区块的常驻作用域里，详情和预览页各建一份视图、读同一组信号。
//! 输入框受控（`.model` 加 [`DraftField`]），ViewModel 的值变了才写回，组合输入中不被别的更新打断；
//! 文字、星级、禁用态按字段绑定；可有可无的行用 `.visible`；标签、候选和自定义字段是带键的 `each`。
//! 事件处理器只发消息，要用到当前值时现读信号。

use std::collections::BTreeMap;
use std::sync::Arc;

use nana_ui::icons_tabler::{COPY, EXTERNAL_LINK, LINK, MESSAGE, STAR};
use nana_ui::runtime::view::{dynamic, each, fields, signal, widget, AnyView, El, IntoView, Signal};
use nana_ui::runtime::{
    Activate, AlignSpec, IconGlyph, JustifySpec, LengthSpec, ListItem, NodeStyle, PaintContext, Painter, RadiusTier,
    SemanticColorRole, SemanticPaint, Stack, TextChanged, TextInput, ValidationIntent, ValidationMessage,
};
use nana_ui_core::Icon;
use serde_json::Value;

use super::files::{local_time, FileContext, FileRow, FilesMessage};
use super::files_view::bind::{fact_row, DraftField, StyleField, ValidationTextField};
use super::files_view::style::{self, ButtonLook};
use super::input::InputMessage;
use super::inspect::InspectMessage;
use super::inspect_asmr::CandidatesView;
use super::inspect_library::LibrarySection;
use super::{ShellMessage, ShellViewModel};

#[path = "inspect_metadata_parts.rs"]
mod parts;

/// Vue `.file-metadata-card__star.is-active` 的金色。浅色深色主题同一个值。
const STAR_GOLD: [f32; 4] = [212.0 / 255.0, 166.0 / 255.0, 61.0 / 255.0, 1.0];

/// 冲突说明和能不能编辑。
#[derive(Clone, Debug, Default, PartialEq)]
struct HeadView {
    conflict: String,
    /// 素材不能编辑（没有素材、虚拟素材或正在保存）。
    locked: bool,
}

/// 标签组：路径、草稿里的标签、展开和菜单的状态，以及菜单里的候选。
#[derive(Clone, Debug, Default, PartialEq)]
struct TagsView {
    path: String,
    tags: Vec<String>,
    expanded: bool,
    menu_open: bool,
    /// 当前目录里已经用过、还没加进草稿的标签，按输入过滤，最多 18 个。
    options: Vec<String>,
    /// 输入的新标签不空、也不重复时才能添加。
    can_create: bool,
    /// 「添加“…”」按钮上的字。
    add_label: String,
}

/// 添加到资源库到来源链接的只读行。空字符串的行不占位。
#[derive(Clone, Debug, Default, PartialEq)]
struct RecordedView {
    added: String,
    created: String,
    file_modified: String,
    dimensions: String,
    original: String,
    /// 色板颜色，原样保留：建色块时跳过不合法的并记日志，一个合法的都没有时整行不占位。
    palette: Vec<String>,
    indexed_tags: String,
    aliases: String,
    origin_title: String,
    source_url: String,
}

/// 元数据区的常驻信号。文件详情和预览页共用一份。
#[derive(Clone, Copy)]
pub(super) struct MetadataSignals {
    head: Signal<HeadView>,
    rating: Signal<i64>,
    tags: Signal<TagsView>,
    recorded: Signal<RecordedView>,
    library: Signal<Vec<LibrarySection>>,
    candidates: Signal<CandidatesView>,
    /// 自定义字段：键和值，按键名排序。
    custom: Signal<Vec<(String, String)>>,
    comment: DraftField,
    link: DraftField,
    tag_draft: DraftField,
    lookup_id: DraftField,
    import_json: DraftField,
}

/// 元数据区的全部投影。
struct MetadataView {
    head: HeadView,
    rating: i64,
    tags: TagsView,
    recorded: RecordedView,
    library: Vec<LibrarySection>,
    candidates: CandidatesView,
    custom: Vec<(String, String)>,
    comment: String,
    link: String,
    tag_draft: String,
    lookup_id: String,
    import_json: String,
}

impl MetadataView {
    /// 从 ViewModel 取元数据区的投影。
    fn project(model: &ShellViewModel) -> Self {
        let inspect = &model.inspect;
        let row = target_row(model);
        let custom = super::inspect_asmr::display_custom(model);
        let library = super::inspect_library::library_sections(&custom);
        let claimed = !library.is_empty();
        Self {
            head: HeadView { conflict: inspect.conflict.clone(), locked: !inspect.can_edit() },
            rating: inspect.draft_rating(),
            tags: tags_view(model),
            recorded: recorded_view(model, row),
            candidates: super::inspect_asmr::candidates_view(model, &custom),
            custom: custom_fields(&custom, claimed).map(|(key, value)| (key.clone(), value.clone())).collect(),
            library,
            comment: inspect.draft_comment().to_string(),
            link: inspect.draft_link().to_string(),
            tag_draft: model.files.tag_draft.clone(),
            lookup_id: super::inspect_asmr::lookup_id_shown(model, &custom),
            import_json: super::inspect_asmr::import_draft(model).to_string(),
        }
    }
}

impl MetadataSignals {
    /// 在常驻作用域里建信号，初值是这一刻的投影。
    pub(super) fn new(model: &ShellViewModel) -> Self {
        let view = MetadataView::project(model);
        Self {
            head: signal(view.head),
            rating: signal(view.rating),
            tags: signal(view.tags),
            recorded: signal(view.recorded),
            library: signal(view.library),
            candidates: signal(view.candidates),
            custom: signal(view.custom),
            comment: DraftField::new(&view.comment),
            link: DraftField::new(&view.link),
            tag_draft: DraftField::new(&view.tag_draft),
            lookup_id: DraftField::new(&view.lookup_id),
            import_json: DraftField::new(&view.import_json),
        }
    }

    /// 写入投影，只写变了的；输入框的草稿只在 ViewModel 的值变了时写回。
    pub(super) fn write(&self, model: &ShellViewModel) {
        let view = MetadataView::project(model);
        self.head.try_set_if_changed(view.head);
        self.rating.try_set_if_changed(view.rating);
        self.tags.try_set_if_changed(view.tags);
        self.recorded.try_set_if_changed(view.recorded);
        self.library.try_set_if_changed(view.library);
        self.candidates.try_set_if_changed(view.candidates);
        self.custom.try_set_if_changed(view.custom);
        self.comment.sync(&view.comment);
        self.link.sync(&view.link);
        self.tag_draft.sync(&view.tag_draft);
        self.lookup_id.sync(&view.lookup_id);
        self.import_json.sync(&view.import_json);
    }

    /// 素材现在能不能编辑，绑定和事件处理器用。
    fn locked(&self) -> impl Fn() -> bool + Send + Clone + 'static {
        let head = self.head;
        move || head.with(|head| head.locked)
    }
}

/// 元数据编辑块：上边一条发丝线，块内各行间距 12。
pub(super) fn metadata_panel(signals: MetadataSignals) -> AnyView {
    let library = signals.library;
    let custom = signals.custom;
    let rows = (
        conflict_notice(signals.head),
        inline_field("注释", MESSAGE, "inspect-comment", signals.comment, "记录这个文件的用途、状态或上下文。", true, signals, InspectMessage::SetComment),
        inline_field("链接", LINK, "inspect-link", signals.link, "https://example.com", false, signals, InspectMessage::SetLink),
        rating_row(signals),
        parts::tags_row(signals),
        recorded_grid(signals.recorded),
        each(library, LibrarySection::clone, super::inspect_library::library_card)
            .gap(12.0)
            .visible(move || library.with(|sections| !sections.is_empty())),
        parts::candidate_section(signals),
        each(custom, |(key, value): &(String, String)| (key.clone(), value.clone()), |(key, value)| {
            fact(&key, value, &format!("inspect-custom-{}", style::key_part(&key)), false)
        })
        .gap(12.0)
        .key("inspect-custom")
        .visible(move || custom.with(|rows| !rows.is_empty())),
    );
    let panel = style::top_rule(Stack::column(12.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)), 12.0);
    widget(panel).children(rows).key("inspect-metadata").into_any()
}

/// 自定义行要画的键值。`claimed` 为真时作品信息区块已经画出它认领的键。
fn custom_fields(custom: &BTreeMap<String, String>, claimed: bool) -> impl Iterator<Item = (&String, &String)> {
    custom.iter().filter(move |(key, _)| {
        !super::inspect::is_reserved_metadata(key)
            && !(claimed && super::inspect_library::claimed_metadata_keys().contains(&key.as_str()))
    })
}

/// 自动保存撞上服务器的新版本：警告色写明冲突，右边是「采用服务器版本」。没有冲突时不占位。
/// 放在注释前面，紧挨着被拒绝写入的字段，不沉到只读事实下面（DESIGN.md：冲突必须靠近字段出现）。
fn conflict_notice(head: Signal<HeadView>) -> AnyView {
    let conflict = head.with_untracked(|head| head.conflict.clone());
    let notice = Stack::bar(8.0).align(AlignSpec::Center).justify(JustifySpec::SpaceBetween).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0));
    widget(notice)
        .visible(move || head.with(|head| !head.conflict.is_empty()))
        .children((
            widget(ValidationMessage::new(conflict, ValidationIntent::Warning))
                .prop::<String, ValidationTextField>(move || head.with(|head| head.conflict.clone()))
                .key("inspect-conflict"),
            widget(style::styled_button("采用服务器版本", None, ButtonLook::TOOLBAR))
                .key("inspect-adopt")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(inspect_message(InspectMessage::AdoptConflict))),
        ))
        .key("inspect-conflict-row")
        .into_any()
}

/// 当前检视目标在文件列表里的那一行。预览页从播放集打开时可能不在列表里。
fn target_row(model: &ShellViewModel) -> Option<&FileRow> {
    let path = model.inspect.target_path.as_deref()?;
    let ctx = FileContext::from_model(model);
    let files = &model.files;
    if ctx.smart_folder {
        return files.virtual_rows.iter().find(|row| row.path == path);
    }
    files.rows.iter().filter(|row| !ctx.category_virtual || row.kind != "directory").find(|row| row.path == path)
}

/// 注释 / 链接：小标签在上；下面是 34 高的描边框，左边 14px 图标，右边不带边的输入。
#[allow(clippy::too_many_arguments)]
fn inline_field(
    label: &'static str,
    icon: Icon,
    key: &'static str,
    draft: DraftField,
    placeholder: &'static str,
    first: bool,
    signals: MetadataSignals,
    map: fn(String) -> InspectMessage,
) -> AnyView {
    let locked = signals.locked();
    let mut input = TextInput::new(draft.current()).placeholder(placeholder).disabled(nana_ui::runtime::view::untrack(&locked));
    input.style.border = None;
    input.style.background = None;
    input.style.radius = None;
    input.style.control_height = None;
    input.style.control_padding_x = None;
    input.style.interaction.hovered.border = None;
    input.style.interaction.focused.border = None;
    input.style.interaction.hovered.background = None;
    input.style.interaction.focused.background = None;
    input.style.interaction.disabled.background = None;
    input.style.interaction.disabled.border = None;
    let layout = Arc::make_mut(&mut input.style.layout);
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Fill);
    layout.height = Some(LengthSpec::Px(32.0));
    layout.padding_left = Some(LengthSpec::Px(10.0));
    layout.padding_right = Some(LengthSpec::Px(10.0));
    layout.font_size = Some(14.0);
    layout.border_width = Some(0.0);
    let frame = Stack::row(8.0)
        .align(AlignSpec::Center)
        .width(LengthSpec::Fill)
        .min_width(LengthSpec::Px(0.0))
        .padding_xy(10.0, 0.0)
        .min_height(LengthSpec::Px(34.0))
        .surface(SemanticColorRole::Background)
        .outline(SemanticColorRole::BorderSoft, 1.0)
        .radius(RadiusTier::Lg);
    widget(style::meta_row(first))
        .key(format!("{key}-row"))
        .children((
            widget(style::row_label(label)).key(format!("{key}-label")),
            widget(frame).key(format!("{key}-field")).children((
                widget(IconGlyph::new(icon).size(14.0).role(SemanticColorRole::Muted)).key(format!("{key}-icon")),
                widget(input)
                    .model(draft.signal())
                    .prop::<bool, fields::text_input::disabled>(locked)
                    .key(key)
                    .on_cx(move |_, event: &TextChanged, cx| {
                        cx.dispatch_program_all(inspect_message(map(event.value.to_string())));
                    }),
            )),
        ))
        .into_any()
}

/// 评分：五颗 34 方的星，间距 4。到达当前分数的星是金色、浅强调底。再点同一颗回到 0。
fn rating_row(signals: MetadataSignals) -> AnyView {
    let (rating, head) = (signals.rating, signals.head);
    let stars = (1_i64..=5)
        .map(|value| {
            let active = move || rating.get() >= value;
            let locked = move || head.with(|head| head.locked);
            let (active_now, locked_now) = (rating.get_untracked() >= value, head.with_untracked(|head| head.locked));
            let glyph = Stack::column(0.0).style(star_glyph_style(active_now));
            widget(ListItem::new(format!("{value} 星")).disabled(locked_now).style(star_style(active_now, locked_now)))
                .prop::<bool, fields::list_item::disabled>(locked)
                .prop::<NodeStyle, StyleField>(move || star_style(active(), locked()))
                .content(widget(glyph).prop::<NodeStyle, StyleField>(move || star_glyph_style(active())))
                .key(format!("inspect-rate-{value}"))
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(inspect_message(InspectMessage::SetRating(value))))
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(style::meta_row(false))
        .key("inspect-rating-row")
        .children((
            widget(style::row_label("评分")).key("inspect-rating"),
            widget(Stack::row(4.0).align(AlignSpec::Center)).key("inspect-rating-stars").children(stars),
        ))
        .into_any()
}

/// 一颗星的外框：34 方的圆，到达分数时浅强调底；不能编辑时淡到一半。
fn star_style(active: bool, locked: bool) -> NodeStyle {
    let mut style = NodeStyle::default();
    style.radius = None;
    style.background = active.then_some(SemanticColorRole::AccentSoft);
    style.interaction.hovered = SemanticPaint { background: Some(SemanticColorRole::AccentSoft), ..SemanticPaint::default() };
    style.interaction.pressed = style.interaction.hovered;
    style.interaction.disabled = SemanticPaint::default();
    let layout = Arc::make_mut(&mut style.layout);
    for edge in [&mut layout.width, &mut layout.height, &mut layout.min_width, &mut layout.min_height] {
        *edge = Some(LengthSpec::Px(34.0));
    }
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.opacity = locked.then_some(0.5);
    style.radius_px(999.0)
}

/// 星形图标所在的一层：铺满外框，自绘星形。
fn star_glyph_style(active: bool) -> NodeStyle {
    Stack::column(0.0).width(LengthSpec::Fill).height(LengthSpec::Fill).painter(StarPainter { active }).node_style()
}

/// 星形图标：18px 描边星，到达分数时金色，否则弱化色。
#[derive(Clone, Copy, Debug)]
struct StarPainter {
    active: bool,
}

impl Painter for StarPainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let bounds = cx.bounds();
        let rect = nana_ui::runtime::LayoutBox {
            x: bounds.x + (bounds.width - 18.0) / 2.0,
            y: bounds.y + (bounds.height - 18.0) / 2.0,
            width: 18.0,
            height: 18.0,
        };
        let color = if self.active { STAR_GOLD } else { cx.color(SemanticColorRole::Muted) };
        cx.icon(rect, STAR, color);
    }

    fn paint_key(&self) -> u64 {
        u64::from(self.active)
    }
}

/// 添加到资源库到来源链接的只读行。第一行不画上边线；没有内容的行不占位。
fn recorded_view(model: &ShellViewModel, row: Option<&FileRow>) -> RecordedView {
    let facts = &model.inspect.facts;
    let empty = BTreeMap::new();
    let metadata = row.map(|row| &row.metadata).unwrap_or(&empty);
    let text = |key: &str, fallback: &str| -> String {
        match metadata.get(key) {
            Some(Value::String(value)) if !value.trim().is_empty() => value.trim().to_string(),
            _ => fallback.trim().to_string(),
        }
    };
    let modified_fallback = row.map(|row| row.modified_at.clone()).filter(|value| !value.is_empty()).unwrap_or_else(|| facts.modified_at.clone());
    let file_modified = {
        let meta = text("fileModifiedAt", &facts.file_modified_meta);
        if meta.is_empty() { modified_fallback } else { meta }
    };
    let width = number(metadata, "width").or((facts.meta_width > 0).then_some(f64::from(facts.meta_width)));
    let height = number(metadata, "height").or((facts.meta_height > 0).then_some(f64::from(facts.meta_height)));
    let dimensions = match (width, height) {
        (Some(width), Some(height)) if width > 0.0 && height > 0.0 => format!("{} × {}", plain(width), plain(height)),
        _ => "未记录".into(),
    };
    let original = number(metadata, "originalSizeBytes").or(facts.original_size_bytes);
    let palette = row.map(|row| row.palette.clone()).filter(|colors| !colors.is_empty()).unwrap_or_else(|| model.inspect.palette.clone());
    let aliases = row
        .and_then(|row| model.browser_entries.iter().find(|entry| entry.path == row.path))
        .map(|entry| entry.alias_paths.iter().filter(|path| **path != entry.path).cloned().collect::<Vec<_>>().join("，"))
        .unwrap_or_default();
    RecordedView {
        added: local_time::format_or(&text("addedToLibraryAt", &facts.added_to_library_at), "未记录"),
        created: local_time::format_or(&text("fileCreatedAt", &facts.file_created_at), "未记录"),
        file_modified: local_time::format_or(&file_modified, "未记录"),
        dimensions,
        original: super::inspect_library::format_original_size(original),
        palette,
        indexed_tags: row.map(|row| row.tags.join("，")).unwrap_or_default(),
        aliases,
        origin_title: text("originTitle", ""),
        source_url: text("sourceUrl", ""),
    }
}

/// 只读行：五个事实一直在，调色板、索引标签、多归属位置、来源标题和来源链接有内容才占位。
fn recorded_grid(recorded: Signal<RecordedView>) -> AnyView {
    let read = move |pick: fn(&RecordedView) -> String| move || recorded.with(|view| pick(view));
    let shown = move |pick: fn(&RecordedView) -> bool| move || recorded.with(|view| pick(view));
    let palette = dynamic(move || recorded.with(|view| view.palette.clone()), |colors: &Vec<String>| {
        super::palette::metadata_chips(colors, "inspect-metadata-palette").unwrap_or_else(|| ().into_any())
    });
    let always = || true;
    let rows = (
        fact_row("添加到资源库", read(|view| view.added.clone()), "inspect-recorded-added", true, always),
        fact_row("创建时间", read(|view| view.created.clone()), "inspect-recorded-created", false, always),
        fact_row("文件修改时间", read(|view| view.file_modified.clone()), "inspect-recorded-modified", false, always),
        fact_row("尺寸", read(|view| view.dimensions.clone()), "inspect-recorded-size", false, always),
        fact_row("原始大小", read(|view| view.original.clone()), "inspect-recorded-bytes", false, always),
        widget(style::meta_row(false))
            .visible(shown(|view| style::any_color(&view.palette)))
            .children((widget(style::row_label("调色板")).key("inspect-palette-label"), palette))
            .key("inspect-palette"),
        fact_row("索引标签", read(|view| view.indexed_tags.clone()), "inspect-indexed-tags", false, shown(|view| !view.indexed_tags.is_empty())),
        right_fact("多归属位置", read(|view| view.aliases.clone()), "inspect-alias-paths", shown(|view| !view.aliases.is_empty())),
        right_fact("来源标题", read(|view| view.origin_title.clone()), "inspect-origin-title", shown(|view| !view.origin_title.is_empty())),
        source_link_row(recorded, shown(|view| !view.source_url.is_empty())),
    );
    widget(Stack::column(12.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children(rows).key("inspect-recorded").into_any()
}

/// 小标签和值的一行，和详情事实行同一种画法。值是常量。
fn fact(label: &str, value: String, key: &str, first: bool) -> AnyView {
    widget(style::meta_row(first))
        .children((
            widget(style::row_label(label.to_string())).key(format!("{key}-label")),
            widget(style::value(value)).key(format!("{key}-value")),
        ))
        .key(key.to_string())
        .into_any()
}

/// 值右对齐的一行：多归属位置和来源标题。没有内容时不占位。
fn right_fact(label: &str, value: impl Fn() -> String + Send + Clone + 'static, key: &str, shown: impl Fn() -> bool + Send + 'static) -> AnyView {
    let mut node = style::value(nana_ui::runtime::view::untrack(&value));
    node.style.text_horizontal_alignment = nana_ui::runtime::TextHorizontalAlignment::End;
    widget(style::meta_row(false))
        .visible(shown)
        .children((
            widget(style::row_label(label.to_string())).key(format!("{key}-label")),
            widget(node).prop::<String, fields::text::value>(value).key(format!("{key}-value")),
        ))
        .key(key.to_string())
        .into_any()
}

/// 来源链接：右对齐的地址，后面是「打开链接」「复制链接」两个 26 方的描边按钮。
/// 只有带协议、且不是 javascript / data / vbscript 的地址才能打开。点下时现读地址。没有地址时不占位。
fn source_link_row(recorded: Signal<RecordedView>, shown: impl Fn() -> bool + Send + 'static) -> AnyView {
    let url = move || recorded.with(|view| view.source_url.clone());
    let mut value = style::value(recorded.with_untracked(|view| view.source_url.clone()));
    value.style.text_horizontal_alignment = nana_ui::runtime::TextHorizontalAlignment::End;
    let openable = move || recorded.with(|view| openable_link(&view.source_url));
    let actions = widget(Stack::row(4.0).grow(0.0).shrink(0.0)).key("inspect-source-actions").children((
        source_action(EXTERNAL_LINK, "inspect-source-open")
            .prop::<bool, fields::button::disabled>(move || !openable())
            .on_cx(move |_, _: &Activate, cx| {
                let url = recorded.with_untracked(|view| view.source_url.clone());
                if openable_link(&url) {
                    cx.dispatch_program_all(ShellMessage::Input(InputMessage::OpenExternalUrl { url }));
                }
            }),
        source_action(COPY, "inspect-source-copy").on_cx(move |_, _: &Activate, cx| {
            let url = recorded.with_untracked(|view| view.source_url.clone());
            cx.dispatch_program_all(ShellMessage::Files(FilesMessage::CopyText(url)));
        }),
    ));
    widget(style::meta_row(false))
        .visible(shown)
        .children((
            widget(style::row_label("来源链接")).key("inspect-source-label"),
            widget(Stack::row(8.0).align(AlignSpec::Start).justify(JustifySpec::End).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
                .children((widget(value).prop::<String, fields::text::value>(url).key("inspect-source-value"), actions))
                .key("inspect-source-body"),
        ))
        .key("inspect-source")
        .into_any()
}

fn source_action(icon: Icon, key: &str) -> El<nana_ui::runtime::Button> {
    let look = ButtonLook {
        height: 26.0,
        padding_x: 5.0,
        foreground: SemanticColorRole::Muted,
        background: Some(SemanticColorRole::Background),
        hover: Some(SemanticColorRole::Background),
        icon_size: 14.0,
        ..ButtonLook::TOOLBAR
    };
    let button = style::styled_button("", Some(icon), look);
    let outlined = button.style.clone().outline(SemanticColorRole::BorderSoft, 1.0);
    let mut button = button.style(outlined);
    Arc::make_mut(&mut button.style.layout).width = Some(LengthSpec::Px(26.0));
    widget(button).key(key.to_string())
}

/// 和 Vue `isOpenableSourceLink` 一致：要有协议，且不是脚本或内联数据。
fn openable_link(url: &str) -> bool {
    let Some((scheme, _)) = url.trim().split_once(':') else {
        return false;
    };
    let valid = scheme.chars().next().is_some_and(|first| first.is_ascii_alphabetic())
        && scheme.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '.' | '-'));
    valid && !matches!(scheme.to_ascii_lowercase().as_str(), "javascript" | "data" | "vbscript")
}

/// 标签组的投影：展开状态按用户点过的来，菜单里的已有标签按输入过滤。
fn tags_view(model: &ShellViewModel) -> TagsView {
    let inspect = &model.inspect;
    let tags = inspect.draft_tags().to_vec();
    let path = inspect.target_path.clone().unwrap_or_default();
    let expanded = model.files.tags_expanded(&path, !tags.is_empty());
    let trimmed = model.files.tag_draft.trim().to_string();
    let keyword = trimmed.to_lowercase();
    let mut options: Vec<String> = Vec::new();
    for row in &model.files.rows {
        for tag in &row.tags {
            if !tags.contains(tag) && !options.contains(tag) && (keyword.is_empty() || tag.to_lowercase().contains(&keyword)) {
                options.push(tag.clone());
            }
        }
    }
    options.truncate(18);
    let can_create = !trimmed.is_empty() && !tags.iter().any(|tag| tag == &trimmed);
    let add_label = format!("添加“{}”", if trimmed.is_empty() { "新标签" } else { trimmed.as_str() });
    TagsView { path, expanded, menu_open: inspect.tag_menu_open(), options, can_create, add_label, tags }
}

fn number(metadata: &BTreeMap<String, Value>, key: &str) -> Option<f64> {
    metadata.get(key).and_then(Value::as_f64).filter(|value| value.is_finite())
}

fn plain(value: f64) -> String {
    if value.fract() == 0.0 { format!("{value:.0}") } else { value.to_string() }
}

fn inspect_message(message: InspectMessage) -> ShellMessage {
    ShellMessage::Inspect(message)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::Value;

    use super::{custom_fields, openable_link};

    /// 通用字段、后端种下的系统字段都不进自定义行；作品信息认领的键只在区块画出来时才让出。
    #[test]
    fn custom_rows_skip_reserved_and_seeded_keys() {
        let mut custom = BTreeMap::new();
        for key in [
            "comment", "note", "link", "rating", "tagGroups", "width", "height", "originalSizeBytes", "addedToLibraryAt",
            "fileCreatedAt", "fileModifiedAt", "palette", "thumbnailPalette", "originTitle", "sourceUrl", "title", "type",
            "favorite", "color", "artist", "workTitle",
        ] {
            custom.insert(key.to_string(), "x".to_string());
        }
        let shown = |claimed| custom_fields(&custom, claimed).map(|(key, _)| key.as_str()).collect::<Vec<_>>();
        assert_eq!(shown(false), ["artist", "workTitle"]);
        assert_eq!(shown(true), ["artist"]);
    }

    /// 选中带完整元数据的 cover.png：评分、注释、尺寸这些只在各自的行里出现一次，不再按原始键名重复；
    /// 后端种下的标题、类型、收藏和主色也不出现，真正的自定义字段照常列出。
    #[test]
    fn selected_metadata_lists_each_reserved_field_once() {
        let mut model = crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(name, _)| *name == "files-selected-metadata")
            .map(|(_, model)| model)
            .expect("files-selected-metadata 场景");
        let row = model.files.rows.iter_mut().find(|row| row.path == "cover.png").expect("cover.png");
        for (key, value) in [("title", "cover.png"), ("type", "png"), ("color", "#c7a566"), ("artist", "Momo")] {
            row.metadata.insert(key.into(), Value::String(value.into()));
        }
        row.metadata.insert("favorite".into(), Value::Bool(false));
        let document = crate::acceptance_document_for_model(model).expect("生产文档");
        let nodes = document.context().world().project_accessibility(document.document());
        let labelled = |label: &str| nodes.iter().filter(|node| node.label.as_deref() == Some(label)).count();
        for key in [
            "comment", "link", "rating", "width", "height", "originalSizeBytes", "addedToLibraryAt", "fileCreatedAt", "title",
            "type", "favorite", "color",
        ] {
            assert_eq!(labelled(key), 0, "保留字段 {key} 不该按原始键名再列一行");
        }
        assert_eq!(labelled("尺寸"), 1);
        assert_eq!(labelled("1920 × 1080"), 1);
        assert_eq!(labelled("artist"), 1, "真正的自定义字段照常列出");
        assert_eq!(labelled("Momo"), 1);
    }

    #[test]
    fn only_real_protocols_can_be_opened() {
        assert!(openable_link("https://example.com/cover"));
        assert!(openable_link("eagle://item/1"));
        assert!(!openable_link("javascript:alert(1)"));
        assert!(!openable_link("DATA:text/html,x"));
        assert!(!openable_link("example.com"));
        assert!(!openable_link("1http://x"));
    }
}
