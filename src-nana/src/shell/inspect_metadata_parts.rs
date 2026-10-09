//! 元数据编辑区里结构会变的两块：可折叠的标签组，以及 ASMR 条目的「补全候选」。
//!
//! 两块都常驻：展开、菜单、导入框按 `.visible` 显隐；标签片、已有标签和候选是带键的 `each`，
//! 只有变了的那一片重建；输入框受控，处理器只发消息，要用到当前值时现读信号。

use std::sync::Arc;

use nana_ui::icons_tabler::{CHEVRON_DOWN, CHEVRON_RIGHT, PLUS};
use nana_ui::runtime::view::fields::{self, HiddenWhenEmpty};
use nana_ui::runtime::view::{computed, css, each, text, untrack, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, IconGlyph, JustifySpec, LengthSpec, ListItem, NodeStyle, RadiusTier, Select,
    SelectChanged, SemanticColorRole, SettingsCard, Stack, TextChanged, TextInput, TextSubmitted,
};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::Icon;

use super::super::files::FilesMessage;
use super::super::files_view::bind::GlyphIconField;
use super::super::files_view::style::{self, ButtonLook};
use super::super::inspect::InspectMessage;
use super::super::inspect_asmr::{self, AsmrMessage, CandidateCard};
use super::super::ShellMessage;
use super::{inspect_message, MetadataSignals};

/// 标签区一行里的一片：标签，或者末尾的「+」。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum TagChip {
    Tag { index: usize, tag: String },
    Add,
}

/// 标签组：一整条可点的标题行（「标签组」在左，数量和箭头在右），展开后是标签和添加入口。
pub(super) fn tags_row(signals: MetadataSignals) -> AnyView {
    let tags = signals.tags;
    let meta = move || tags.with(|view| if view.tags.is_empty() { "暂无标签".to_string() } else { format!("{} 个标签", view.tags.len()) });
    let chevron = move || tags.with(|view| if view.expanded { CHEVRON_DOWN } else { CHEVRON_RIGHT });
    let header = Stack::bar(10.0).align(AlignSpec::Center).justify(JustifySpec::SpaceBetween);
    let mut item_style = NodeStyle::default();
    let layout = Arc::make_mut(&mut item_style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.height = Some(LengthSpec::Px(32.0));
    layout.min_width = Some(LengthSpec::Px(0.0));
    let collapse = widget(ListItem::new("标签组").style(item_style))
        .content(widget(header).children((
            widget(style::text("标签组", 14.0, 500, SemanticColorRole::Text, 21.7)).key("inspect-tag-group-label"),
            widget(Stack::row(6.0).align(AlignSpec::Center)).children((
                widget(style::text(untrack(meta), 12.0, 500, SemanticColorRole::Muted, 18.6))
                    .prop::<String, fields::text::value>(meta)
                    .key("inspect-tag-group-count"),
                widget(IconGlyph::new(untrack(chevron)).size(14.0).role(SemanticColorRole::Muted))
                    .prop::<Icon, GlyphIconField>(chevron)
                    .key("inspect-tag-group-icon"),
            )),
        )))
        .key("inspect-tag-group")
        .on_cx(move |_, _: &Activate, cx| {
            let (path, expanded) = tags.with_untracked(|view| (view.path.clone(), view.expanded));
            cx.dispatch_program_all(ShellMessage::Files(FilesMessage::ToggleTags { path, expanded }));
        })
        .into_any();
    widget(style::meta_row(false)).key("inspect-tags-row").children((collapse, tag_panel(signals))).into_any()
}

/// 展开后的标签区：有标签时是标签片和一枚「+」，没有时是「添加标签」；菜单打开时接在后面。
fn tag_panel(signals: MetadataSignals) -> AnyView {
    let tags = signals.tags;
    let locked = signals.locked();
    let empty = move || tags.with(|view| view.tags.is_empty());
    let look = ButtonLook { icon_size: 18.0, gap: 8.0, ..ButtonLook::TOOLBAR };
    let add_first = widget(style::styled_button("添加标签", Some(PLUS), look).disabled(untrack(&locked)))
        .visible(empty)
        .prop::<bool, fields::button::disabled>(locked.clone())
        .key("inspect-tag-menu")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(toggle_tag_menu()));
    let chips = computed(move || {
        tags.with(|view| {
            let mut chips = view.tags.iter().enumerate().map(|(index, tag)| TagChip::Tag { index, tag: tag.clone() }).collect::<Vec<_>>();
            chips.push(TagChip::Add);
            chips
        })
    });
    let chip_locked = locked.clone();
    let row = each(chips, TagChip::clone, move |chip| match chip {
        TagChip::Tag { index, tag } => tag_chip(index, &tag, chip_locked.clone()),
        TagChip::Add => round_button(PLUS, "inspect-tag-menu", chip_locked.clone()),
    })
    .horizontal(8.0)
    .css(css! { flex-wrap: wrap; row-gap: 8px; })
    .key("inspect-tags")
    .visible(move || !empty());
    widget(Stack::column(10.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .visible(move || tags.with(|view| view.expanded))
        .children((add_first, row, tag_menu(signals)))
        .key("inspect-tag-panel")
        .into_any()
}

/// 打开或关上标签菜单。
fn toggle_tag_menu() -> ShellMessage {
    ShellMessage::Files(FilesMessage::ToggleTagMenu)
}

/// 标签片：30 高的药丸，浅底，标签文字和一枚「×」。键带序号，重复标签也不撞键。
fn tag_chip(index: usize, tag: &str, locked: impl Fn() -> bool + Send + Clone + 'static) -> AnyView {
    let remove = tag.to_string();
    let key = format!("inspect-tag-{index}-{}", style::key_part(tag));
    let look = ButtonLook {
        height: 30.0,
        padding_x: 0.0,
        foreground: SemanticColorRole::Muted,
        hover: Some(SemanticColorRole::Active),
        weight: 400,
        ..ButtonLook::TOOLBAR
    };
    let mut button = style::styled_button("×", None, look).disabled(untrack(&locked));
    Arc::make_mut(&mut button.style.layout).width = Some(LengthSpec::Px(30.0));
    let rounded = button.style.clone().radius_px(999.0);
    let button = widget(button.style(rounded))
        .prop::<bool, fields::button::disabled>(locked)
        .key(format!("{key}-remove"))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(inspect_message(InspectMessage::RemoveTag(remove.clone()))));
    widget(
        Stack::row(6.0)
            .align(AlignSpec::Center)
            .min_height(LengthSpec::Px(30.0))
            .with_layout(|layout| layout.padding_left = Some(LengthSpec::Px(10.0)))
            .surface(SemanticColorRole::Subtle)
            .radius_px(999.0),
    )
    .children((widget(style::body(tag.to_string()).nowrap(true)).key(format!("{key}-label")), button))
    .key(key)
    .into_any()
}

/// 30 方的圆按钮，弱化色，悬停时 `--bg-active` 底。点了开关标签菜单。
fn round_button(icon: Icon, key: &str, locked: impl Fn() -> bool + Send + Clone + 'static) -> AnyView {
    let look = ButtonLook {
        height: 30.0,
        padding_x: 6.0,
        foreground: SemanticColorRole::Muted,
        hover: Some(SemanticColorRole::Active),
        icon_size: 18.0,
        ..ButtonLook::TOOLBAR
    };
    let mut button = style::styled_button("", Some(icon), look).disabled(untrack(&locked));
    Arc::make_mut(&mut button.style.layout).width = Some(LengthSpec::Px(30.0));
    let rounded = button.style.clone().radius_px(999.0);
    widget(button.style(rounded))
        .prop::<bool, fields::button::disabled>(locked)
        .key(key.to_string())
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(toggle_tag_menu()))
        .into_any()
}

/// 标签菜单：新建标签输入和「添加“…”」，下面是已有标签。没有标签时铺在「添加标签」下面，
/// 有标签时浮在「+」右下方；这里都按铺开的版式画在标签区里。
fn tag_menu(signals: MetadataSignals) -> AnyView {
    let tags = signals.tags;
    let draft = signals.tag_draft;
    let locked = signals.locked();
    let mut input = TextInput::new(draft.current()).placeholder("输入新标签").disabled(untrack(&locked));
    {
        let layout = Arc::make_mut(&mut input.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(34.0));
        layout.font_size = Some(14.0);
    }
    let label = move || tags.with(|view| view.add_label.clone());
    let create_locked = locked.clone();
    let create_disabled = move || !tags.with(|view| view.can_create) || create_locked();
    let create = widget(
        style::styled_button(untrack(&label), None, ButtonLook { height: 32.0, ..ButtonLook::TOOLBAR }).disabled(untrack(&create_disabled)),
    )
    .prop::<String, fields::button::label>(label)
    .prop::<bool, fields::button::disabled>(create_disabled)
    .key("inspect-tag-create")
    .on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program_all(ShellMessage::Files(FilesMessage::SubmitTagDraft(draft.current().trim().to_string())));
    });
    let options = computed(move || tags.with(|view| view.options.iter().cloned().enumerate().collect::<Vec<_>>()));
    let option_locked = locked.clone();
    let existing = widget(Stack::column(8.0).width(LengthSpec::Fill))
        .visible(move || tags.with(|view| !view.options.is_empty()))
        .children((
            widget(style::row_label("已有标签")).key("inspect-tag-existing-label"),
            each(options, |option: &(usize, String)| option.clone(), move |(index, tag)| option_chip(index, &tag, option_locked.clone()))
                .horizontal(8.0)
                .css(css! { flex-wrap: wrap; row-gap: 8px; })
                .key("inspect-tag-existing-chips"),
        ))
        .key("inspect-tag-existing");
    let draft_group = widget(Stack::column(6.0).width(LengthSpec::Fill)).key("inspect-tag-draft-group").children((
        widget(style::row_label("新建标签")).key("inspect-tag-draft-label"),
        widget(input)
            .model(draft.signal())
            .prop::<bool, fields::text_input::disabled>(locked)
            .key("inspect-tag-draft")
            .on_cx(|_, event: &TextChanged, cx| {
                cx.dispatch_program_all(ShellMessage::Files(FilesMessage::SetTagDraft(event.value.to_string())));
            })
            .on_cx(|_, event: &TextSubmitted, cx| {
                cx.dispatch_program_all(ShellMessage::Files(FilesMessage::SubmitTagDraft(event.value.clone())));
            }),
    ));
    widget(
        Stack::column(10.0)
            .width(LengthSpec::Fill)
            .padding(12.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Xl),
    )
    .visible(move || tags.with(|view| view.menu_open))
    .children((draft_group, create, existing))
    .key("inspect-tag-menu-panel")
    .into_any()
}

/// `workspace-filter-chip`：26 高的描边药丸，12px 弱化色，点一下加进草稿。
/// Vue 的全局 `button { height: 32px }` 会把它顶到 32，这里按组件声明的 26 画。
fn option_chip(index: usize, tag: &str, locked: impl Fn() -> bool + Send + Clone + 'static) -> AnyView {
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
    let button = style::styled_button(tag.to_string(), None, look).disabled(untrack(&locked));
    let outlined = button.style.clone().outline(SemanticColorRole::Border, 1.0).radius_px(999.0);
    let add = tag.to_string();
    widget(button.style(outlined))
        .prop::<bool, fields::button::disabled>(locked)
        .key(format!("inspect-tag-choice-{index}-{}", style::key_part(tag)))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(inspect_message(InspectMessage::AddTag(add.clone()))))
        .into_any()
}

/// 元数据列里的补全候选。没有候选就写「暂无候选」，不造一条来源；不是 ASMR 条目时整块不占位。
pub(super) fn candidate_section(signals: MetadataSignals) -> AnyView {
    let view = signals.candidates;
    let locked = move || view.with(|view| view.locked);
    let current = view.get_untracked();
    let import = widget(Button::new("导入").kind(ButtonKind::Ghost).disabled(current.locked))
        .prop::<bool, fields::button::disabled>(locked)
        .key("inspect-asmr-import")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::ToggleImport)));
    let empty = text("暂无候选".to_string())
        .visible(move || view.with(|view| view.candidates.is_empty() && !view.import_open))
        .key("inspect-asmr-candidate-empty");
    let cards = each(computed(move || view.with(|view| view.candidates.clone())), CandidateCard::clone, move |card| {
        inspect_asmr::candidate_card(card, locked)
    })
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
fn import_form(signals: MetadataSignals) -> AnyView {
    let view = signals.candidates;
    let locked = move || view.with(|view| view.locked);
    let current = view.get_untracked();
    let select = Select::new(Some(current.provider.clone()))
        .options(inspect_asmr::provider_options())
        .placeholder("ASMR Provider")
        .size(ControlSize::Small)
        .disabled(current.locked);
    let lookup = signals.lookup_id;
    let json = signals.import_json;
    let body = (
        widget(select)
            .prop::<Option<Arc<str>>, fields::select::value>(move || Some(Arc::from(view.with(|view| view.provider.clone()))))
            .prop::<bool, fields::select::disabled>(locked)
            .key("inspect-asmr-provider")
            .on_cx(|_, event: &SelectChanged, cx| {
                cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::SetProvider(event.value.to_string())));
            }),
        widget(TextInput::new(lookup.current()).placeholder("RJ123456").size(ControlSize::Small).disabled(current.locked))
            .model(lookup.signal())
            .prop::<bool, fields::text_input::disabled>(locked)
            .key("inspect-asmr-work-id")
            .on_cx(|_, event: &TextChanged, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::SetLookupId(event.value.to_string())))),
        widget(Button::new("抓取候选").kind(ButtonKind::Ghost).disabled(current.locked))
            .prop::<bool, fields::button::disabled>(locked)
            .key("inspect-asmr-lookup")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::Lookup))),
        widget(TextInput::new(json.current()).placeholder("ASMR 候选 JSON").disabled(current.locked))
            .model(json.signal())
            .prop::<bool, fields::text_input::disabled>(locked)
            .key("inspect-asmr-import-json")
            .on_cx(|_, event: &TextChanged, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::SetImportDraft(event.value.to_string())))),
        text(current.import_error.clone())
            .prop::<String, HiddenWhenEmpty<fields::text::value>>(move || view.with(|view| view.import_error.clone()))
            .key("inspect-asmr-import-error"),
        widget(Button::new("导入候选").kind(ButtonKind::Ghost).disabled(current.locked))
            .prop::<bool, fields::button::disabled>(locked)
            .key("inspect-asmr-import-submit")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Asmr(AsmrMessage::ImportCandidate))),
    );
    widget(Stack::column(8.0))
        .visible(move || view.with(|view| view.import_open))
        .key("inspect-asmr-import-form")
        .children(body)
        .into_any()
}
