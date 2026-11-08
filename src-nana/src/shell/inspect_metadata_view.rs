//! 已选文件的元数据编辑，对应 Vue `FileMetadataEditor.vue`。
//!
//! 顺序和 Vue 一致：注释、链接两行带图标的输入，评分五颗星，可折叠的标签组，然后是
//! 添加到资源库、创建时间、文件修改时间、尺寸、原始大小、调色板、索引标签、多归属位置、
//! 来源标题和来源链接。详情卡这几行跟 `currentFileEntry`：素材编号对得上才把详情叠到列表上。
//! 作品信息、自定义行和补全候选跟这几行同一份元数据，不跟编辑器里还没保存的草稿。
//! 预览页跟 `previewFileEntry`，不拿素材详情补列表里没有的值。索引标签和多归属位置仍只读列表条目。
//! 改动先进草稿，260ms 后自动保存；画面上不放保存按钮。
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
use nana_ui::runtime::view::{
    AnyView, El, IntoView, Signal, dynamic, each, fields, signal, widget,
};
use nana_ui::runtime::{
    Activate, AlignSpec, IconButton, IconGlyph, JustifySpec, LengthSpec, ListItem, NodeStyle,
    PaintContext, Painter, RadiusTier, SemanticColorRole, SemanticPaint, Stack, TextChanged,
    TextInput, ValidationIntent, ValidationMessage,
};
use nana_ui_core::Icon;
use serde_json::Value;

use crate::host_api::openable_url;

use super::files::{FileContext, FileRow, FilesMessage, local_time};
use super::files_view::bind::{DraftField, StyleField, ValidationTextField, fact_row};
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
    /// 仓库的标签分组里还没加进草稿的标签，空的分组不列（Vue `groupedTagOptions`，不按输入过滤）。
    groups: Vec<TagGroupOption>,
    /// 整个资源库（仓库摘要）和搜索结果里用过、还没加进草稿的标签，按 zh-CN 排序、按输入过滤，最多 18 个
    /// （Vue `availableTags` 即筛选栏的标签候选）。
    options: Vec<String>,
    /// 输入的新标签不空、也不重复时才能添加。
    can_create: bool,
    /// 「添加“…”」按钮上的字。
    add_label: String,
}

/// 添加到资源库到来源链接的只读行。空字符串的行不占位。
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct RecordedView {
    pub(super) added: String,
    pub(super) created: String,
    pub(super) file_modified: String,
    pub(super) dimensions: String,
    pub(super) original: String,
    /// 色板颜色，原样保留：建色块时跳过不合法的并记日志，一个合法的都没有时整行不占位。
    pub(super) palette: Vec<String>,
    pub(super) indexed_tags: String,
    pub(super) aliases: String,
    pub(super) origin_title: String,
    pub(super) source_url: String,
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
        // 作品信息、自定义行和补全候选跟只读行同一份条目，不跟未保存草稿。
        let shown = shown_fields(&card_entry_metadata(model, row));
        // 作品信息是 ASMR 扩展的面板：扩展插件停用时不画，ASMR 的键照普通自定义字段列出。
        let mut library = if super::inspect_asmr::asmr_entry(model, &shown) {
            super::inspect_library::library_sections(&shown)
        } else {
            Vec::new()
        };
        super::inspect_asmr::attach_cover(&mut library, model);
        let claimed = !library.is_empty();
        Self {
            head: HeadView {
                conflict: inspect.conflict.clone(),
                locked: !inspect.can_edit(),
            },
            rating: inspect.draft_rating(),
            tags: tags_view(model),
            recorded: recorded_view(model, row),
            candidates: super::inspect_asmr::candidates_view(model, &shown),
            custom: custom_fields(&shown, claimed)
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            library,
            comment: inspect.draft_comment().to_string(),
            link: inspect.draft_link().to_string(),
            tag_draft: model.files.tag_draft.clone(),
            lookup_id: super::inspect_asmr::lookup_id_shown(model),
            import_json: super::inspect_asmr::import_draft(model),
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
        inline_field(
            "注释",
            MESSAGE,
            "inspect-comment",
            signals.comment,
            "记录这个文件的用途、状态或上下文。",
            true,
            signals,
            InspectMessage::SetComment,
        ),
        inline_field(
            "链接",
            LINK,
            "inspect-link",
            signals.link,
            "https://example.com",
            false,
            signals,
            InspectMessage::SetLink,
        ),
        rating_row(signals),
        parts::tags_row(signals),
        recorded_grid(signals.recorded),
        each(
            library,
            LibrarySection::clone,
            super::inspect_library::library_card,
        )
        .gap(12.0)
        .visible(move || library.with(|sections| !sections.is_empty())),
        parts::candidate_section(signals),
        each(
            custom,
            |(key, value): &(String, String)| (key.clone(), value.clone()),
            |(key, value)| {
                fact(
                    &key,
                    value,
                    &format!("inspect-custom-{}", style::key_part(&key)),
                    false,
                )
            },
        )
        .gap(12.0)
        .key("inspect-custom")
        .visible(move || custom.with(|rows| !rows.is_empty())),
    );
    let panel = style::top_rule(
        Stack::column(12.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
        12.0,
    );
    widget(panel)
        .children(rows)
        .key("inspect-metadata")
        .into_any()
}

/// 元数据画成自定义行和作品信息用的文字。空值不收，没有的键不补。
fn shown_fields(metadata: &BTreeMap<String, Value>) -> BTreeMap<String, String> {
    metadata
        .iter()
        .map(|(key, value)| (key.clone(), super::inspect::support::display_value(value)))
        .filter(|(_, text)| !text.is_empty())
        .collect()
}

/// 自定义行要画的键值。`claimed` 为真时作品信息区块已经画出它认领的键。
fn custom_fields(
    custom: &BTreeMap<String, String>,
    claimed: bool,
) -> impl Iterator<Item = (&String, &String)> {
    custom.iter().filter(move |(key, _)| {
        !super::inspect::is_reserved_metadata(key)
            && !(claimed && super::inspect_library::claimed_metadata_keys().contains(&key.as_str()))
    })
}

/// 自动保存撞上服务器的新版本：警告色写明冲突，右边是「采用服务器版本」。没有冲突时不占位。
/// 放在注释前面，紧挨着被拒绝写入的字段，不沉到只读事实下面（DESIGN.md：冲突必须靠近字段出现）。
fn conflict_notice(head: Signal<HeadView>) -> AnyView {
    let conflict = head.with_untracked(|head| head.conflict.clone());
    let notice = Stack::bar(8.0)
        .align(AlignSpec::Center)
        .justify(JustifySpec::SpaceBetween)
        .width(LengthSpec::Fill)
        .min_width(LengthSpec::Px(0.0));
    widget(notice)
        .visible(move || head.with(|head| !head.conflict.is_empty()))
        .children((
            widget(ValidationMessage::new(conflict, ValidationIntent::Warning))
                .prop::<String, ValidationTextField>(move || {
                    head.with(|head| head.conflict.clone())
                })
                .key("inspect-conflict"),
            widget(style::styled_button(
                "采用服务器版本",
                None,
                ButtonLook::TOOLBAR,
            ))
            .key("inspect-adopt")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(inspect_message(InspectMessage::AdoptConflict))
            }),
        ))
        .key("inspect-conflict-row")
        .into_any()
}

/// 当前检视目标在文件列表里的那一行。预览页从播放集打开时可能不在列表里。
fn target_row(model: &ShellViewModel) -> Option<&FileRow> {
    let path = model.inspect.target_path.as_deref()?;
    let ctx = FileContext::from_model(model);
    let files = &model.files;
    if let Some(row) = files
        .listed_rows(&ctx)
        .into_iter()
        .find(|row| row.path == path)
    {
        return Some(row);
    }
    // 分类行还没从摘要投影出来时，检视仍认当前目录里的这一条，避免把已经打开的文件说成不在列表里。
    if ctx.smart_folder {
        return None;
    }
    files
        .rows
        .iter()
        .filter(|row| !ctx.category_virtual || row.kind != "directory")
        .find(|row| row.path == path)
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
    let mut input = TextInput::new(draft.current())
        .placeholder(placeholder)
        .disabled(nana_ui::runtime::view::untrack(&locked));
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
                widget(
                    IconGlyph::new(icon)
                        .size(14.0)
                        .role(SemanticColorRole::Muted),
                )
                .key(format!("{key}-icon")),
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
            let (active_now, locked_now) = (
                rating.get_untracked() >= value,
                head.with_untracked(|head| head.locked),
            );
            let glyph = Stack::column(0.0).style(star_glyph_style(active_now));
            widget(
                ListItem::new(format!("{value} 星"))
                    .disabled(locked_now)
                    .style(star_style(active_now, locked_now)),
            )
            .prop::<bool, fields::list_item::disabled>(locked)
            .prop::<NodeStyle, StyleField>(move || star_style(active(), locked()))
            .content(
                widget(glyph).prop::<NodeStyle, StyleField>(move || star_glyph_style(active())),
            )
            .key(format!("inspect-rate-{value}"))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(inspect_message(InspectMessage::SetRating(value)))
            })
            .into_any()
        })
        .collect::<Vec<_>>();
    widget(style::meta_row(false))
        .key("inspect-rating-row")
        .children((
            widget(style::row_label("评分")).key("inspect-rating"),
            widget(Stack::row(4.0).align(AlignSpec::Center))
                .key("inspect-rating-stars")
                .children(stars),
        ))
        .into_any()
}

/// 一颗星的外框：34 方的圆，到达分数时浅强调底；不能编辑时淡到一半。
fn star_style(active: bool, locked: bool) -> NodeStyle {
    let mut style = NodeStyle::default();
    style.radius = None;
    style.background = active.then_some(SemanticColorRole::AccentSoft);
    style.interaction.hovered = SemanticPaint {
        background: Some(SemanticColorRole::AccentSoft),
        ..SemanticPaint::default()
    };
    style.interaction.pressed = style.interaction.hovered;
    style.interaction.disabled = SemanticPaint::default();
    let layout = Arc::make_mut(&mut style.layout);
    for edge in [
        &mut layout.width,
        &mut layout.height,
        &mut layout.min_width,
        &mut layout.min_height,
    ] {
        *edge = Some(LengthSpec::Px(34.0));
    }
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.opacity = locked.then_some(0.5);
    style.radius_px(999.0)
}

/// 星形图标所在的一层：铺满外框，自绘星形。
fn star_glyph_style(active: bool) -> NodeStyle {
    Stack::column(0.0)
        .width(LengthSpec::Fill)
        .height(LengthSpec::Fill)
        .painter(StarPainter { active })
        .node_style()
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
        let color = if self.active {
            STAR_GOLD
        } else {
            cx.color(SemanticColorRole::Muted)
        };
        cx.icon(rect, STAR, color);
    }

    fn paint_key(&self) -> u64 {
        u64::from(self.active)
    }
}

/// Vue `aliasPaths`：去掉和当前路径相同的位置，用「，」连起来。
/// 读列表行上保存的那一份。继续加载会换掉上一页目录快照，智能文件夹结果也不在那一页里。
fn alias_locations(row: &FileRow) -> String {
    row.alias_paths
        .iter()
        .filter(|path| path.as_str() != row.path)
        .cloned()
        .collect::<Vec<_>>()
        .join("，")
}

/// 添加到资源库到来源链接的只读行。第一行不画上边线；没有内容的行不占位。
///
/// 读 [`recorded_metadata`]：详情卡叠对得上的素材详情，预览页只用列表或播放集条目。
/// 文件修改时间在元数据里没有时退回这一条自己的 `modifiedAt`，不用素材摘要上的修改时间。
fn recorded_view(model: &ShellViewModel, row: Option<&FileRow>) -> RecordedView {
    let (metadata, modified_at) = recorded_metadata(model, row);
    let text = |key: &str| metadata_string(&metadata, key).trim().to_string();
    let file_modified = {
        let meta = metadata_string(&metadata, "fileModifiedAt");
        if meta.is_empty() { modified_at } else { meta }
    };
    let width = raw_number(&metadata, "width");
    let height = raw_number(&metadata, "height");
    let dimensions = match (width, height) {
        (Some(width), Some(height)) if width > 0.0 && height > 0.0 => {
            format!("{} × {}", plain(width), plain(height))
        }
        _ => "未记录".into(),
    };
    RecordedView {
        added: local_time::format_or(&text("addedToLibraryAt"), "未记录"),
        created: local_time::format_or(&text("fileCreatedAt"), "未记录"),
        file_modified: local_time::format_or(&file_modified, "未记录"),
        dimensions,
        original: super::inspect_library::format_original_size(raw_number(
            &metadata,
            "originalSizeBytes",
        )),
        // 详情元数据优先覆盖列表；没有详情时才回退到列表行持久色板。
        // 预览取样色板只存于 inspect，不进入此行。
        palette: {
            let detail_palette = super::palette::from_metadata_map(&metadata);
            if !detail_palette.is_empty() {
                detail_palette
            } else {
                row.filter(|row| !row.palette.is_empty())
                    .map(|row| row.palette.clone())
                    .unwrap_or_default()
            }
        },
        indexed_tags: row.map(|row| row.tags.join("，")).unwrap_or_default(),
        aliases: row.map(alias_locations).unwrap_or_default(),
        origin_title: text("originTitle"),
        source_url: text("sourceUrl"),
    }
}

/// 只读行用的元数据和修改时间。元数据见 [`card_entry_metadata`]。
/// 文件修改时间在元数据里没有时退回这一条自己的 `modifiedAt`。
fn recorded_metadata(
    model: &ShellViewModel,
    row: Option<&FileRow>,
) -> (BTreeMap<String, Value>, String) {
    if !super::files_view::previewing(model) && row.is_none() {
        eprintln!("Nana 详情卡只读行找不到列表条目，不拿素材详情硬填");
    }
    (
        card_entry_metadata(model, row),
        row.map(|row| row.modified_at.clone()).unwrap_or_default(),
    )
}

/// 详情卡作品信息、自定义行和只读行共用的元数据。封面按钮也读这一份。
///
/// 预览页照 Vue `previewFileEntry`：当前列表优先，没有再看播放集条目，不叠素材详情，也不读编辑草稿。
/// 详情卡照 `currentFileEntry`（`withActiveAssetMetadata`）：素材 id 对得上时详情盖到列表上，同键以详情为准；
/// 对不上或还没读回时只用列表。没有的键不补。
fn card_entry_metadata(model: &ShellViewModel, row: Option<&FileRow>) -> BTreeMap<String, Value> {
    if super::files_view::previewing(model) {
        if let Some(row) = row {
            return row.metadata.clone();
        }
        let path = model.inspect.target_path.as_deref().unwrap_or("");
        return playlist_entry_metadata(model, path);
    }
    let Some(row) = row else {
        return BTreeMap::new();
    };
    let mut metadata = row.metadata.clone();
    if let Some(overlay) = model.files.detail_metadata(row.asset_id.as_deref()) {
        for (key, value) in overlay {
            metadata.insert(key.clone(), value.clone());
        }
    }
    metadata
}

/// 作品信息上的封面按钮要读的那一份元数据，和 [`card_entry_metadata`] 同一条。
pub(super) fn current_card_metadata(model: &ShellViewModel) -> BTreeMap<String, Value> {
    card_entry_metadata(model, target_row(model))
}

/// 播放集条目自己的元数据。正在放的是另一张时，不用打开着的这一页，也不拿素材详情补。
fn playlist_entry_metadata(model: &ShellViewModel, path: &str) -> BTreeMap<String, Value> {
    let listed = model.player.listed.as_ref();
    let playing = model
        .player
        .queue
        .iter()
        .find(|item| !item.transient)
        .map(|item| item.playlist_id.as_str());
    let page = match (playing, listed) {
        (Some(playing), Some(page)) if page.playlist.playlist_id != playing => {
            eprintln!("Nana 预览只读行不拿另一张播放集的元数据：{path}");
            return BTreeMap::new();
        }
        (_, Some(page)) => page,
        (Some(_), None) => {
            eprintln!("Nana 正在播放的播放集没有条目元数据，只读行不拿素材详情补：{path}");
            return BTreeMap::new();
        }
        (None, None) => return BTreeMap::new(),
    };
    let Some(item) = page.items.iter().find(|item| item.path == path) else {
        return BTreeMap::new();
    };
    match item.metadata.as_ref() {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        Some(other) => {
            eprintln!("Nana 播放集条目的元数据不是对象，只读行不采用：{path}：{other}");
            BTreeMap::new()
        }
        None => BTreeMap::new(),
    }
}

/// Vue `metadataString`：只有字符串算数，别的类型当成空，不退回另一份元数据。
fn metadata_string(metadata: &BTreeMap<String, Value>, key: &str) -> String {
    match metadata.get(key) {
        Some(Value::String(value)) => value.clone(),
        Some(other) => {
            eprintln!("Nana 元数据 {key} 不是字符串：{other}");
            String::new()
        }
        None => String::new(),
    }
}

/// Vue `metadataRawNumber`：只认有限数字。键在但类型不对时记日志，不当成列表上的旧值。
fn raw_number(metadata: &BTreeMap<String, Value>, key: &str) -> Option<f64> {
    match metadata.get(key) {
        Some(Value::Number(number)) => {
            number
                .as_f64()
                .filter(|value| value.is_finite())
                .or_else(|| {
                    eprintln!("Nana 元数据 {key} 不是有限数字");
                    None
                })
        }
        Some(other) => {
            eprintln!("Nana 元数据 {key} 不是数字：{other}");
            None
        }
        None => None,
    }
}

/// 测试读当前文件的只读行。
#[cfg(test)]
pub(super) fn recorded_rows(model: &ShellViewModel) -> RecordedView {
    recorded_view(model, target_row(model))
}

/// 只读行：五个事实一直在，调色板、索引标签、多归属位置、来源标题和来源链接有内容才占位。
fn recorded_grid(recorded: Signal<RecordedView>) -> AnyView {
    let read = move |pick: fn(&RecordedView) -> String| move || recorded.with(|view| pick(view));
    let shown = move |pick: fn(&RecordedView) -> bool| move || recorded.with(|view| pick(view));
    let palette = dynamic(
        move || recorded.with(|view| view.palette.clone()),
        |colors: &Vec<String>| {
            super::palette::metadata_chips(colors, "inspect-metadata-palette")
                .unwrap_or_else(|| ().into_any())
        },
    );
    let always = || true;
    let rows = (
        fact_row(
            "添加到资源库",
            read(|view| view.added.clone()),
            "inspect-recorded-added",
            true,
            always,
        ),
        fact_row(
            "创建时间",
            read(|view| view.created.clone()),
            "inspect-recorded-created",
            false,
            always,
        ),
        fact_row(
            "文件修改时间",
            read(|view| view.file_modified.clone()),
            "inspect-recorded-modified",
            false,
            always,
        ),
        fact_row(
            "尺寸",
            read(|view| view.dimensions.clone()),
            "inspect-recorded-size",
            false,
            always,
        ),
        fact_row(
            "原始大小",
            read(|view| view.original.clone()),
            "inspect-recorded-bytes",
            false,
            always,
        ),
        widget(style::meta_row(false))
            .visible(shown(|view| style::any_color(&view.palette)))
            .children((
                widget(style::row_label("调色板")).key("inspect-palette-label"),
                palette,
            ))
            .key("inspect-palette"),
        fact_row(
            "索引标签",
            read(|view| view.indexed_tags.clone()),
            "inspect-indexed-tags",
            false,
            shown(|view| !view.indexed_tags.is_empty()),
        ),
        right_fact(
            "多归属位置",
            read(|view| view.aliases.clone()),
            "inspect-alias-paths",
            shown(|view| !view.aliases.is_empty()),
        ),
        right_fact(
            "来源标题",
            read(|view| view.origin_title.clone()),
            "inspect-origin-title",
            shown(|view| !view.origin_title.is_empty()),
        ),
        source_link_row(recorded, shown(|view| !view.source_url.is_empty())),
    );
    widget(
        Stack::column(12.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children(rows)
    .key("inspect-recorded")
    .into_any()
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
fn right_fact(
    label: &str,
    value: impl Fn() -> String + Send + Clone + 'static,
    key: &str,
    shown: impl Fn() -> bool + Send + 'static,
) -> AnyView {
    let mut node = style::value(nana_ui::runtime::view::untrack(&value));
    node.style.text_horizontal_alignment = nana_ui::runtime::TextHorizontalAlignment::End;
    widget(style::meta_row(false))
        .visible(shown)
        .children((
            widget(style::row_label(label.to_string())).key(format!("{key}-label")),
            widget(node)
                .prop::<String, fields::text::value>(value)
                .key(format!("{key}-value")),
        ))
        .key(key.to_string())
        .into_any()
}

/// 来源链接：右对齐的地址，后面是「打开链接」「复制链接」两个 26 方的描边按钮。
/// 悬停提示和 Vue `title` 一样。只有 http、https、mailto、tel 才能打开（`host_api::openable_url`）。
/// 点下时现读地址。没有地址时不占位。
fn source_link_row(
    recorded: Signal<RecordedView>,
    shown: impl Fn() -> bool + Send + 'static,
) -> AnyView {
    let url = move || recorded.with(|view| view.source_url.clone());
    let mut value = style::value(recorded.with_untracked(|view| view.source_url.clone()));
    value.style.text_horizontal_alignment = nana_ui::runtime::TextHorizontalAlignment::End;
    let openable = move || recorded.with(|view| openable_url(&view.source_url));
    let actions = widget(Stack::row(4.0).grow(0.0).shrink(0.0))
        .key("inspect-source-actions")
        .children((
            source_action(EXTERNAL_LINK, "inspect-source-open", "打开链接")
                .prop::<bool, fields::icon_button::disabled>(move || !openable())
                .on_cx(move |_, _: &Activate, cx| {
                    let url = recorded.with_untracked(|view| view.source_url.clone());
                    if openable_url(&url) {
                        cx.dispatch_program_all(ShellMessage::Input(
                            InputMessage::OpenExternalUrl { url },
                        ));
                    } else {
                        eprintln!("Nana 来源链接不能打开：{url}");
                    }
                }),
            source_action(COPY, "inspect-source-copy", "复制链接").on_cx(
                move |_, _: &Activate, cx| {
                    let url = recorded.with_untracked(|view| view.source_url.clone());
                    if url.is_empty() {
                        eprintln!("Nana 没有可复制的来源链接");
                        return;
                    }
                    cx.dispatch_program_all(ShellMessage::Files(FilesMessage::CopyText(url)));
                },
            ),
        ));
    widget(style::meta_row(false))
        .visible(shown)
        .children((
            widget(style::row_label("来源链接")).key("inspect-source-label"),
            widget(
                Stack::row(8.0)
                    .align(AlignSpec::Start)
                    .justify(JustifySpec::End)
                    .width(LengthSpec::Fill)
                    .min_width(LengthSpec::Px(0.0)),
            )
            .children((
                widget(value)
                    .prop::<String, fields::text::value>(url)
                    .key("inspect-source-value"),
                actions,
            ))
            .key("inspect-source-body"),
        ))
        .key("inspect-source")
        .into_any()
}

/// 来源链接旁的 26 方按钮。`title` 是无障碍名和悬停提示，对应 Vue `title`。
///
/// 平时弱色图标、`--bg` 底、`--border-soft` 细边。悬停时图标改正文色、边框改强调色
/// （`.file-metadata-card__source-action:hover`）。打不开的链接停用时，图标和边框按 0.45
/// 盖在按钮底色上，和 Vue `opacity: 0.45` 同一深浅，不再整枚淡一层。
fn source_action(icon: Icon, key: &str, title: &'static str) -> El<IconButton> {
    let mut button = IconButton::new(icon, title)
        .with_tooltip(title)
        .colors_from_style();
    button.style.foreground = Some(SemanticColorRole::Muted);
    button.style.background = Some(SemanticColorRole::Background);
    button.style.border = None;
    button.style.radius = Some(RadiusTier::Sm);
    button.style.control_height = None;
    button.style.control_padding_x = None;
    button.style = button
        .style
        .clone()
        .outline(SemanticColorRole::BorderSoft, 1.0);
    let hover = SemanticPaint {
        foreground: Some(SemanticColorRole::Text),
        border: Some(SemanticColorRole::Accent),
        background: Some(SemanticColorRole::Background),
        ..SemanticPaint::default()
    };
    button.style.interaction.hovered = hover;
    button.style.interaction.pressed = hover;
    button.style.interaction.disabled = SemanticPaint {
        foreground_mix: Some(nana_ui_core::SemanticColorMix::new(
            SemanticColorRole::Muted,
            SemanticColorRole::Background,
            crate::theme_map::DISABLED_OPACITY,
        )),
        border_mix: Some(nana_ui_core::SemanticColorMix::new(
            SemanticColorRole::BorderSoft,
            SemanticColorRole::Background,
            crate::theme_map::DISABLED_OPACITY,
        )),
        background: Some(SemanticColorRole::Background),
        ..SemanticPaint::default()
    };
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(26.0));
    layout.height = Some(LengthSpec::Px(26.0));
    layout.min_width = Some(LengthSpec::Px(26.0));
    layout.min_height = Some(LengthSpec::Px(26.0));
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    layout.padding_top = Some(LengthSpec::Px(0.0));
    layout.padding_bottom = Some(LengthSpec::Px(0.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    widget(button).key(key.to_string())
}

/// 标签菜单里的一个仓库标签分组：分组 id、名称和可加的标签。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TagGroupOption {
    id: String,
    name: String,
    tags: Vec<String>,
}

/// 标签组的投影：展开状态按用户点过的来；菜单里先列仓库的标签分组，再列已有标签（按输入过滤）。
fn tags_view(model: &ShellViewModel) -> TagsView {
    let inspect = &model.inspect;
    let tags = inspect.draft_tags().to_vec();
    let path = inspect.target_path.clone().unwrap_or_default();
    let expanded = model.files.tags_expanded(&path, !tags.is_empty());
    let trimmed = model.files.tag_draft.trim().to_string();
    let keyword = trimmed.to_lowercase();
    let menu_open = inspect.tag_menu_open();
    // 候选要扫整个资源库的标签，只在菜单开着时算；菜单收着时画面上没有它们。
    let (groups, options) = if menu_open {
        let options = available_tags(model)
            .into_iter()
            .filter(|tag| {
                !tags.contains(tag) && (keyword.is_empty() || tag.to_lowercase().contains(&keyword))
            })
            .take(18)
            .collect();
        let groups = repository_tag_groups(model)
            .iter()
            .map(|group| TagGroupOption {
                id: group.tag_group_id.clone(),
                name: group.name.clone(),
                tags: group
                    .tags
                    .iter()
                    .filter(|tag| !tag.trim().is_empty() && !tags.contains(tag))
                    .cloned()
                    .collect(),
            })
            .filter(|group| !group.tags.is_empty())
            .collect();
        (groups, options)
    } else {
        (Vec::new(), Vec::new())
    };
    let can_create = !trimmed.is_empty() && !tags.iter().any(|tag| tag == &trimmed);
    let add_label = format!(
        "添加“{}”",
        if trimmed.is_empty() {
            "新标签"
        } else {
            trimmed.as_str()
        }
    );
    TagsView {
        path,
        expanded,
        menu_open,
        groups,
        options,
        can_create,
        add_label,
        tags,
    }
}

/// 当前资源库的摘要已经读到时，它的标签分组；还是别的仓库的摘要时没有。
fn repository_tag_groups(
    model: &ShellViewModel,
) -> &[crate::backend::services::repository::RepositoryTagGroup] {
    let search = &model.inspect.search_ui;
    let active = model.workspace.active_repo_id.as_deref();
    if active.is_some() && search.facets_repo.as_deref() == active {
        &search.tag_groups
    } else {
        &[]
    }
}

/// 整个资源库用过的标签（仓库摘要里每项素材的索引标签）加上搜索结果的标签，去空白、去重，按 zh-CN 排序。
/// 和筛选栏的标签候选同一来源（Vue `rebuildFilterOptions` 的 `tagFilterOptions`），不只看当前目录读到的那一批。
fn available_tags(model: &ShellViewModel) -> Vec<String> {
    let search = &model.inspect.search_ui;
    let active = model.workspace.active_repo_id.as_deref();
    let summary = (active.is_some() && search.facets_repo.as_deref() == active)
        .then_some(&search.facets)
        .into_iter()
        .flatten()
        .flat_map(|facet| &facet.tags);
    let results = model.inspect.results.iter().flat_map(|row| &row.tags);
    let unique = summary
        .chain(results)
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    let mut tags = unique.into_iter().map(str::to_string).collect::<Vec<_>>();
    tags.sort_by(|left, right| super::search_collate::compare_zh(left, right));
    tags
}

fn plain(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn inspect_message(message: InspectMessage) -> ShellMessage {
    ShellMessage::Inspect(message)
}

/// 测试读详情卡上的作品信息行、自定义行和封面地址。没画出来的是空。
#[cfg(test)]
pub(super) fn work_and_custom_rows(
    model: &ShellViewModel,
) -> (Vec<(String, String)>, Vec<(String, String)>, Option<String>) {
    let view = MetadataView::project(model);
    let works = view
        .library
        .iter()
        .find(|section| section.key == "inspect-asmr-works");
    let rows = works
        .map(|section| {
            section
                .rows
                .iter()
                .filter_map(|row| match row {
                    super::inspect_library::LibraryRow::Value { label, value, .. } => {
                        Some(((*label).to_string(), value.clone()))
                    }
                    super::inspect_library::LibraryRow::Body(_) => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let cover = works.and_then(|section| section.cover.as_ref().map(|button| button.url.clone()));
    (rows, view.custom, cover)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nana_ui::runtime::{Entity, IconButton, SemanticColorRole, StableNodeId};
    use serde_json::Value;

    use super::custom_fields;
    use crate::shell::input::InputMessage;
    use crate::shell::view_harness::ShellHarness;
    use crate::shell::{FilesMessage, ShellMessage};

    /// 通用字段、后端种下的系统字段都不进自定义行；作品信息认领的键只在区块画出来时才让出。
    #[test]
    fn custom_rows_skip_reserved_and_seeded_keys() {
        let mut custom = BTreeMap::new();
        for key in [
            "comment",
            "note",
            "link",
            "rating",
            "tagGroups",
            "width",
            "height",
            "originalSizeBytes",
            "addedToLibraryAt",
            "fileCreatedAt",
            "fileModifiedAt",
            "palette",
            "thumbnailPalette",
            "originTitle",
            "sourceUrl",
            "title",
            "type",
            "favorite",
            "color",
            "artist",
            "workTitle",
        ] {
            custom.insert(key.to_string(), "x".to_string());
        }
        let shown = |claimed| {
            custom_fields(&custom, claimed)
                .map(|(key, _)| key.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(shown(false), ["artist", "workTitle"]);
        assert_eq!(shown(true), ["artist"]);
    }

    /// F7：标签菜单先列仓库的标签分组（去掉草稿里已有的，空了的分组不列，不按输入过滤），再列整个资源库用过的标签：
    /// 仓库摘要里每项素材的标签，不只是当前目录读到的那几行，按 zh-CN 排序、按输入过滤。菜单收着时不算候选。
    #[test]
    fn tag_menu_offers_repository_groups_and_library_wide_tags() {
        use crate::backend::services::repository::RepositoryTagGroup;
        use crate::shell::{FilesMessage, ShellMessage};
        let mut model = crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(name, _)| *name == "files-selected-metadata")
            .map(|(_, model)| model)
            .expect("files-selected-metadata 场景");
        assert_eq!(model.inspect.draft_tags(), ["封面", "参考"]);
        model
            .inspect
            .search_ui
            .facets
            .push(super::super::inspect::AssetFacet {
                tags: vec!["远处的标签".into(), "动画".into()],
                extension: "png".into(),
            });
        let group = |id: &str, name: &str, tags: &[&str]| RepositoryTagGroup {
            tag_group_id: id.into(),
            name: name.into(),
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
        };
        model.inspect.search_ui.tag_groups = vec![
            group("g1", "角色", &["主角", "封面"]),
            group("g2", "已用完", &["参考"]),
        ];
        assert!(
            super::tags_view(&model).options.is_empty(),
            "菜单收着时不算候选"
        );

        model.reduce(ShellMessage::Files(FilesMessage::ToggleTagMenu));
        let view = super::tags_view(&model);
        assert_eq!(
            view.groups,
            [super::TagGroupOption {
                id: "g1".into(),
                name: "角色".into(),
                tags: vec!["主角".into()]
            }]
        );
        assert_eq!(
            view.options,
            ["动画", "远处的标签"],
            "整库的标签，不在当前目录也列出"
        );

        model.reduce(ShellMessage::Files(FilesMessage::SetTagDraft("远".into())));
        let view = super::tags_view(&model);
        assert_eq!(view.options, ["远处的标签"]);
        assert_eq!(view.groups.len(), 1, "标签分组不按输入过滤");
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
        let row = model
            .files
            .rows
            .iter_mut()
            .find(|row| row.path == "cover.png")
            .expect("cover.png");
        for (key, value) in [
            ("title", "cover.png"),
            ("type", "png"),
            ("color", "#c7a566"),
            ("artist", "Momo"),
        ] {
            row.metadata.insert(key.into(), Value::String(value.into()));
        }
        row.metadata.insert("favorite".into(), Value::Bool(false));
        let document = crate::acceptance_document_for_model(model).expect("生产文档");
        let nodes = document
            .context()
            .world()
            .project_accessibility(document.document());
        let labelled = |label: &str| {
            nodes
                .iter()
                .filter(|node| node.label.as_deref() == Some(label))
                .count()
        };
        for key in [
            "comment",
            "link",
            "rating",
            "width",
            "height",
            "originalSizeBytes",
            "addedToLibraryAt",
            "fileCreatedAt",
            "title",
            "type",
            "favorite",
            "color",
        ] {
            assert_eq!(labelled(key), 0, "保留字段 {key} 不该按原始键名再列一行");
        }
        assert_eq!(labelled("尺寸"), 1);
        assert_eq!(labelled("1920 × 1080"), 1);
        assert_eq!(labelled("artist"), 1, "真正的自定义字段照常列出");
        assert_eq!(labelled("Momo"), 1);
    }

    /// 多归属位置跟正在看的列表条目走。智能文件夹的查询结果不在上一页目录里，
    /// 继续加载也会把 `browser_entries` 换成后面那一页；两种情况下位置都还在。
    #[test]
    fn alias_locations_follow_the_list_entry_not_the_latest_directory_page() {
        let mut model = crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(name, _)| *name == "files-selected-metadata")
            .map(|(_, model)| model)
            .expect("files-selected-metadata 场景");
        let mut entry = model
            .files
            .rows
            .iter()
            .find(|row| row.path == "cover.png")
            .expect("cover.png")
            .clone();
        entry.alias_paths = vec![
            "cover.png".into(),
            "albums/cover.png".into(),
            "sets/cover.png".into(),
        ];
        model.inspect.target_path = Some("cover.png".into());
        model.files.rows.clear();
        model.files.virtual_rows = vec![entry];
        model.workspace.panel = super::super::workspace::WorkspacePanel::SmartFolder;
        // 上一页目录快照已经不在了。旧逻辑从这里找位置，会把这一行写成空。
        model.browser_entries.clear();

        let row = super::target_row(&model);
        let recorded = super::recorded_view(&model, row);
        assert_eq!(recorded.aliases, "albums/cover.png，sets/cover.png");
    }

    /// 来源链接两个按钮照 Vue `title`：悬停提示是「打开链接」「复制链接」。
    /// 能开的地址点「打开链接」交给系统，点「复制链接」写入剪贴板。
    /// `javascript:` 打不开，按钮停用，点下去不发打开。
    #[test]
    fn source_link_buttons_show_vue_titles_and_open_or_copy() {
        let https = source_link_model("https://example.com/cover");
        let mut harness = ShellHarness::mount_at(https, 1200.0, 2000.0);
        let open = harness.node("打开链接");
        let copy = harness.node("复制链接");
        assert_eq!(tooltip(&harness, open).as_deref(), Some("打开链接"));
        assert_eq!(tooltip(&harness, copy).as_deref(), Some("复制链接"));
        assert!(
            hover_uses_text_and_accent(&harness, open),
            "悬停应把图标改正文色、边框改强调色"
        );
        assert!(!disabled(&harness, open), "https 链接可以打开");

        let (x, y) = harness.center(copy);
        let messages = harness.click_messages(x, y);
        assert!(
            messages.iter().any(|message| matches!(
                message,
                ShellMessage::Files(FilesMessage::CopyText(url)) if url == "https://example.com/cover"
            )),
            "复制链接应写入这条地址，实际 {} 条消息",
            messages.len()
        );
        let (x, y) = harness.center(open);
        let messages = harness.click_messages(x, y);
        assert!(
            messages.iter().any(|message| matches!(
                message,
                ShellMessage::Input(InputMessage::OpenExternalUrl { url }) if url == "https://example.com/cover"
            )),
            "打开链接应交给系统，实际 {} 条消息",
            messages.len()
        );

        let mut blocked =
            ShellHarness::mount_at(source_link_model("javascript:alert(1)"), 1200.0, 2000.0);
        let open = blocked.node("打开链接");
        assert!(disabled(&blocked, open), "javascript: 不能打开");
        assert_eq!(tooltip(&blocked, open).as_deref(), Some("打开链接"));
        let (x, y) = blocked.center(open);
        let messages = blocked.click_messages(x, y);
        assert!(
            messages.iter().all(|message| !matches!(
                message,
                ShellMessage::Input(InputMessage::OpenExternalUrl { .. })
            )),
            "停用的打开链接不应发出打开，实际 {} 条消息",
            messages.len()
        );
        let copy = blocked.node("复制链接");
        assert!(!disabled(&blocked, copy), "复制不受协议限制");
        let (x, y) = blocked.center(copy);
        let messages = blocked.click_messages(x, y);
        assert!(
            messages.iter().any(|message| matches!(
                message,
                ShellMessage::Files(FilesMessage::CopyText(url)) if url == "javascript:alert(1)"
            )),
            "打不开的链接仍可复制，实际 {} 条消息",
            messages.len()
        );
    }

    fn source_link_model(url: &str) -> crate::shell::ShellViewModel {
        let mut model = crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(name, _)| *name == "files-selected-metadata")
            .map(|(_, model)| model)
            .expect("files-selected-metadata 场景");
        let row = model
            .files
            .rows
            .iter_mut()
            .find(|row| row.path == "cover.png")
            .expect("cover.png");
        row.metadata.insert(
            "sourceUrl".into(),
            serde_json::Value::String(url.to_string()),
        );
        model
    }

    fn tooltip(harness: &ShellHarness, id: StableNodeId) -> Option<String> {
        harness
            .document()
            .context()
            .read(Entity::<IconButton>::from_stable_id(id), |button| {
                button.tooltip.as_ref().map(|tip| tip.label.to_string())
            })
            .unwrap_or_else(|_| panic!("来源链接按钮应该是图标按钮"))
    }

    fn hover_uses_text_and_accent(harness: &ShellHarness, id: StableNodeId) -> bool {
        harness
            .document()
            .context()
            .read(Entity::<IconButton>::from_stable_id(id), |button| {
                let hover = &button.style.interaction.hovered;
                hover.foreground == Some(SemanticColorRole::Text)
                    && hover.border == Some(SemanticColorRole::Accent)
            })
            .unwrap_or(false)
    }

    fn disabled(harness: &ShellHarness, id: StableNodeId) -> bool {
        harness
            .document()
            .context()
            .world()
            .accessibility(id)
            .is_some_and(|state| state.disabled)
    }
}
