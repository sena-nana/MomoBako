//! 插件管理面板里的一张插件卡片和它展开的设置区。
//!
//! 卡片照 `.extensions-workbench__card`：名称和状态胶囊、说明、插件 id 和版本、禁用或降级原因、
//! 分类到能力的芯片、依赖 / 权限 / Hook / 执行记录四段、底部的设置 / 启用 / 删除，展开设置时下面
//! 多一块设置区（设置标题和说明、「打开目录」、来源账号区、字段表单）。
//!
//! 卡片按插件 id 做键只建一次：文字、按钮文案和禁用状态绑定在卡片投影上；芯片、原因和四段没有
//! 可聚焦的控件，内容变了整块换；设置区只在展开时建。处理器只带插件 id，会变的启用状态点下去时现读。

use nana_ui::runtime::view::{dynamic, fields, when, widget, AnyView, IntoView, Item, Store, StorePath};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, Stack};
use nana_ui_core::{GridTrack, RadiusTier, SemanticColorRole as Role};

use super::super::ShellMessage;
use super::bind::{row_flag, row_text, ActionDisabled, StyleField};
use super::icons;
use super::plugins_state::{ChipTone, ExecutionView, PluginCardView, PluginPanelSignals};
use super::style::{self, action, column, label, pad, row, wrapping, PillTone, Soft, SoftFill, Tone};
use super::AdminMessage;

/// 卡片在 Store 里的句柄。
pub(crate) type CardItem = Item<
    nana_ui::runtime::view::Subfield<
        Item<Store<Vec<super::plugins_state::PluginGroupView>>, String, super::plugins_state::PluginGroupView>,
        Vec<PluginCardView>,
    >,
    String,
    PluginCardView,
>;

/// `.extensions-workbench__card`：主背景、xl 圆角、1px 透明边加 16 内边距，竖排间距 10。
pub(super) fn plugin_card(card: CardItem, signals: PluginPanelSignals) -> AnyView {
    let plugin_id = card.get_untracked().plugin_id;
    let pid = style::key_part(&plugin_id);
    let head = widget(style::spread(10.0, AlignSpec::Center))
        .children((
            style::bound(label(String::new(), 14.0, 700, Role::Text), row_text(card, |card| &card.name)).key(format!("admin-plugin-{pid}")),
            status_pill(card, &pid),
        ))
        .into_any();
    let description = style::bound(wrapping(label(String::new(), 14.0, 400, Role::Muted)), row_text(card, |card| &card.description))
        .key(format!("admin-plugin-desc-{pid}"))
        .into_any();
    let meta = widget(row(10.0).wrap(true))
        .children((
            style::bound(label(String::new(), 12.0, 400, Role::Muted), row_text(card, |card| &card.plugin_id)),
            style::bound(label(String::new(), 12.0, 400, Role::Muted), row_text(card, |card| &card.version)),
        ))
        .key(format!("admin-plugin-meta-{pid}"))
        .into_any();
    let notices = dynamic(row_flag(card, |card| card.notices.clone()), |notices: &Vec<(String, bool)>| {
        widget(column(6.0)).children(notices.iter().map(|(text, danger)| plugin_notice(text, *danger)).collect::<Vec<_>>()).into_any()
    })
    .visible(row_flag(card, |card| !card.notices.is_empty()))
    .key(format!("admin-plugin-notices-{pid}"))
    .into_any();
    let chips_pid = pid.clone();
    let chips = dynamic(row_flag(card, |card| card.chips.clone()), move |chips: &Vec<String>| chips_row(chips, &chips_pid))
        .key(format!("admin-plugin-chips-{pid}"))
        .into_any();
    let settings_id = plugin_id.clone();
    let settings = when(row_flag(card, |card| card.settings_open), move || settings_section(&settings_id, signals))
        .visible(row_flag(card, |card| card.settings_open))
        .into_any();
    let mut body = vec![head, description, meta, notices, chips];
    body.extend(sections(card, &pid));
    body.push(card_actions(card, signals, &plugin_id));
    body.push(settings);
    let card_box = pad(column(10.0), 17.0, 17.0, 17.0, 17.0).surface(Role::Background).radius(RadiusTier::Xl);
    widget(card_box).children(body).key(format!("admin-plugin-card-{pid}")).into_any()
}

/// 状态胶囊：外框和字色跟着语气整套换。
fn status_pill(card: CardItem, pid: &str) -> AnyView {
    let tone = move || card.try_with(|card| card.tone).unwrap_or(PillTone::Ghost);
    let (body, color) = style::pill_look(tone());
    widget(body)
        .prop::<nana_ui::runtime::NodeStyle, StyleField>(move || style::pill_look(tone()).0.node_style())
        .children((style::bound(label(String::new(), 11.0, 600, color), row_text(card, |card| &card.status))
            .foreground(move || Some(style::pill_look(tone()).1))
            .key(format!("admin-plugin-status-{pid}")),))
        .into_any()
}

/// `.plugin-manager__notice`：内边距 8/10、md 圆角、12 号行高 1.5；禁用原因用危险色柔和底。
fn plugin_notice(text: &str, danger: bool) -> AnyView {
    let body = pad(column(0.0), 8.0, 10.0, 8.0, 10.0);
    let (body, color) = if danger {
        (body.painter(SoftFill::new(Soft::Danger, Some(RadiusTier::Md))), Role::Danger)
    } else {
        (body.surface(Role::Subtle).radius(RadiusTier::Md), Role::Muted)
    };
    widget(body).children((widget(wrapping(style::label_lh(text, 12.0, 400, color, 1.5))),)).into_any()
}

/// `.settings-list__chips`：分类、类型、来源、运行时、依赖和能力，上边距 8、间距 8。
fn chips_row(values: &[String], pid: &str) -> AnyView {
    let items = values
        .iter()
        .enumerate()
        .map(|(index, value)| style::hint_chip(value.clone(), format!("admin-plugin-chip-{pid}-{index}")))
        .collect::<Vec<_>>();
    widget(pad(row(8.0).wrap(true), 8.0, 0.0, 0.0, 0.0).width(LengthSpec::Fill)).children(items).into_any()
}

/// 依赖、权限、Hook 和执行记录四段，标签列 56 宽。每段没有内容时不占布局，内容变了整段换。
fn sections(card: CardItem, pid: &str) -> Vec<AnyView> {
    let dependencies = dynamic(row_flag(card, |card| card.dependencies.clone()), |items: &Vec<(String, ChipTone)>| {
        section("依赖", items.iter().map(|(text, tone)| dependency_chip(text.clone(), *tone)).collect())
    })
    .visible(row_flag(card, |card| !card.dependencies.is_empty()))
    .key(format!("admin-plugin-deps-{pid}"))
    .into_any();
    let permissions = dynamic(row_flag(card, |card| card.permissions.clone()), |items: &Vec<String>| {
        section("权限", items.iter().map(|text| dependency_chip(text.clone(), ChipTone::Plain)).collect())
    })
    .visible(row_flag(card, |card| !card.permissions.is_empty()))
    .key(format!("admin-plugin-permissions-{pid}"))
    .into_any();
    let hooks = dynamic(row_flag(card, |card| card.hooks.clone()), |items: &Vec<String>| {
        section("Hook", items.iter().map(|text| dependency_chip(text.clone(), ChipTone::Plain)).collect())
    })
    .visible(row_flag(card, |card| !card.hooks.is_empty()))
    .key(format!("admin-plugin-hooks-{pid}"))
    .into_any();
    let executions = dynamic(row_flag(card, |card| card.executions.clone()), |items: &Vec<ExecutionView>| {
        section_with_body("执行记录", widget(column(8.0)).children(items.iter().map(execution_item).collect::<Vec<_>>()).into_any())
    })
    .visible(row_flag(card, |card| !card.executions.is_empty()))
    .key(format!("admin-plugin-executions-{pid}"))
    .into_any();
    vec![dependencies, permissions, hooks, executions]
}

/// `.plugin-manager__dependency`：最小高 24、内边距 4/8、md 圆角、`bg-subtle`、12 号。
fn dependency_chip(text: String, tone: ChipTone) -> AnyView {
    let body = pad(row(0.0), 4.0, 8.0, 4.0, 8.0).with_layout(|layout| {
        layout.min_height = Some(LengthSpec::Px(24.0));
    });
    let (body, color) = match tone {
        ChipTone::Plain => (body.surface(Role::Subtle).radius(RadiusTier::Md), Role::Text),
        ChipTone::Muted => (body.surface(Role::Subtle).radius(RadiusTier::Md), Role::Muted),
        ChipTone::Danger => (body.painter(SoftFill::new(Soft::Danger, Some(RadiusTier::Md))), Role::Danger),
    };
    widget(body).children((widget(label(text, 12.0, 400, color)),)).into_any()
}

fn section(title: &str, items: Vec<AnyView>) -> AnyView {
    let list = widget(row(6.0).wrap(true).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children(items).into_any();
    section_with_body(title, list)
}

/// `.plugin-manager__section`：`grid-template-columns: 56px minmax(0, 1fr)`，间距 8，顶对齐。
/// 网格先定列宽、再按列宽量行高，折行的芯片按内容列的宽度排。
fn section_with_body(title: &str, body: AnyView) -> AnyView {
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid_columns = Some(vec![GridTrack::Px(56.0), GridTrack::MinMax { min_px: 0.0, fr: 1.0, max_px: None }]);
        layout.gap = Some(LengthSpec::Px(8.0));
        layout.width = Some(LengthSpec::Fill);
        layout.align_items = AlignSpec::Start;
    });
    widget(grid).children((widget(style::label_px(title, 12.0, 400, Role::Muted, 24.0)), body)).into_any()
}

/// 一条执行记录：`grid-template-columns: auto minmax(0, 1fr)`，状态小块加标题、槽位、消息和时间。
fn execution_item(record: &ExecutionView) -> AnyView {
    let mut lines = vec![
        widget(row(6.0).wrap(true))
            .children((widget(label(record.title.clone(), 12.0, 700, Role::Text)), widget(label(record.slot.clone(), 12.0, 400, Role::Muted))))
            .into_any(),
    ];
    if !record.message.is_empty() {
        lines.push(widget(wrapping(style::label_lh(record.message.clone(), 12.0, 400, Role::Muted, 1.45))).into_any());
    }
    lines.push(widget(style::label_lh(record.time.clone(), 12.0, 400, Role::Muted, 1.45)).into_any());
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid_columns = Some(vec![GridTrack::Auto, GridTrack::MinMax { min_px: 0.0, fr: 1.0, max_px: None }]);
        layout.gap = Some(LengthSpec::Px(8.0));
        layout.width = Some(LengthSpec::Fill);
        layout.align_items = AlignSpec::Start;
    });
    widget(grid)
        .children((
            dependency_chip(record.status.clone(), record.tone),
            widget(Stack::column(3.0).min_width(LengthSpec::Px(0.0))).children(lines),
        ))
        .key(format!("admin-plugin-execution-{}", record.key))
        .into_any()
}

/// 卡片底部的操作：设置、启用或禁用，用户插件多一个删除。靠右排。
fn card_actions(card: CardItem, signals: PluginPanelSignals, plugin_id: &str) -> AnyView {
    let pid = style::key_part(plugin_id);
    let head = signals.head;
    let managing = move || head.with(|head| head.managing);
    let (settings_id, toggle_id, delete_id) = (plugin_id.to_string(), plugin_id.to_string(), plugin_id.to_string());
    let buttons = (
        widget(action("设置", Some(icons::SETTINGS), Tone::Plain, false))
            .prop::<bool, ActionDisabled>(managing)
            .key(format!("admin-plugin-settings-{pid}"))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ToggleSettings(settings_id.clone())));
            }),
        widget(action("禁用", Some(icons::POWER), Tone::Plain, false))
            .prop::<String, fields::button::label>(move || if card.try_with(|card| card.enabled).unwrap_or(false) { "禁用" } else { "启用" }.to_string())
            .prop::<bool, ActionDisabled>(managing)
            .key(format!("admin-plugin-toggle-{pid}"))
            .on_cx(move |_, _: &Activate, cx| {
                // 启用状态随插件列表变，点下去时现读。
                let Some(enabled) = card.try_with(|card| card.enabled) else {
                    eprintln!("Nana 插件卡片已经不在列表里：{toggle_id}");
                    return;
                };
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetEnabled { plugin_id: toggle_id.clone(), enabled: !enabled }));
            }),
        widget(action("删除", Some(icons::TRASH2), Tone::Danger, false))
            .prop::<bool, ActionDisabled>(managing)
            .visible(row_flag(card, |card| card.can_delete))
            .key(format!("admin-plugin-delete-{pid}"))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RequestDelete(delete_id.clone())));
            }),
    );
    widget(row(8.0).wrap(true).width(LengthSpec::Fill).justify(nana_ui::runtime::JustifySpec::End))
        .children(buttons)
        .key(format!("admin-plugin-actions-{pid}"))
        .into_any()
}

/// `.plugin-manager__settings`：上边线、上内边距 12，竖排间距 12。头部是设置标题、说明和
/// 「打开目录」；下面是来源账号区和字段表单。只在这张卡片的设置展开时建。
fn settings_section(plugin_id: &str, signals: PluginPanelSignals) -> AnyView {
    let pid = style::key_part(plugin_id);
    let settings = signals.settings;
    let head_signal = signals.head;
    let directory_id = plugin_id.to_string();
    let head_text = widget(Stack::column(0.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
        style::bound(label(String::new(), 13.0, 700, Role::Text), move || settings.with(|head| head.label.clone()))
            .key(format!("admin-plugin-settings-title-{pid}")),
        widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0))
            .visible(move || settings.with(|head| !head.description.is_empty()))
            .children((style::bound(wrapping(style::label_lh(String::new(), 12.0, 400, Role::Muted, 1.5)), move || {
                settings.with(|head| head.description.clone())
            })
            .key(format!("admin-plugin-settings-desc-{pid}")),)),
    ));
    let head = widget(style::spread(12.0, AlignSpec::Start))
        .children((
            head_text,
            widget(action("打开目录", Some(icons::FOLDER_OPEN), Tone::Plain, false))
                .prop::<bool, ActionDisabled>(move || head_signal.with(|head| head.managing))
                .key(format!("admin-plugin-directory-{pid}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::OpenDataDirectory(directory_id.clone())));
                }),
        ))
        .key(format!("admin-plugin-settings-head-{pid}"));
    widget(style::top_rule(pad(column(12.0), 12.0, 0.0, 0.0, 0.0)))
        .children((
            head,
            super::source_page::source_auth_settings(plugin_id, signals.source, signals.source_repos),
            super::super::admin_fields::fields_form(plugin_id, signals.fields),
        ))
        .key(format!("admin-plugin-settings-section-{pid}"))
        .into_any()
}
