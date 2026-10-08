//! 已选文件的元数据编辑：评分、注释、链接、标签和自定义字段。
//!
//! 改动先进草稿，260ms 后自动保存；标签菜单和标签组展开走检视状态。

use std::sync::Arc;

use nana_ui::icons_tabler::{LINK, MESSAGE, PLUS, STAR};
use nana_ui::runtime::view::{button, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, Chip, ChipDismissed, Icon, IconButton, IconGlyph, LabeledValue,
    LengthSpec, ListItem, RadiusTier, SemanticColorRole, Stack, TextChanged, TextInput,
    ValidationIntent, ValidationMessage,
};
use nana_ui::{ButtonKind, ControlSize};

use super::inspect::InspectMessage;
use super::{ShellMessage, ShellViewModel};

/// 注释、链接、评分，然后是标签组。和 `FileMetadataEditor` 的顺序一致。
/// 注释和链接是标签在上、输入在下。保存交给 260 毫秒自动保存，画面上不放保存、撤销和重做。
pub(super) fn metadata_panel(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let locked = !inspect.can_edit();
    let expanded = inspect.tags_expanded;
    let mut rows = vec![
        meta_row(
            "注释",
            MESSAGE,
            "inspect-comment",
            inspect.draft_comment(),
            "记录这个文件的用途、状态或上下文。",
            locked,
            |value| InspectMessage::SetComment(value),
        ),
        meta_row("链接", LINK, "inspect-link", inspect.draft_link(), "https://example.com", locked, |value| {
            InspectMessage::SetLink(value)
        }),
        rating_buttons(inspect.draft_rating(), locked),
        tag_group_row(inspect.draft_tags().len(), expanded),
    ];
    if expanded {
        let mut tags = Vec::new();
        for tag in inspect.draft_tags() {
            let remove = tag.clone();
            tags.push(
                widget(Chip::new(tag.clone()).selected(true).dismissible(!locked).close_label(format!("移除 {tag}")).disabled(locked))
                    .key(format!("inspect-tag-{tag}"))
                    .on_cx(move |_, _: &ChipDismissed, cx| cx.dispatch_program(inspect_message(InspectMessage::RemoveTag(remove.clone()))))
                    .into_any(),
            );
        }
        if !tags.is_empty() {
            rows.push(widget(Stack::row(4.0).wrap(true)).key("inspect-tags").children(tags).into_any());
        }
        rows.push(tag_add_button(locked));
        if inspect.tag_menu_open() {
            rows.push(tag_draft_field(locked));
            for tag in tag_choices(model) {
                let add = tag.clone();
                rows.push(
                    button(tag.clone())
                        .key(format!("inspect-tag-choice-{tag}"))
                        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::AddTag(add.clone()))))
                        .into_any(),
                );
            }
            rows.push(button("关闭标签").key("inspect-tag-menu-close").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::CloseTagMenu));
            }).into_any());
        }
    }
    rows.extend(super::inspect_library::recorded_rows(model));
    if let Some(palette) = super::palette::swatches(&inspect.palette, "inspect-metadata-palette") {
        rows.push(palette);
    }
    let custom = super::inspect_asmr::display_custom(model);
    let library = super::inspect_library::library_sections(&custom);
    let claimed = !library.is_empty();
    rows.extend(library);
    rows.extend(super::inspect_asmr::candidate_section(model, &custom));
    for (key, value) in inspect.draft_custom() {
        if claimed && super::inspect_library::claimed_metadata_keys().contains(&key.as_str()) {
            continue;
        }
        rows.push(widget(LabeledValue::new(key.clone(), value.clone())).key(format!("inspect-custom-{key}")).into_any());
        let remove = key.clone();
        rows.push(
            widget(Button::new(format!("删除字段 {key}")).kind(ButtonKind::Danger).disabled(locked))
                .key(format!("inspect-custom-remove-{key}"))
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::RemoveCustom(remove.clone()))))
                .into_any(),
        );
    }
    if !inspect.conflict.is_empty() {
        rows.push(widget(ValidationMessage::new(inspect.conflict.clone(), ValidationIntent::Danger)).key("inspect-conflict").into_any());
        rows.push(
            button("采用服务器版本").key("inspect-adopt").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::AdoptConflict));
            }).into_any(),
        );
    }
    // 上边一条发丝线，把统计和编辑分开。不给这一列再套卡片。
    let panel = super::workbench::with_top_divider(
        Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)),
    );
    widget(panel).children(rows).into_any()
}

/// 当前文件上、还没写进草稿的标签，最多 18 个。
fn tag_choices(model: &ShellViewModel) -> Vec<String> {
    let Some(path) = model.inspect.target_path.clone() else {
        return Vec::new();
    };
    let ctx = super::files::FileContext::from_model(model);
    let Some(row) = model.files.visible_rows(&ctx).into_iter().find(|row| row.path == path) else {
        eprintln!("Nana 标签菜单找不到当前文件：{path}");
        return Vec::new();
    };
    let draft = model.inspect.draft_tags().to_vec();
    row.tags.into_iter().filter(|tag| !draft.iter().any(|item| item == tag)).take(18).collect()
}

/// 标签在上、输入在下。
/// 注释用 Tabler `message`（方框气泡加两行字，最接近 Lucide MessageSquareText）。
/// 链接用 Tabler `link`。已编译目录里没有 Lucide Link2 那种左右半环加横线。
fn meta_row(
    label: &'static str,
    icon: Icon,
    key_name: &'static str,
    value: &str,
    placeholder: &'static str,
    disabled: bool,
    map: impl Fn(String) -> InspectMessage + Send + 'static,
) -> AnyView {
    let mut input = TextInput::new(value.to_string()).placeholder(placeholder).size(ControlSize::Small).disabled(disabled);
    input.style.border = None;
    input.style.background = None;
    input.style.radius = None;
    input.style.interaction.hovered.border = None;
    input.style.interaction.focused.border = None;
    let layout = Arc::make_mut(&mut input.style.layout);
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Fill);
    widget(Stack::column(2.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .key(format!("{key_name}-row"))
        .children((
            widget(super::workbench::eyebrow(label)).key(format!("{key_name}-label")),
            widget(
                Stack::row(8.0)
                    .align(AlignSpec::Center)
                    .width(LengthSpec::Fill)
                    .min_width(LengthSpec::Px(0.0))
                    .padding_xy(10.0, 0.0)
                    .min_height(LengthSpec::Px(32.0))
                    .surface(SemanticColorRole::Background)
                    .outline(SemanticColorRole::Border, 1.0)
                    .radius(RadiusTier::Lg),
            )
            .key(format!("{key_name}-field"))
            .children((
                widget(IconGlyph::new(icon).size(14.0)).key(format!("{key_name}-icon")),
                widget(input).key(key_name).on_cx(move |_, event: &TextChanged, cx| {
                    cx.dispatch_program(inspect_message(map(event.value.to_string())));
                }),
            )),
        ))
        .into_any()
}

/// 标签组是一整条：标题在左，数量和箭头在右。展开时箭头向下。
fn tag_group_row(count: usize, expanded: bool) -> AnyView {
    let meta = if count == 0 { "暂无标签".to_string() } else { format!("{count} 个标签") };
    let icon = if expanded { Icon::ChevronDown } else { Icon::ChevronRight };
    let mut item = ListItem::new("标签组").detail(meta).size(ControlSize::Small);
    let layout = Arc::make_mut(&mut item.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.flex_grow = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    widget(item)
        .key("inspect-tag-group")
        .trailing(widget(IconGlyph::new(icon).size(14.0)).key("inspect-tag-group-icon"))
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::ToggleTagGroup)))
        .into_any()
}

/// 空标签时只放「添加标签」。输入留在点开的菜单里。
fn tag_add_button(locked: bool) -> AnyView {
    let mut add = super::workbench::ghost_button("添加标签").icon(PLUS).icon_size(16.0).size(ControlSize::Small).disabled(locked);
    let layout = Arc::make_mut(&mut add.style.layout);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.width = Some(LengthSpec::Shrink);
    widget(add).key("inspect-tag-menu").on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program(inspect_message(InspectMessage::OpenTagMenu { x: 8.0, y: 8.0 }));
    }).into_any()
}

/// 菜单里的新标签输入。没点开「添加标签」时不画。
fn tag_draft_field(locked: bool) -> AnyView {
    let mut input = TextInput::new(String::new()).placeholder("输入新标签").size(ControlSize::Small).disabled(locked);
    let layout = Arc::make_mut(&mut input.style.layout);
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Fill);
    widget(input).key("inspect-tag-draft").on_cx(|_, event: &TextChanged, cx| {
        cx.dispatch_program(inspect_message(InspectMessage::AddTag(event.value.to_string())));
    }).into_any()
}

/// 1–5 星标。再点同一颗回到 0，和 `SetRating` 的语义一致。
fn rating_buttons(rating: i64, locked: bool) -> AnyView {
    let mut stars = Vec::new();
    for value in 1_i64..=5 {
        let active = rating >= value;
        stars.push(
            widget(IconButton::new(STAR, format!("{value} 星")).selected(active).disabled(locked))
                .key(format!("inspect-rate-{value}"))
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::SetRating(value))))
                .into_any(),
        );
    }
    widget(Stack::column(2.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children((
        widget(super::workbench::eyebrow("评分")).key("inspect-rating"),
        widget(Stack::row(2.0).align(AlignSpec::Center)).key("inspect-rating-stars").children(stars),
    )).into_any()
}

fn inspect_message(message: InspectMessage) -> ShellMessage {
    ShellMessage::Inspect(message)
}
