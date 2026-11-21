//! 元数据编辑区里结构会变的两块：可折叠的标签组，以及 ASMR 条目的「补全候选」。
//!
//! 标签片和可加的标签都是 NanaUI 的 `Chip`。
//!
//! 两块都常驻：展开、菜单、导入框按 `.visible` 显隐；标签片、标签分组、已有标签和候选是带键的 `each`，
//! 只有变了的那一片重建；输入框受控，处理器只发消息，要用到当前值时现读信号。

use std::sync::Arc;

use nana_ui::icons_tabler::{CHEVRON_DOWN, CHEVRON_RIGHT, PLUS};
use nana_ui::runtime::view::fields::{self, HiddenWhenEmpty};
use nana_ui::runtime::view::{
    AnyView, FieldWrite, IntoView, computed, css, each, text, untrack, widget,
};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, Chip, ChipDismissed, EmptyState, IconGlyph, JustifySpec,
    LengthSpec, ListItem, NodeStyle, Popover, PopoverAlignment, PopoverToggled, Select,
    SelectChanged, SemanticColorRole, SettingsCard, Stack, TextChanged, TextInput, TextSubmitted,
};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{Icon, RadiusTier, SpacingRole, SpacingStep, TextWeight};

use super::super::ShellMessage;
use super::super::field_bind::GlyphIcon;
use super::super::files::FilesMessage;
use super::super::files_view::style::{self, ButtonLook, Height, Inset};
use super::super::inspect::InspectMessage;
use super::super::inspect_asmr::{self, AsmrMessage, CandidateCard};
use super::{MetadataSignals, TagGroupOption, inspect_message};
use crate::theme_map::tiers;

/// 标签区一行里的一片：标签，或者末尾的「+」。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum TagChip {
    Tag { index: usize, tag: String },
    Add,
}

/// 标签组：一整条可点的标题行（「标签组」在左，数量和箭头在右），展开后是标签和添加入口。
pub(super) fn tags_row(signals: MetadataSignals) -> AnyView {
    let tags = signals.tags;
    let meta = move || {
        tags.with(|view| {
            if view.tags.is_empty() {
                "暂无标签".to_string()
            } else {
                format!("{} 个标签", view.tags.len())
            }
        })
    };
    let chevron = move || {
        tags.with(|view| {
            if view.expanded {
                CHEVRON_DOWN
            } else {
                CHEVRON_RIGHT
            }
        })
    };
    let header = Stack::bar(0.0)
        .gap_theme(SpacingStep::Lg)
        .align(AlignSpec::Center)
        .justify(JustifySpec::SpaceBetween);
    let mut item_style = NodeStyle::default()
        .control_height(nana_ui_core::ControlHeight::Exact(ControlSize::Medium));
    let layout = Arc::make_mut(&mut item_style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.height = Some(LengthSpec::Px(
        ControlSize::Medium.height_in(nana_ui::theme::UI_METRICS),
    ));
    layout.min_width = Some(LengthSpec::Px(0.0));
    let collapse = widget(ListItem::new("标签组").style(item_style))
        .content(
            widget(header).children((
                widget(style::text(
                    "标签组",
                    14.0,
                    TextWeight::Medium,
                    SemanticColorRole::Text,
                    21.7,
                ))
                .key("inspect-tag-group-label"),
                widget(style::row(SpacingStep::Sm).align(AlignSpec::Center)).children((
                    widget(style::text(
                        untrack(meta),
                        12.0,
                        TextWeight::Medium,
                        SemanticColorRole::Muted,
                        18.6,
                    ))
                    .prop::<String, fields::text::value>(meta)
                    .key("inspect-tag-group-count"),
                    widget(
                        IconGlyph::new(untrack(chevron))
                            .size(style::TOOLBAR_ICON)
                            .role(SemanticColorRole::Muted),
                    )
                    .prop::<Icon, GlyphIcon>(chevron)
                    .key("inspect-tag-group-icon"),
                )),
            )),
        )
        .key("inspect-tag-group")
        .on_cx(move |_, _: &Activate, cx| {
            let (path, expanded) = tags.with_untracked(|view| (view.path.clone(), view.expanded));
            cx.dispatch_program_all(ShellMessage::Files(FilesMessage::ToggleTags {
                path,
                expanded,
            }));
        })
        .into_any();
    widget(style::meta_row(false))
        .key("inspect-tags-row")
        .children((collapse, tag_panel(signals)))
        .into_any()
}

/// 展开后的标签区：标签片，末尾是打开标签菜单的触发器（没有标签时写「添加标签」，有标签时是一枚「+」）。
fn tag_panel(signals: MetadataSignals) -> AnyView {
    let tags = signals.tags;
    let locked = signals.locked();
    let chips = computed(move || {
        tags.with(|view| {
            let mut chips = view
                .tags
                .iter()
                .enumerate()
                .map(|(index, tag)| TagChip::Tag {
                    index,
                    tag: tag.clone(),
                })
                .collect::<Vec<_>>();
            chips.push(TagChip::Add);
            chips
        })
    });
    each(chips, TagChip::clone, move |chip| match chip {
        TagChip::Tag { index, tag } => tag_chip(index, &tag, locked.clone()),
        TagChip::Add => tag_menu(signals),
    })
    .horizontal(tiers::space_px(SpacingStep::Md))
    .css(css! { flex-wrap: wrap; gap: theme(space-md); })
    .key("inspect-tags")
    .visible(move || tags.with(|view| view.expanded))
    .into_any()
}
/// 标签片：NanaUI 的可关闭 `Chip`，关闭钮由框架装在片尾，点它发 [`ChipDismissed`]，换成 `RemoveTag`。
/// 不能编辑时整片禁用，关闭钮一并不发事件。键带序号，重复标签也不撞键。
fn tag_chip(
    index: usize,
    tag: &str,
    locked: impl Fn() -> bool + Send + Clone + 'static,
) -> AnyView {
    let remove = tag.to_string();
    let chip = Chip::new(tag)
        .dismissible(true)
        .close_label(format!("移除标签“{tag}”"))
        .disabled(untrack(&locked));
    widget(chip)
        .prop::<bool, fields::chip::disabled>(locked)
        .key(format!("inspect-tag-{index}-{}", style::key_part(tag)))
        .on_cx(move |_, _: &ChipDismissed, cx| {
            cx.dispatch_program_all(inspect_message(InspectMessage::RemoveTag(remove.clone())))
        })
        .into_any()
}

/// 标签菜单：NanaUI 的 `Popover`，触发器是标签片末尾的「+」（没有标签时写「添加标签」）。面板里是新建标签
/// 输入和「添加“…”」，下面是标签分组和已有标签；点外面、Escape 和再点触发器收起，焦点回到触发器，都由
/// 框架负责。框架开合时发 [`PopoverToggled`]，换成 `TagMenuToggled` 写回 ViewModel；ViewModel 不让开
/// （没有素材、虚拟素材、保存中）时 `open` 写回假，面板随即收起。
fn tag_menu(signals: MetadataSignals) -> AnyView {
    let tags = signals.tags;
    let draft = signals.tag_draft;
    let locked = signals.locked();
    let mut input = TextInput::new(draft.current())
        .placeholder("输入新标签")
        .disabled(untrack(&locked));
    {
        let layout = Arc::make_mut(&mut input.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(style::TOOLBAR_HEIGHT));
    }
    let label = move || tags.with(|view| view.add_label.clone());
    let create_locked = locked.clone();
    let create_disabled = move || !tags.with(|view| view.can_create) || create_locked();
    let create = widget(
        style::styled_button(
            untrack(&label),
            None,
            ButtonLook {
                height: Height::Control(ControlSize::Medium),
                ..ButtonLook::TOOLBAR
            },
        )
        .disabled(untrack(&create_disabled)),
    )
    .prop::<String, fields::button::label>(label)
    .prop::<bool, fields::button::disabled>(create_disabled)
    .key("inspect-tag-create")
    .on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program_all(ShellMessage::Files(FilesMessage::SubmitTagDraft(
            draft.current().trim().to_string(),
        )));
    });
    let groups = computed(move || tags.with(|view| view.groups.clone()));
    let group_locked = locked.clone();
    let grouped = widget(style::column(SpacingStep::Md).width(LengthSpec::Fill))
        .visible(move || tags.with(|view| !view.groups.is_empty()))
        .children((
            widget(style::row_label("标签分组")).key("inspect-tag-groups-label"),
            each(groups, TagGroupOption::clone, move |group| {
                tag_group(group, group_locked.clone())
            })
            .gap(tiers::space_px(SpacingStep::Md))
            .key("inspect-tag-groups-list"),
        ))
        .key("inspect-tag-groups");
    let options = computed(move || {
        tags.with(|view| view.options.iter().cloned().enumerate().collect::<Vec<_>>())
    });
    let option_locked = locked.clone();
    let existing = widget(style::column(SpacingStep::Md).width(LengthSpec::Fill))
        .visible(move || tags.with(|view| !view.options.is_empty()))
        .children((
            widget(style::row_label("已有标签")).key("inspect-tag-existing-label"),
            each(
                options,
                |option: &(usize, String)| option.clone(),
                move |(index, tag)| {
                    option_chip(
                        &format!("inspect-tag-choice-{index}"),
                        &tag,
                        option_locked.clone(),
                    )
                },
            )
            .horizontal(tiers::space_px(SpacingStep::Md))
            .css(css! { flex-wrap: wrap; gap: theme(space-md); })
            .key("inspect-tag-existing-chips"),
        ))
        .key("inspect-tag-existing");
    let draft_group = widget(style::column(SpacingStep::Sm).width(LengthSpec::Fill))
        .key("inspect-tag-draft-group")
        .children((
            widget(style::row_label("新建标签")).key("inspect-tag-draft-label"),
            widget(input)
                .model(draft.signal())
                .prop::<bool, fields::text_input::disabled>(locked)
                .key("inspect-tag-draft")
                .on_cx(|_, event: &TextChanged, cx| {
                    cx.dispatch_program_all(ShellMessage::Files(FilesMessage::SetTagDraft(
                        event.value.to_string(),
                    )));
                })
                .on_cx(|_, event: &TextSubmitted, cx| {
                    cx.dispatch_program_all(ShellMessage::Files(FilesMessage::SubmitTagDraft(
                        event.value.clone(),
                    )));
                }),
        ));
    let body = widget(style::column(SpacingStep::Lg).width(LengthSpec::Fill))
        .children((draft_group, create, grouped, existing))
        .key("inspect-tag-menu-panel");
    let empty = move || tags.with(|view| view.tags.is_empty());
    let trigger = widget(style::row(SpacingStep::Sm).align(AlignSpec::Center))
        .children((
            widget(
                IconGlyph::new(PLUS)
                    .size(TAG_ICON)
                    .role(SemanticColorRole::Muted),
            )
            .key("inspect-tag-menu-icon"),
            widget(
                style::text(
                    "添加标签",
                    14.0,
                    TextWeight::Medium,
                    SemanticColorRole::Text,
                    21.7,
                )
                .nowrap(true),
            )
            .visible(empty)
            .key("inspect-tag-menu-text"),
        ))
        .key("inspect-tag-menu-trigger");
    let popover = Popover::new()
        .trigger("添加标签")
        .bare_trigger(true)
        .alignment(PopoverAlignment::Start)
        .width(320.0)
        .trigger_style(tag_trigger_style());
    widget(popover)
        .prop::<bool, TagMenuOpen>(move || tags.with(|view| view.menu_open))
        .trigger(trigger)
        .children((body,))
        .key("inspect-tag-menu")
        .on_cx(|_, toggled: &PopoverToggled, cx| {
            cx.dispatch_program_all(ShellMessage::Files(FilesMessage::TagMenuToggled(
                toggled.open,
            )))
        })
        .into_any()
}

/// 标签菜单触发器上的加号 18px。主题没有图标尺寸档，照写像素。
const TAG_ICON: f32 = 18.0;
/// 触发器的高度和最小宽度 30：控件档里没有这个高度，照写像素。
const TAG_TRIGGER: f32 = 30.0;

/// 触发器：30 高的药丸，左右 `Sm`，弱化色，悬停 `--bg-active` 底。
fn tag_trigger_style() -> NodeStyle {
    let look = ButtonLook {
        height: Height::Px(TAG_TRIGGER),
        padding_x: Inset::Step(SpacingStep::Sm),
        foreground: SemanticColorRole::Muted,
        hover: Some(SemanticColorRole::Active),
        icon_size: TAG_ICON,
        radius: RadiusTier::Full,
        ..ButtonLook::TOOLBAR
    };
    let mut style = style::styled_button("", Some(PLUS), look).style;
    Arc::make_mut(&mut style.layout).min_width = Some(LengthSpec::Px(TAG_TRIGGER));
    style
}

/// 标签菜单开没开着。
struct TagMenuOpen;

impl FieldWrite<Popover, bool> for TagMenuOpen {
    const FIELD: &'static str = "Popover.open";

    fn write(target: &mut Popover, open: bool) {
        target.open = open;
    }

    fn differs(target: &Popover, open: &bool) -> bool {
        target.open != *open
    }
}

/// 一个仓库标签分组：11px 半粗的分组名，下面是可加的标签片；名字和标签片按「归属」关系隔 `Sm`。
fn tag_group(group: TagGroupOption, locked: impl Fn() -> bool + Send + Clone + 'static) -> AnyView {
    let key = format!("inspect-tag-group-{}", style::key_part(&group.id));
    let chips = group
        .tags
        .iter()
        .enumerate()
        .map(|(index, tag)| option_chip(&format!("{key}-{index}"), tag, locked.clone()))
        .collect::<Vec<_>>();
    widget(
        Stack::column(0.0)
            .gap_theme(SpacingRole::Attach.step())
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children((
        widget(style::text(
            group.name.clone(),
            11.0,
            TextWeight::Semibold,
            SemanticColorRole::Muted,
            11.0 * style::LINE_BODY,
        ))
        .key(format!("{key}-name")),
        widget(
            style::row(SpacingStep::Md)
                .wrap(true)
                .width(LengthSpec::Fill)
                .min_width(LengthSpec::Px(0.0))
                .with_layout(|layout| tiers::row_gap(layout, SpacingStep::Md)),
        )
        .children(chips)
        .key(format!("{key}-chips")),
    ))
    .key(key)
    .into_any()
}

/// 可加的标签片：NanaUI 的 `Chip`，点一下加进草稿。键是 `{prefix}-{标签}`。
fn option_chip(
    prefix: &str,
    tag: &str,
    locked: impl Fn() -> bool + Send + Clone + 'static,
) -> AnyView {
    let add = tag.to_string();
    widget(Chip::new(tag).disabled(untrack(&locked)))
        .prop::<bool, fields::chip::disabled>(locked)
        .key(format!("{prefix}-{}", style::key_part(tag)))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(inspect_message(InspectMessage::AddTag(add.clone())))
        })
        .into_any()
}

/// 元数据列里的补全候选。没有候选就写「暂无候选」，不造一条来源；不是 ASMR 条目时整块不占位。
pub(super) fn candidate_section(signals: MetadataSignals) -> AnyView {
    let view = signals.candidates;
    let locked = move || view.with(|view| view.locked);
    let cover_locked = move || view.with(|view| view.cover_locked);
    let current = view.get_untracked();
    let import = widget(
        Button::new("导入")
            .kind(ButtonKind::Ghost)
            .disabled(current.locked),
    )
    .prop::<bool, fields::button::disabled>(locked)
    .key("inspect-asmr-import")
    .on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::ToggleImport))
    });
    let empty = widget(EmptyState::new("暂无候选").compact(true))
        .visible(move || view.with(|view| view.candidates.is_empty() && !view.import_open))
        .key("inspect-asmr-candidate-empty");
    let cards = each(
        computed(move || view.with(|view| view.candidates.clone())),
        CandidateCard::clone,
        move |card| inspect_asmr::candidate_card(card, locked, cover_locked),
    )
    .visible(move || view.with(|view| !view.candidates.is_empty()));
    widget(SettingsCard::new("补全候选"))
        .visible(move || view.with(|view| view.shown))
        .children((
            text("ASMR Provider".to_string()).key("inspect-asmr-provider-eyebrow"),
            text("补全候选".to_string()).key("inspect-asmr-provider-title"),
            import,
            import_form(signals),
            empty,
            cards,
        ))
        .key("inspect-asmr-candidates")
        .into_any()
}

/// 导入框：来源、作品 ID 和「抓取候选」，候选 JSON 和「导入候选」，出错时多一行原因。
/// 抓取进行中，来源、作品 ID 和抓取按钮不能动，按钮写「抓取中」。
fn import_form(signals: MetadataSignals) -> AnyView {
    let view = signals.candidates;
    let locked = move || view.with(|view| view.locked);
    let fetching = move || view.with(|view| view.locked || view.looking_up);
    let current = view.get_untracked();
    let select = Select::new(Some(current.provider.clone()))
        .options(inspect_asmr::provider_options())
        .placeholder("ASMR Provider")
        .size(ControlSize::Small)
        .disabled(current.locked || current.looking_up);
    let lookup = signals.lookup_id;
    let json = signals.import_json;
    let lookup_label = move || {
        view.with(|view| {
            if view.looking_up {
                "抓取中"
            } else {
                "抓取候选"
            }
            .to_string()
        })
    };
    let body = (
        widget(select)
            .prop::<Option<Arc<str>>, fields::select::value>(move || {
                Some(Arc::from(view.with(|view| view.provider.clone())))
            })
            .prop::<bool, fields::select::disabled>(fetching)
            .key("inspect-asmr-provider")
            .on_cx(|_, event: &SelectChanged, cx| {
                cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::SetProvider(
                    event.value.to_string(),
                )));
            }),
        widget(
            TextInput::new(lookup.current())
                .placeholder("RJ123456")
                .size(ControlSize::Small)
                .disabled(current.locked || current.looking_up),
        )
        .model(lookup.signal())
        .prop::<bool, fields::text_input::disabled>(fetching)
        .key("inspect-asmr-work-id")
        .on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::SetLookupId(
                event.value.to_string(),
            )))
        }),
        widget(
            Button::new(untrack(&lookup_label))
                .kind(ButtonKind::Ghost)
                .disabled(current.locked || current.looking_up),
        )
        .prop::<String, fields::button::label>(lookup_label)
        .prop::<bool, fields::button::disabled>(fetching)
        .key("inspect-asmr-lookup")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::Lookup))
        }),
        widget(
            TextInput::new(json.current())
                .placeholder("ASMR 候选 JSON")
                .disabled(current.locked),
        )
        .model(json.signal())
        .prop::<bool, fields::text_input::disabled>(locked)
        .key("inspect-asmr-import-json")
        .on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::SetImportDraft(
                event.value.to_string(),
            )))
        }),
        text(current.import_error.clone())
            .prop::<String, HiddenWhenEmpty<fields::text::value>>(move || {
                view.with(|view| view.import_error.clone())
            })
            .key("inspect-asmr-import-error"),
        widget(
            Button::new("导入候选")
                .kind(ButtonKind::Ghost)
                .disabled(current.locked),
        )
        .prop::<bool, fields::button::disabled>(locked)
        .key("inspect-asmr-import-submit")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::ImportCandidate))
        }),
    );
    widget(style::column(SpacingStep::Md))
        .visible(move || view.with(|view| view.import_open))
        .key("inspect-asmr-import-form")
        .children(body)
        .into_any()
}
