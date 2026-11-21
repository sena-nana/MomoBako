//! 已选文件的元数据编辑。
//!
//! 顺序：注释、链接两行带图标的输入，评分五颗星，可折叠的标签组，然后是
//! 添加到资源库、创建时间、文件修改时间、尺寸、原始大小、调色板、索引标签、多归属位置、
//! 来源标题和来源链接。详情卡这几行：素材编号对得上才把详情叠到列表上。
//! 作品信息、自定义行和补全候选跟这几行同一份元数据，不跟编辑器里还没保存的草稿。
//! 预览页只读预览条目，不拿素材详情补列表里没有的值。索引标签和多归属位置仍只读列表条目。
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
    AnyView, El, IntoView, Signal, css, dynamic, each, fields, signal, widget,
};
use nana_ui::runtime::{
    Activate, AlignSpec, IconButton, IconGlyph, JustifySpec, LengthSpec, ListItem, NodeStyle,
    PaintContext, Painter, RadiusTier, SemanticColorRole, SemanticPaint, Stack, TextChanged,
    TextInput, ValidationIntent, ValidationMessage,
};
use nana_ui_core::{Icon, SpacingStep};
use serde_json::Value;

use crate::theme_map::tiers::{self, Edge, Hairline};

use crate::host_api::openable_url;

use super::files::{FileContext, FileRow, FilesMessage, local_time};
use super::files_view::bind::{DraftField, StyleField, ValidationTextField, fact_row};
use super::files_view::style::{self, ButtonLook};
use super::input::InputMessage;
use super::inspect::InspectMessage;
use super::inspect_asmr::CandidatesView;
use super::inspect_library::LibrarySection;
use super::{ShellMessage, ShellViewModel};

mod parts;
mod recorded;

pub(super) use recorded::current_card_metadata;
#[cfg(test)]
pub(super) use recorded::recorded_rows;
use recorded::*;

/// 点亮的星：主题的警告色（金色一档）。
const STAR_GOLD: SemanticColorRole = SemanticColorRole::Warning;
/// 星形图标 18px。主题没有图标尺寸档，照写像素。
const STAR_ICON: f32 = 18.0;
/// 一颗星的外框边长 34：控件档里没有这个高度，照写像素。
const STAR_BOX: f32 = 34.0;

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
    /// 仓库的标签分组里还没加进草稿的标签，空的分组不列，不按输入过滤。
    groups: Vec<TagGroupOption>,
    /// 整个资源库（仓库摘要）和搜索结果里用过、还没加进草稿的标签，按 zh-CN 排序、按输入过滤，最多 18 个
    /// （和筛选栏的标签候选同一批）。
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
        .css(css! { gap: theme(space-xl); })
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
        .css(css! { gap: theme(space-xl); })
        .key("inspect-custom")
        .visible(move || custom.with(|rows| !rows.is_empty())),
    );
    let panel = style::top_rule(
        style::column(SpacingStep::Xl)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
        SpacingStep::Xl,
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
    let notice = Stack::bar(0.0)
        .gap_theme(SpacingStep::Md)
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
    layout.height = Some(LengthSpec::Px(
        style::TOOLBAR_HEIGHT - tiers::hairline_px() * 2.0,
    ));
    tiers::padding(layout, &Edge::X, SpacingStep::Lg);
    layout.border_width = Some(0.0);
    layout.theme.border_width = None;
    let frame = Hairline::hairline(
        style::row(SpacingStep::Md)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .with_layout(|layout| {
                tiers::padding(layout, &Edge::X, SpacingStep::Lg);
                layout.padding_top = Some(LengthSpec::Px(0.0));
                layout.padding_bottom = Some(LengthSpec::Px(0.0));
            })
            .min_height(LengthSpec::Px(style::TOOLBAR_HEIGHT))
            .surface(SemanticColorRole::Background),
        SemanticColorRole::BorderSoft,
    )
    .radius(RadiusTier::Lg);
    widget(style::meta_row(first))
        .key(format!("{key}-row"))
        .children((
            widget(style::row_label(label)).key(format!("{key}-label")),
            widget(frame).key(format!("{key}-field")).children((
                widget(
                    IconGlyph::new(icon)
                        .size(style::TOOLBAR_ICON)
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

/// 评分：五颗 34 方的星，间距 `Xs`。到达当前分数的星是金色、浅强调底。再点同一颗回到 0。
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
            widget(style::row(SpacingStep::Xs).align(AlignSpec::Center))
                .key("inspect-rating-stars")
                .children(stars),
        ))
        .into_any()
}

/// 一颗星的外框：34 方的圆，到达分数时浅强调底；不能编辑时淡到主题的停用档。
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
        *edge = Some(LengthSpec::Px(STAR_BOX));
    }
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.opacity = locked.then_some(crate::theme_map::tiers::DISABLED_OPACITY);
    style.radius(RadiusTier::Full)
}

/// 星形图标所在的一层：铺满外框，自绘星形。
fn star_glyph_style(active: bool) -> NodeStyle {
    Stack::column(0.0)
        .width(LengthSpec::Fill)
        .height(LengthSpec::Fill)
        .painter(StarPainter { active })
        .node_style()
}

/// 星形图标：18px 描边星，到达分数时警告色（金色），否则弱化色。
#[derive(Clone, Copy, Debug)]
struct StarPainter {
    active: bool,
}

impl Painter for StarPainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let bounds = cx.bounds();
        let rect = nana_ui::runtime::LayoutBox {
            x: bounds.x + (bounds.width - STAR_ICON) / 2.0,
            y: bounds.y + (bounds.height - STAR_ICON) / 2.0,
            width: STAR_ICON,
            height: STAR_ICON,
        };
        let color = if self.active {
            STAR_GOLD
        } else {
            SemanticColorRole::Muted
        };
        cx.icon(rect, STAR, color);
    }

    fn paint_key(&self) -> u64 {
        u64::from(self.active)
    }
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
/// 和筛选栏的标签候选同一来源，不只看当前目录读到的那一批。
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
mod tests;
