//! 已选文件的元数据编辑，对应 Vue `FileMetadataEditor.vue`。
//!
//! 顺序和 Vue 一致：注释、链接两行带图标的输入，评分五颗星，可折叠的标签组，然后是
//! 添加到资源库、创建时间、文件修改时间、尺寸、原始大小、调色板、索引标签、多归属位置、
//! 来源标题和来源链接。改动先进草稿，260ms 后自动保存；画面上不放保存按钮。
//! 自动保存撞上服务器的新版本时，注释前面先写出冲突和「采用服务器版本」，草稿留在输入框里。
//! 文件详情和预览页共用这一块。

use std::collections::BTreeMap;
use std::sync::Arc;

use nana_ui::icons_tabler::{CHEVRON_DOWN, CHEVRON_RIGHT, COPY, EXTERNAL_LINK, LINK, MESSAGE, PLUS, STAR};
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, IconGlyph, JustifySpec, LengthSpec, ListItem, NodeStyle, PaintContext, Painter, RadiusTier,
    SemanticColorRole, SemanticPaint, Stack, TextChanged, TextInput, TextSubmitted, ValidationIntent, ValidationMessage,
};
use nana_ui_core::Icon;
use serde_json::Value;

use super::files::{local_time, FileContext, FileRow, FilesMessage};
use super::files_view::style::{self, ButtonLook};
use super::input::InputMessage;
use super::inspect::InspectMessage;
use super::{ShellMessage, ShellViewModel};

/// 点击时现做的壳层消息。`ShellMessage` 不能克隆。
type Action = Arc<dyn Fn() -> ShellMessage + Send + Sync>;

/// Vue `.file-metadata-card__star.is-active` 的金色。浅色深色主题同一个值。
const STAR_GOLD: [f32; 4] = [212.0 / 255.0, 166.0 / 255.0, 61.0 / 255.0, 1.0];

/// 元数据编辑块：上边一条发丝线，块内各行间距 12。
pub(super) fn metadata_panel(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let row = target_row(model);
    let locked = !inspect.can_edit();
    let mut rows: Vec<AnyView> = conflict_notice(&inspect.conflict).into_iter().collect();
    rows.extend([
        inline_field("注释", MESSAGE, "inspect-comment", inspect.draft_comment(), "记录这个文件的用途、状态或上下文。", locked, true, InspectMessage::SetComment),
        inline_field("链接", LINK, "inspect-link", inspect.draft_link(), "https://example.com", locked, false, InspectMessage::SetLink),
        rating_row(inspect.draft_rating(), locked),
        tags_row(model, locked),
        recorded_grid(model, row.as_ref()),
    ]);
    let custom = super::inspect_asmr::display_custom(model);
    let library = super::inspect_library::library_sections(&custom);
    let claimed = !library.is_empty();
    rows.extend(library);
    rows.extend(super::inspect_asmr::candidate_section(model, &custom));
    let extra = custom
        .iter()
        .filter(|(key, _)| !(claimed && super::inspect_library::claimed_metadata_keys().contains(&key.as_str())))
        .map(|(key, value)| fact(key, value.clone(), &format!("inspect-custom-{}", style::key_part(key)), false))
        .collect::<Vec<_>>();
    if !extra.is_empty() {
        rows.push(widget(Stack::column(12.0).width(LengthSpec::Fill)).children(extra).key("inspect-custom").into_any());
    }
    let panel = style::top_rule(Stack::column(12.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)), 12.0);
    widget(panel).children(rows).key("inspect-metadata").into_any()
}

/// 自动保存撞上服务器的新版本：警告色写明冲突，右边是「采用服务器版本」。没有冲突时不占位。
/// 放在注释前面，紧挨着被拒绝写入的字段，不沉到只读事实下面（DESIGN.md：冲突必须靠近字段出现）。
fn conflict_notice(conflict: &str) -> Option<AnyView> {
    if conflict.is_empty() {
        return None;
    }
    let notice = Stack::bar(8.0).align(AlignSpec::Center).justify(JustifySpec::SpaceBetween).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0));
    let view = widget(notice)
        .children((
            widget(ValidationMessage::new(conflict.to_string(), ValidationIntent::Warning)).key("inspect-conflict"),
            widget(style::styled_button("采用服务器版本", None, ButtonLook::TOOLBAR))
                .key("inspect-adopt")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(inspect_message(InspectMessage::AdoptConflict))),
        ))
        .key("inspect-conflict-row");
    Some(view.into_any())
}

/// 当前检视目标在文件列表里的那一行。预览页从播放集打开时可能不在列表里。
fn target_row(model: &ShellViewModel) -> Option<FileRow> {
    let path = model.inspect.target_path.as_deref()?;
    let ctx = FileContext::from_model(model);
    model.files.visible_rows(&ctx).into_iter().find(|row| row.path == path)
}

/// 注释 / 链接：小标签在上；下面是 34 高的描边框，左边 14px 图标，右边不带边的输入。
#[allow(clippy::too_many_arguments)]
fn inline_field(
    label: &'static str,
    icon: Icon,
    key: &'static str,
    value: &str,
    placeholder: &'static str,
    disabled: bool,
    first: bool,
    map: fn(String) -> InspectMessage,
) -> AnyView {
    let mut input = TextInput::new(value.to_string()).placeholder(placeholder).disabled(disabled);
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
                widget(input).key(key).on_cx(move |_, event: &TextChanged, cx| {
                    cx.dispatch_program_all(inspect_message(map(event.value.to_string())));
                }),
            )),
        ))
        .into_any()
}

/// 评分：五颗 34 方的星，间距 4。到达当前分数的星是金色、浅强调底。再点同一颗回到 0。
fn rating_row(rating: i64, locked: bool) -> AnyView {
    let stars = (1_i64..=5)
        .map(|value| {
            let active = rating >= value;
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
            let style = style.radius_px(999.0);
            let glyph = Stack::column(0.0).width(LengthSpec::Fill).height(LengthSpec::Fill).painter(StarPainter { active });
            widget(ListItem::new(format!("{value} 星")).disabled(locked).style(style))
                .content(widget(glyph))
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

/// 标签组：一整条可点的标题行（「标签组」在左，数量和箭头在右），展开后是标签和添加入口。
fn tags_row(model: &ShellViewModel, locked: bool) -> AnyView {
    let inspect = &model.inspect;
    let tags = inspect.draft_tags().to_vec();
    let path = inspect.target_path.clone().unwrap_or_default();
    let expanded = model.files.tags_expanded(&path, !tags.is_empty());
    let meta = if tags.is_empty() { "暂无标签".to_string() } else { format!("{} 个标签", tags.len()) };
    let header = Stack::bar(10.0).align(AlignSpec::Center).justify(JustifySpec::SpaceBetween);
    let toggle_path = path.clone();
    let mut item_style = NodeStyle::default();
    let layout = Arc::make_mut(&mut item_style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.height = Some(LengthSpec::Px(32.0));
    layout.min_width = Some(LengthSpec::Px(0.0));
    let collapse = widget(ListItem::new("标签组").style(item_style))
        .content(widget(header).children((
            widget(style::text("标签组", 14.0, 500, SemanticColorRole::Text, 21.7)).key("inspect-tag-group-label"),
            widget(Stack::row(6.0).align(AlignSpec::Center)).children((
                widget(style::text(meta, 12.0, 500, SemanticColorRole::Muted, 18.6)).key("inspect-tag-group-count"),
                widget(IconGlyph::new(if expanded { CHEVRON_DOWN } else { CHEVRON_RIGHT }).size(14.0).role(SemanticColorRole::Muted))
                    .key("inspect-tag-group-icon"),
            )),
        )))
        .key("inspect-tag-group")
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Files(FilesMessage::ToggleTags { path: toggle_path.clone(), expanded }));
        })
        .into_any();
    let mut parts = vec![collapse];
    if expanded {
        parts.push(tag_panel(model, &tags, locked));
    }
    widget(style::meta_row(false)).key("inspect-tags-row").children(parts).into_any()
}

/// 展开后的标签区：有标签时是标签片和一枚「+」，没有时是「添加标签」；菜单打开时接在后面。
fn tag_panel(model: &ShellViewModel, tags: &[String], locked: bool) -> AnyView {
    let menu_open = model.inspect.tag_menu_open();
    let mut parts = Vec::new();
    if tags.is_empty() {
        let look = ButtonLook { icon_size: 18.0, gap: 8.0, ..ButtonLook::TOOLBAR };
        parts.push(
            widget(style::styled_button("添加标签", Some(PLUS), look).disabled(locked))
                .key("inspect-tag-menu")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(toggle_tag_menu()))
                .into_any(),
        );
    } else {
        let mut chips = tags.iter().enumerate().map(|(index, tag)| tag_chip(index, tag, locked)).collect::<Vec<_>>();
        chips.push(round_button(PLUS, "inspect-tag-menu", locked, Arc::new(toggle_tag_menu)));
        parts.push(
            widget(Stack::row(8.0).wrap(true).align(AlignSpec::Center).with_layout(|layout| layout.row_gap = Some(LengthSpec::Px(8.0))))
                .children(chips)
                .key("inspect-tags")
                .into_any(),
        );
    }
    if menu_open {
        parts.push(tag_menu(model, tags, locked));
    }
    widget(Stack::column(10.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children(parts).key("inspect-tag-panel").into_any()
}

/// 打开或关上标签菜单。
fn toggle_tag_menu() -> ShellMessage {
    ShellMessage::Files(FilesMessage::ToggleTagMenu)
}

/// 标签片：30 高的药丸，浅底，标签文字和一枚「×」。键带序号，重复标签也不撞键。
fn tag_chip(index: usize, tag: &str, locked: bool) -> AnyView {
    let remove = tag.to_string();
    let key = format!("inspect-tag-{index}-{}", style::key_part(tag));
    widget(
        Stack::row(6.0)
            .align(AlignSpec::Center)
            .min_height(LengthSpec::Px(30.0))
            .with_layout(|layout| layout.padding_left = Some(LengthSpec::Px(10.0)))
            .surface(SemanticColorRole::Subtle)
            .radius_px(999.0),
    )
    .children((
        widget(style::body(tag.to_string()).nowrap(true)).key(format!("{key}-label")),
        round_button_text("×", &format!("{key}-remove"), locked, Arc::new(move || inspect_message(InspectMessage::RemoveTag(remove.clone())))),
    ))
    .key(key)
    .into_any()
}

/// 30 方的圆按钮，弱化色，悬停时 `--bg-active` 底。
fn round_button(icon: Icon, key: &str, locked: bool, make: Action) -> AnyView {
    let look = ButtonLook {
        height: 30.0,
        padding_x: 6.0,
        foreground: SemanticColorRole::Muted,
        hover: Some(SemanticColorRole::Active),
        icon_size: 18.0,
        ..ButtonLook::TOOLBAR
    };
    let mut button = style::styled_button("", Some(icon), look).disabled(locked);
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(30.0));
    let rounded = button.style.clone().radius_px(999.0);
    let button = button.style(rounded);
    widget(button).key(key.to_string()).on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(make())).into_any()
}

/// 文字版圆按钮，标签片里的「×」。
fn round_button_text(text: &str, key: &str, locked: bool, make: Action) -> AnyView {
    let look = ButtonLook {
        height: 30.0,
        padding_x: 0.0,
        foreground: SemanticColorRole::Muted,
        hover: Some(SemanticColorRole::Active),
        weight: 400,
        ..ButtonLook::TOOLBAR
    };
    let mut button = style::styled_button(text, None, look).disabled(locked);
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(30.0));
    let rounded = button.style.clone().radius_px(999.0);
    let button = button.style(rounded);
    widget(button).key(key.to_string()).on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(make())).into_any()
}

/// 标签菜单：新建标签输入和「添加“…”」，下面是已有标签。没有标签时铺在「添加标签」下面，
/// 有标签时浮在「+」右下方；这里都按铺开的版式画在标签区里。
fn tag_menu(model: &ShellViewModel, tags: &[String], locked: bool) -> AnyView {
    let draft = model.files.tag_draft.clone();
    let trimmed = draft.trim().to_string();
    let can_create = !trimmed.is_empty() && !tags.iter().any(|tag| tag == &trimmed);
    let mut input = TextInput::new(draft.clone()).placeholder("输入新标签").disabled(locked);
    {
        let layout = Arc::make_mut(&mut input.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(34.0));
        layout.font_size = Some(14.0);
    }
    let add_label = format!("添加“{}”", if trimmed.is_empty() { "新标签" } else { trimmed.as_str() });
    let add_tag = trimmed.clone();
    let mut parts = vec![
        widget(Stack::column(6.0).width(LengthSpec::Fill))
            .key("inspect-tag-draft-group")
            .children((
                widget(style::row_label("新建标签")).key("inspect-tag-draft-label"),
                widget(input)
                    .key("inspect-tag-draft")
                    .on_cx(|_, event: &TextChanged, cx| {
                        cx.dispatch_program_all(ShellMessage::Files(FilesMessage::SetTagDraft(event.value.to_string())));
                    })
                    .on_cx(|_, event: &TextSubmitted, cx| {
                        cx.dispatch_program_all(ShellMessage::Files(FilesMessage::SubmitTagDraft(event.value.clone())));
                    }),
            ))
            .into_any(),
        // `ghost file-metadata-card__tag-create` 没有自己的尺寸，照基础按钮 32 高。
        widget(style::styled_button(add_label, None, ButtonLook { height: 32.0, ..ButtonLook::TOOLBAR }).disabled(!can_create || locked))
            .key("inspect-tag-create")
            .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Files(FilesMessage::SubmitTagDraft(add_tag.clone()))))
            .into_any(),
    ];
    let options = existing_tags(model, tags, &trimmed);
    if !options.is_empty() {
        let chips = options.iter().enumerate().map(|(index, tag)| option_chip(index, tag, locked)).collect::<Vec<_>>();
        parts.push(
            widget(Stack::column(8.0).width(LengthSpec::Fill))
                .children((
                    widget(style::row_label("已有标签")).key("inspect-tag-existing-label"),
                    widget(Stack::row(8.0).wrap(true).with_layout(|layout| layout.row_gap = Some(LengthSpec::Px(8.0))))
                        .children(chips)
                        .key("inspect-tag-existing-chips"),
                ))
                .key("inspect-tag-existing")
                .into_any(),
        );
    }
    widget(Stack::column(10.0).width(LengthSpec::Fill).padding(12.0).surface(SemanticColorRole::Surface).outline(SemanticColorRole::BorderSoft, 1.0).radius(RadiusTier::Xl))
        .children(parts)
        .key("inspect-tag-menu-panel")
        .into_any()
}

/// 当前目录里已经用过、还没加进草稿的标签，按输入过滤，最多 18 个。
fn existing_tags(model: &ShellViewModel, draft: &[String], keyword: &str) -> Vec<String> {
    let keyword = keyword.to_lowercase();
    let mut seen = Vec::new();
    for row in &model.files.rows {
        for tag in &row.tags {
            if !draft.contains(tag) && !seen.contains(tag) && (keyword.is_empty() || tag.to_lowercase().contains(&keyword)) {
                seen.push(tag.clone());
            }
        }
    }
    seen.truncate(18);
    seen
}

/// `workspace-filter-chip`：26 高的描边药丸，12px 弱化色，点一下加进草稿。
/// Vue 的全局 `button { height: 32px }` 会把它顶到 32，这里按组件声明的 26 画。
fn option_chip(index: usize, tag: &str, locked: bool) -> AnyView {
    let look = ButtonLook {
        height: 26.0,
        padding_x: 9.0,
        font_size: 12.0,
        weight: 400,
        foreground: SemanticColorRole::Muted,
        background: Some(SemanticColorRole::Background),
        hover: Some(SemanticColorRole::AccentSoft),
        ..ButtonLook::TOOLBAR
    };
    let button = style::styled_button(tag.to_string(), None, look).disabled(locked);
    let outlined = button.style.clone().outline(SemanticColorRole::Border, 1.0).radius_px(999.0);
    let button = button.style(outlined);
    let add = tag.to_string();
    widget(button)
        .key(format!("inspect-tag-choice-{index}-{}", style::key_part(tag)))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(inspect_message(InspectMessage::AddTag(add.clone()))))
        .into_any()
}

/// 添加到资源库到来源链接的只读行。第一行不画上边线。
fn recorded_grid(model: &ShellViewModel, row: Option<&FileRow>) -> AnyView {
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
    let mut rows = vec![
        fact("添加到资源库", local_time::format_or(&text("addedToLibraryAt", &facts.added_to_library_at), "未记录"), "inspect-recorded-added", true),
        fact("创建时间", local_time::format_or(&text("fileCreatedAt", &facts.file_created_at), "未记录"), "inspect-recorded-created", false),
        fact("文件修改时间", local_time::format_or(&file_modified, "未记录"), "inspect-recorded-modified", false),
        fact("尺寸", dimensions, "inspect-recorded-size", false),
        fact("原始大小", super::inspect_library::format_original_size(original), "inspect-recorded-bytes", false),
    ];
    let palette = row.map(|row| row.palette.clone()).filter(|colors| !colors.is_empty()).unwrap_or_else(|| model.inspect.palette.clone());
    if let Some(chips) = super::palette::metadata_chips(&palette, "inspect-metadata-palette") {
        rows.push(
            widget(style::meta_row(false))
                .children((widget(style::row_label("调色板")).key("inspect-palette-label"), chips))
                .key("inspect-palette")
                .into_any(),
        );
    }
    if let Some(row) = row.filter(|row| !row.tags.is_empty()) {
        rows.push(fact("索引标签", row.tags.join("，"), "inspect-indexed-tags", false));
    }
    if let Some(entry) = row.and_then(|row| model.browser_entries.iter().find(|entry| entry.path == row.path)) {
        let aliases = entry.alias_paths.iter().filter(|path| **path != entry.path).cloned().collect::<Vec<_>>();
        if !aliases.is_empty() {
            rows.push(right_fact("多归属位置", aliases.join("，"), "inspect-alias-paths"));
        }
    }
    let origin_title = text("originTitle", "");
    if !origin_title.is_empty() {
        rows.push(right_fact("来源标题", origin_title, "inspect-origin-title"));
    }
    let source_url = text("sourceUrl", "");
    if !source_url.is_empty() {
        rows.push(source_link_row(&source_url));
    }
    widget(Stack::column(12.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children(rows).key("inspect-recorded").into_any()
}

/// 小标签和值的一行，和详情事实行同一种画法。
fn fact(label: &str, value: String, key: &str, first: bool) -> AnyView {
    widget(style::meta_row(first))
        .children((
            widget(style::row_label(label.to_string())).key(format!("{key}-label")),
            widget(style::value(value)).key(format!("{key}-value")),
        ))
        .key(key.to_string())
        .into_any()
}

/// 值右对齐的一行：多归属位置和来源标题。
fn right_fact(label: &str, value: String, key: &str) -> AnyView {
    let mut node = style::value(value);
    node.style.text_horizontal_alignment = nana_ui::runtime::TextHorizontalAlignment::End;
    widget(style::meta_row(false))
        .children((widget(style::row_label(label.to_string())).key(format!("{key}-label")), widget(node).key(format!("{key}-value"))))
        .key(key.to_string())
        .into_any()
}

/// 来源链接：右对齐的地址，后面是「打开链接」「复制链接」两个 26 方的描边按钮。
/// 只有带协议、且不是 javascript / data / vbscript 的地址才能打开。
fn source_link_row(url: &str) -> AnyView {
    let openable = openable_link(url);
    let open_url = url.to_string();
    let copy_url = url.to_string();
    let mut value = style::value(url.to_string());
    value.style.text_horizontal_alignment = nana_ui::runtime::TextHorizontalAlignment::End;
    let actions = widget(Stack::row(4.0).grow(0.0).shrink(0.0)).key("inspect-source-actions").children((
        source_action(EXTERNAL_LINK, "inspect-source-open", openable, Arc::new(move || ShellMessage::Input(InputMessage::OpenExternalUrl { url: open_url.clone() }))),
        source_action(COPY, "inspect-source-copy", true, Arc::new(move || ShellMessage::Files(FilesMessage::CopyText(copy_url.clone())))),
    ));
    widget(style::meta_row(false))
        .children((
            widget(style::row_label("来源链接")).key("inspect-source-label"),
            widget(Stack::row(8.0).align(AlignSpec::Start).justify(JustifySpec::End).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
                .children((widget(value).key("inspect-source-value"), actions))
                .key("inspect-source-body"),
        ))
        .key("inspect-source")
        .into_any()
}

fn source_action(icon: Icon, key: &str, enabled: bool, make: Action) -> AnyView {
    let look = ButtonLook {
        height: 26.0,
        padding_x: 5.0,
        foreground: SemanticColorRole::Muted,
        background: Some(SemanticColorRole::Background),
        hover: Some(SemanticColorRole::Background),
        icon_size: 14.0,
        ..ButtonLook::TOOLBAR
    };
    let button = style::styled_button("", Some(icon), look).disabled(!enabled);
    let outlined = button.style.clone().outline(SemanticColorRole::BorderSoft, 1.0);
    let mut button = button.style(outlined);
    Arc::make_mut(&mut button.style.layout).width = Some(LengthSpec::Px(26.0));
    widget(button).key(key.to_string()).on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(make())).into_any()
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
    use super::openable_link;

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
