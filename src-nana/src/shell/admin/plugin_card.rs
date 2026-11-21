//! 插件管理面板里的一张插件卡片和它展开的设置区。
//!
//! 卡片是 NanaUI 的描边 `Card`，内容：名称和状态徽章（`StatusBadge`）、说明、插件 id 和版本、
//! 禁用或降级原因（`callout` 提示条）、分类到能力的标签（`tag`）、依赖 / 权限 / Hook / 执行记录
//! 四段（依赖和执行结果是状态徽章，权限和 Hook 是标签）、底部的启用开关（`Switch`）和设置 / 删除，
//! 展开设置时下面多一块设置区（设置标题和说明、「打开目录」、下载服务或 Office 转换状态、来源账号区、
//! 字段表单）。
//!
//! 卡片按插件 id 做键只建一次：文字、徽章色调、开关和禁用状态绑定在卡片投影上；标签、原因和四段没有
//! 可聚焦的控件，内容变了整块换；设置区只在展开时建。处理器只带插件 id。

use std::sync::Arc;

use nana_ui::icons_tabler::{ALERT_CIRCLE, ALERT_TRIANGLE};
use nana_ui::runtime::view::{
    AnyView, IntoView, Item, Store, StorePath, callout, dynamic, status_badge, tag, when, widget,
};
use nana_ui::runtime::{
    Activate, AlignSpec, Card, LengthSpec, Stack, StatusTone, Switch, ToggleChanged,
};
use nana_ui_core::{
    ButtonKind, CardKind, GridTrack, SemanticColorRole as Role, SwitchControlPosition, TextWeight,
    TypeRole,
};

use super::super::ShellMessage;
use super::AdminMessage;
use super::bind::{ActionDisabled, row_flag, row_text};
use super::icons;
use super::plugins_state::{ExecutionView, PluginCardView, PluginPanelSignals};
use super::style::{self, action, column, label, pad, row, wrapping};

/// 卡片在 Store 里的句柄。
pub(crate) type CardItem = Item<
    nana_ui::runtime::view::Subfield<
        Item<
            Store<Vec<super::plugins_state::PluginGroupView>>,
            String,
            super::plugins_state::PluginGroupView,
        >,
        Vec<PluginCardView>,
    >,
    String,
    PluginCardView,
>;

/// 插件卡片：NanaUI 的描边卡片，里面竖排间距 10。
pub(super) fn plugin_card(card: CardItem, signals: PluginPanelSignals) -> AnyView {
    let plugin_id = card.get_untracked().plugin_id;
    let pid = style::key_part(&plugin_id);
    let head = widget(style::spread(10.0, AlignSpec::Center))
        .children((
            style::bound(
                label(
                    String::new(),
                    TypeRole::Section,
                    TextWeight::Bold,
                    Role::Text,
                ),
                row_text(card, |card| &card.name),
            )
            .key(format!("admin-plugin-{pid}")),
            status_pill(card, &pid),
        ))
        .into_any();
    let description = style::bound(
        wrapping(label(
            String::new(),
            TypeRole::Section,
            TextWeight::Regular,
            Role::Muted,
        )),
        row_text(card, |card| &card.description),
    )
    .key(format!("admin-plugin-desc-{pid}"))
    .into_any();
    let meta = widget(row(10.0).wrap(true))
        .children((
            style::bound(
                label(
                    String::new(),
                    TypeRole::Meta,
                    TextWeight::Regular,
                    Role::Muted,
                ),
                row_text(card, |card| &card.plugin_id),
            ),
            style::bound(
                label(
                    String::new(),
                    TypeRole::Meta,
                    TextWeight::Regular,
                    Role::Muted,
                ),
                row_text(card, |card| &card.version),
            ),
        ))
        .key(format!("admin-plugin-meta-{pid}"))
        .into_any();
    let notices = dynamic(
        row_flag(card, |card| card.notices.clone()),
        |notices: &Vec<(String, bool)>| {
            widget(column(6.0))
                .children(
                    notices
                        .iter()
                        .map(|(text, danger)| plugin_notice(text, *danger))
                        .collect::<Vec<_>>(),
                )
                .into_any()
        },
    )
    .visible(row_flag(card, |card| !card.notices.is_empty()))
    .key(format!("admin-plugin-notices-{pid}"))
    .into_any();
    let chips_pid = pid.clone();
    let chips = dynamic(
        row_flag(card, |card| card.chips.clone()),
        move |chips: &Vec<String>| chips_row(chips, &chips_pid),
    )
    .key(format!("admin-plugin-chips-{pid}"))
    .into_any();
    let settings_id = plugin_id.clone();
    let settings = when(row_flag(card, |card| card.settings_open), move || {
        settings_section(&settings_id, signals)
    })
    .visible(row_flag(card, |card| card.settings_open))
    .into_any();
    let mut body = vec![head, description, meta, notices, chips];
    body.extend(sections(card, &pid));
    body.push(card_actions(card, signals, &plugin_id));
    body.push(settings);
    widget(Card::new().kind(CardKind::Outlined))
        .children((widget(column(10.0)).children(body),))
        .key(format!("admin-plugin-card-{pid}"))
        .into_any()
}

/// 状态徽章：NanaUI 的 `StatusBadge`，文字和色调跟着卡片投影。
fn status_pill(card: CardItem, pid: &str) -> AnyView {
    let status = row_text(card, |card| &card.status);
    status_badge(move || Arc::<str>::from(status()))
        .tone(move || {
            card.try_with(|card| card.tone)
                .unwrap_or(StatusTone::Neutral)
        })
        .key(format!("admin-plugin-status-{pid}"))
        .into_any()
}

/// 卡片里的提示条：禁用原因用危险色调，降级原因用警示色调。
fn plugin_notice(text: &str, danger: bool) -> AnyView {
    let (icon, tone) = if danger {
        (ALERT_CIRCLE, StatusTone::Danger)
    } else {
        (ALERT_TRIANGLE, StatusTone::Warning)
    };
    callout(icon, tone, text.to_string()).into_any()
}

/// 一枚标签：NanaUI 的 `tag`，不能点。
fn keyed_tag(text: &str, key: String) -> AnyView {
    widget(column(0.0).width(LengthSpec::Shrink))
        .children((tag(text.to_string()),))
        .key(key)
        .into_any()
}

/// 标签行：分类、类型、来源、运行时、依赖和能力，上边距 8、间距 8。
fn chips_row(values: &[String], pid: &str) -> AnyView {
    let items = values
        .iter()
        .enumerate()
        .map(|(index, value)| keyed_tag(value, format!("admin-plugin-chip-{pid}-{index}")))
        .collect::<Vec<_>>();
    widget(pad(row(8.0).wrap(true), 8.0, 0.0, 0.0, 0.0).width(LengthSpec::Fill))
        .children(items)
        .into_any()
}

/// 依赖、权限、Hook 和执行记录四段，标签列 56 宽。每段没有内容时不占布局，内容变了整段换。
fn sections(card: CardItem, pid: &str) -> Vec<AnyView> {
    let dependencies = dynamic(
        row_flag(card, |card| card.dependencies.clone()),
        |items: &Vec<(String, StatusTone)>| {
            section(
                "依赖",
                items
                    .iter()
                    .map(|(text, tone)| badge(text, *tone))
                    .collect(),
            )
        },
    )
    .visible(row_flag(card, |card| !card.dependencies.is_empty()))
    .key(format!("admin-plugin-deps-{pid}"))
    .into_any();
    let permissions = dynamic(
        row_flag(card, |card| card.permissions.clone()),
        |items: &Vec<String>| section("权限", items.iter().map(|text| tag(text.clone())).collect()),
    )
    .visible(row_flag(card, |card| !card.permissions.is_empty()))
    .key(format!("admin-plugin-permissions-{pid}"))
    .into_any();
    let hooks = dynamic(
        row_flag(card, |card| card.hooks.clone()),
        |items: &Vec<String>| section("Hook", items.iter().map(|text| tag(text.clone())).collect()),
    )
    .visible(row_flag(card, |card| !card.hooks.is_empty()))
    .key(format!("admin-plugin-hooks-{pid}"))
    .into_any();
    let executions = dynamic(
        row_flag(card, |card| card.executions.clone()),
        |items: &Vec<ExecutionView>| {
            section_with_body(
                "执行记录",
                widget(column(8.0))
                    .children(items.iter().map(execution_item).collect::<Vec<_>>())
                    .into_any(),
            )
        },
    )
    .visible(row_flag(card, |card| !card.executions.is_empty()))
    .key(format!("admin-plugin-executions-{pid}"))
    .into_any();
    vec![dependencies, permissions, hooks, executions]
}

/// 依赖状态和执行结果：NanaUI 的 `StatusBadge`。
fn badge(text: &str, tone: StatusTone) -> AnyView {
    status_badge(text.to_string()).tone(tone).into_any()
}

fn section(title: &str, items: Vec<AnyView>) -> AnyView {
    let list = widget(
        row(6.0)
            .wrap(true)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children(items)
    .into_any();
    section_with_body(title, list)
}

/// 卡片里的一段：`grid-template-columns: 56px minmax(0, 1fr)`，间距 8，顶对齐。
/// 网格先定列宽、再按列宽量行高，折行的芯片按内容列的宽度排。
fn section_with_body(title: &str, body: AnyView) -> AnyView {
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid.columns = Some(vec![
            GridTrack::Px(56.0),
            GridTrack::MinMax {
                min_px: 0.0,
                fr: 1.0,
                max_px: None,
            },
        ]);
        layout.gap = Some(LengthSpec::Px(8.0));
        layout.width = Some(LengthSpec::Fill);
        layout.align_items = AlignSpec::Start;
    });
    widget(grid)
        .children((
            widget(style::label_px(
                title,
                TypeRole::Meta,
                TextWeight::Regular,
                Role::Muted,
                24.0,
            )),
            body,
        ))
        .into_any()
}

/// 一条执行记录：`grid-template-columns: auto minmax(0, 1fr)`，状态小块加标题、槽位、消息和时间。
fn execution_item(record: &ExecutionView) -> AnyView {
    let mut lines = vec![
        widget(row(6.0).wrap(true))
            .children((
                widget(label(
                    record.title.clone(),
                    TypeRole::Meta,
                    TextWeight::Bold,
                    Role::Text,
                )),
                widget(label(
                    record.slot.clone(),
                    TypeRole::Meta,
                    TextWeight::Regular,
                    Role::Muted,
                )),
            ))
            .into_any(),
    ];
    if !record.message.is_empty() {
        lines.push(
            widget(wrapping(style::label_lh(
                record.message.clone(),
                TypeRole::Meta,
                TextWeight::Regular,
                Role::Muted,
                1.45,
            )))
            .into_any(),
        );
    }
    lines.push(
        widget(style::label_lh(
            record.time.clone(),
            TypeRole::Meta,
            TextWeight::Regular,
            Role::Muted,
            1.45,
        ))
        .into_any(),
    );
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid.columns = Some(vec![
            GridTrack::Auto,
            GridTrack::MinMax {
                min_px: 0.0,
                fr: 1.0,
                max_px: None,
            },
        ]);
        layout.gap = Some(LengthSpec::Px(8.0));
        layout.width = Some(LengthSpec::Fill);
        layout.align_items = AlignSpec::Start;
    });
    widget(grid)
        .children((
            badge(&record.status, record.tone),
            widget(Stack::column(3.0).min_width(LengthSpec::Px(0.0))).children(lines),
        ))
        .key(format!("admin-plugin-execution-{}", record.key))
        .into_any()
}

/// 卡片底部：左边启用开关（NanaUI 的 `Switch`，开关在文字前，占去余下的宽度），右边设置，
/// 用户插件多一个删除。开关拨过去以后发启用或禁用；是否启用等插件列表回来再跟着写。
fn card_actions(card: CardItem, signals: PluginPanelSignals, plugin_id: &str) -> AnyView {
    let pid = style::key_part(plugin_id);
    let head = signals.head;
    let managing = move || head.with(|head| head.managing);
    let (toggle_id, settings_id, delete_id) = (
        plugin_id.to_string(),
        plugin_id.to_string(),
        plugin_id.to_string(),
    );
    let buttons = (
        widget(enable_switch(card))
            .checked(move || card.try_with(|card| card.enabled).unwrap_or(false))
            .disabled(managing)
            .key(format!("admin-plugin-toggle-{pid}"))
            .on_cx(move |_, event: &ToggleChanged, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetEnabled {
                    plugin_id: toggle_id.clone(),
                    enabled: event.checked,
                }));
            }),
        widget(action(
            "设置",
            Some(icons::SETTINGS),
            ButtonKind::Ghost,
            false,
        ))
        .prop::<bool, ActionDisabled>(managing)
        .key(format!("admin-plugin-settings-{pid}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ToggleSettings(
                settings_id.clone(),
            )));
        }),
        widget(action(
            "删除",
            Some(icons::TRASH2),
            ButtonKind::Danger,
            false,
        ))
        .prop::<bool, ActionDisabled>(managing)
        .visible(row_flag(card, |card| card.can_delete))
        .key(format!("admin-plugin-delete-{pid}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RequestDelete(
                delete_id.clone(),
            )));
        }),
    );
    widget(row(8.0).wrap(true).width(LengthSpec::Fill))
        .children(buttons)
        .key(format!("admin-plugin-actions-{pid}"))
        .into_any()
}

/// 启用开关：标签「启用」，开关画在文字前；初值取这张卡片现在的启用状态。
fn enable_switch(card: CardItem) -> Switch {
    let mut switch = Switch::new("启用", card.try_with(|card| card.enabled).unwrap_or(false));
    switch.control_position = SwitchControlPosition::Start;
    switch
}

/// 设置区：上边线、上内边距 12，竖排间距 12。头部是设置标题、说明和
/// 「打开目录」；下面是下载服务或 Office 转换状态、来源账号区和字段表单。只在这张卡片的设置展开时建。
fn settings_section(plugin_id: &str, signals: PluginPanelSignals) -> AnyView {
    let pid = style::key_part(plugin_id);
    let settings = signals.settings;
    let head_signal = signals.head;
    let directory_id = plugin_id.to_string();
    let head_text = widget(
        Stack::column(0.0)
            .width(LengthSpec::Shrink)
            .min_width(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0),
    )
    .children((
        style::bound(
            label(String::new(), TypeRole::Body, TextWeight::Bold, Role::Text),
            move || settings.with(|head| head.label.clone()),
        )
        .key(format!("admin-plugin-settings-title-{pid}")),
        widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0))
            .visible(move || settings.with(|head| !head.description.is_empty()))
            .children((style::bound(
                wrapping(style::label_lh(
                    String::new(),
                    TypeRole::Meta,
                    TextWeight::Regular,
                    Role::Muted,
                    1.5,
                )),
                move || settings.with(|head| head.description.clone()),
            )
            .key(format!("admin-plugin-settings-desc-{pid}")),)),
    ));
    let head = widget(style::spread(12.0, AlignSpec::Start))
        .children((
            head_text,
            widget(action(
                "打开目录",
                Some(icons::FOLDER_OPEN),
                ButtonKind::Ghost,
                false,
            ))
            .prop::<bool, ActionDisabled>(move || head_signal.with(|head| head.managing))
            .key(format!("admin-plugin-directory-{pid}"))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::OpenDataDirectory(
                    directory_id.clone(),
                )));
            }),
        ))
        .key(format!("admin-plugin-settings-head-{pid}"));
    widget(style::top_rule(pad(column(12.0), 12.0, 0.0, 0.0, 0.0)))
        .children((
            head,
            super::downloader::settings_block(signals.downloader),
            super::office::settings_block(signals.office),
            super::source_page::source_auth_settings(
                plugin_id,
                signals.source,
                signals.source_repos,
            ),
            super::super::admin_fields::fields_form(plugin_id, signals.fields),
        ))
        .key(format!("admin-plugin-settings-section-{pid}"))
        .into_any()
}
