//! 插件管理面板。
//!
//! 结构照 `src/components/PluginManagerPanel.vue`：页头（眉题、标题、说明、插件数、刷新、
//! 安装）、提示、筛选框、按分类分组的插件卡片，卡片里可展开插件设置。设置页和拓展页
//! 共用，只换标题文案。删除确认是壳层浮层，见 [`delete_dialog`]。

use nana_ui::runtime::view::{signal, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, Stack, TextChanged, TextInput};
use nana_ui::ButtonKind;
use nana_ui_core::{RadiusTier, SemanticColorRole as Role};

use super::super::view_part_overlay::dialog::{intent_button, DialogFrame};
use super::super::view_part_overlay::session::Projected;
use super::super::{ShellMessage, ShellViewModel};
use super::icons;
use super::style::{self, action, column, label, pad, row, wrapping, PillTone, Soft, SoftFill, Tone};
use super::support;
use super::AdminMessage;
use crate::backend::services::repository::{PluginHookExecutionRecord, PluginManifest};

/// 面板上随入口变化的文案。
pub(crate) struct PanelCopy {
    pub title: &'static str,
    pub eyebrow: &'static str,
    pub subline: &'static str,
    pub search_placeholder: &'static str,
    pub empty_title: &'static str,
    pub empty_description: &'static str,
}

/// 设置页里的「插件管理」。
pub(crate) const SETTINGS_COPY: PanelCopy = PanelCopy {
    title: "插件管理",
    eyebrow: "系统扩展",
    subline: "在这里启用、禁用、删除用户插件，或从压缩包导入新的插件。",
    search_placeholder: "筛选插件、能力或运行时",
    empty_title: "没有匹配的插件",
    empty_description: "试试其他关键词，或从压缩包导入新的插件。",
};

/// 拓展页里的「文件系统与插件」。
pub(crate) const EXTENSIONS_COPY: PanelCopy = PanelCopy {
    title: "文件系统与插件",
    eyebrow: "拓展能力",
    subline: "这里集中展示当前插件和后端能力。",
    search_placeholder: "筛选导入器、脚本或元数据拓展",
    empty_title: "没有匹配的插件",
    empty_description: "试试其他关键词，或从 .momoplug 安装新的插件。",
};

/// 整个插件管理面板。
pub(crate) fn manager_panel(model: &ShellViewModel, copy: &PanelCopy) -> AnyView {
    let admin = &model.admin;
    let filtered = support::filtered_plugins(&admin.plugins, &admin.keyword, &admin.hook_executions);
    let managing = admin.managing;
    let mut body = vec![style::workbench_header(
        copy.eyebrow,
        copy.title,
        copy.subline,
        vec![
            style::stat(format!("{} 个插件", filtered.len()), "admin-plugin-count"),
            widget(action("刷新", Some(icons::REFRESH_CW), Tone::Plain, managing))
                .key("admin-plugin-refresh")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RefreshPlugins)))
                .into_any(),
            widget(action("从 .momoplug 安装", Some(icons::UPLOAD), Tone::Primary, managing))
                .key("admin-plugin-install")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ChooseArchive)))
                .into_any(),
        ],
        "admin-plugin",
    )];
    if !admin.action_error.is_empty() {
        body.push(style::state_notice(admin.action_error.clone(), true, "admin-plugin-error"));
    } else if !admin.action_message.is_empty() {
        body.push(style::state_notice(admin.action_message.clone(), false, "admin-plugin-message"));
    } else if !admin.load_error.is_empty() && !admin.loading_settings {
        body.push(style::state_notice(admin.load_error.clone(), true, "admin-plugin-load-error"));
    }
    body.push(search_field(&admin.keyword, copy.search_placeholder));
    if admin.loading_settings {
        body.push(widget(label("正在加载插件信息", 14.0, 400, Role::Text)).key("admin-plugin-loading").into_any());
    } else if filtered.is_empty() {
        body.push(style::dashed_empty(copy.empty_title, copy.empty_description, "admin-plugin-empty"));
    } else {
        body.push(groups(model));
    }
    style::workbench_panel(body, "admin-plugin-panel")
}

/// `.search-workbench__field`：主背景、1px 边线、lg 圆角，最小高 38，左右 12。
fn search_field(keyword: &str, placeholder: &str) -> AnyView {
    let input = style::bare_input(TextInput::new(keyword.to_string()).label("筛选插件").placeholder(placeholder.to_string()));
    widget(style::field_frame())
        .children((widget(input).key("admin-plugin-keyword").on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetKeyword(event.value.to_string())));
        }),))
        .key("admin-plugin-search")
        .into_any()
}

/// 分组：组间 16，组内标题行最小高 28，卡片间 10。
fn groups(model: &ShellViewModel) -> AnyView {
    let mut sections = Vec::new();
    for (category, ids) in model.admin.grouped_plugins() {
        let keys = style::unique_keys(ids.iter().map(String::as_str));
        let mut cards = Vec::new();
        for (plugin_id, key) in ids.iter().zip(&keys) {
            if let Some(plugin) = model.admin.plugins.iter().find(|plugin| &plugin.plugin_id == plugin_id) {
                cards.push(plugin_card(model, plugin, key));
            }
        }
        let head = widget(style::fixed(style::spread(12.0, AlignSpec::Center), None, None).with_layout(|layout| {
            layout.min_height = Some(LengthSpec::Px(28.0));
        }))
        .children((
            widget(style::label_lh(support::category_label(&category), 15.0, 700, Role::Text, 1.25)).key(format!("admin-plugin-group-{}", style::key_part(&category))),
            widget(label(format!("{} 个插件", ids.len()), 12.0, 400, Role::Muted)).key(format!("admin-plugin-group-count-{}", style::key_part(&category))),
        ));
        sections.push(
            widget(column(8.0))
                .children((head, widget(column(10.0)).children(cards)))
                .key(format!("admin-plugin-section-{}", style::key_part(&category)))
                .into_any(),
        );
    }
    widget(column(16.0)).children(sections).key("admin-plugin-groups").into_any()
}

/// `.extensions-workbench__card`：主背景、xl 圆角、1px 透明边加 16 内边距，竖排间距 10。
fn plugin_card(model: &ShellViewModel, plugin: &PluginManifest, pid: &str) -> AnyView {
    let plugin_id = plugin.plugin_id.as_str();
    let managing = model.admin.managing;
    let status = support::plugin_status_label(plugin);
    let head = widget(style::spread(10.0, AlignSpec::Center))
        .children((
            widget(label(plugin.name.clone(), 14.0, 700, Role::Text)).key(format!("admin-plugin-{pid}")),
            style::pill(status, status_tone(plugin), format!("admin-plugin-status-{pid}")),
        ))
        .into_any();
    let mut body = vec![
        head,
        widget(wrapping(label(plugin.description.clone(), 14.0, 400, Role::Muted))).key(format!("admin-plugin-desc-{pid}")).into_any(),
        widget(row(10.0).wrap(true))
            .children((
                widget(label(plugin.plugin_id.clone(), 12.0, 400, Role::Muted)),
                widget(label(format!("v{}", plugin.version), 12.0, 400, Role::Muted)),
            ))
            .key(format!("admin-plugin-meta-{pid}"))
            .into_any(),
    ];
    if plugin.disable_reason.is_some() || plugin.degradation_reason.is_some() {
        let mut notices = Vec::new();
        if let Some(reason) = plugin.disable_reason.as_deref() {
            notices.push(plugin_notice(reason, true));
        }
        if let Some(reason) = plugin.degradation_reason.as_deref() {
            notices.push(plugin_notice(reason, false));
        }
        body.push(widget(column(6.0)).children(notices).key(format!("admin-plugin-notices-{pid}")).into_any());
    }
    body.push(chips(plugin));
    body.extend(sections(model, plugin));
    body.push(card_actions(plugin, managing));
    if model.admin.active_settings_plugin_id.as_deref() == Some(plugin_id) {
        body.push(settings_section(model, plugin));
    }
    let card = pad(column(10.0), 17.0, 17.0, 17.0, 17.0).surface(Role::Background).radius(RadiusTier::Xl);
    widget(card).children(body).key(format!("admin-plugin-card-{pid}")).into_any()
}

fn status_tone(plugin: &PluginManifest) -> PillTone {
    if plugin.status == "unavailable" || plugin.status == "error" {
        PillTone::Danger
    } else if plugin.degraded && plugin.enabled {
        PillTone::Warning
    } else if !plugin.enabled || plugin.status == "disabled" {
        PillTone::Ghost
    } else {
        PillTone::Accent
    }
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
fn chips(plugin: &PluginManifest) -> AnyView {
    let pid = style::key_part(&plugin.plugin_id);
    let mut values = vec![
        support::category_label(&support::plugin_category(plugin)).to_string(),
        plugin.kind.clone(),
        support::plugin_source_label(&plugin.source).to_string(),
        support::plugin_runtime_label(&plugin.runtime).to_string(),
        format!("依赖 {}", support::dependency_label(plugin)),
    ];
    values.extend(plugin.capabilities.iter().cloned());
    let items = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| style::hint_chip(value, format!("admin-plugin-chip-{pid}-{index}")))
        .collect::<Vec<_>>();
    widget(pad(row(8.0).wrap(true), 8.0, 0.0, 0.0, 0.0).width(LengthSpec::Fill)).children(items).key(format!("admin-plugin-chips-{pid}")).into_any()
}

/// 依赖、权限、Hook 和执行记录四段，标签列 56 宽。
fn sections(model: &ShellViewModel, plugin: &PluginManifest) -> Vec<AnyView> {
    let plugin_id = plugin.plugin_id.as_str();
    let pid = style::key_part(plugin_id);
    let mut out = Vec::new();
    let deps = &plugin.dependency_status;
    if !deps.required.is_empty() || !deps.optional.is_empty() {
        let mut items = Vec::new();
        for state in &deps.required {
            let name = state.name.clone().unwrap_or_else(|| state.plugin_id.clone());
            items.push(dependency_chip(format!("必需 {name} · {}", support::dependency_status_label(&state.status)), dependency_tone(&state.status)));
        }
        for state in &deps.optional {
            let name = state.name.clone().unwrap_or_else(|| state.plugin_id.clone());
            items.push(dependency_chip(format!("可选 {name} · {}", support::dependency_status_label(&state.status)), dependency_tone(&state.status)));
        }
        out.push(section("依赖", items, format!("admin-plugin-deps-{pid}")));
    }
    if !plugin.permissions.is_empty() {
        let items = plugin.permissions.iter().map(|permission| dependency_chip(permission.clone(), ChipTone::Plain)).collect();
        out.push(section("权限", items, format!("admin-plugin-permissions-{pid}")));
    }
    if !plugin.hooks.is_empty() {
        let items = plugin
            .hooks
            .iter()
            .map(|hook| dependency_chip(format!("{} · {}", hook.label.clone().unwrap_or_else(|| hook.action.clone()), hook.slot), ChipTone::Plain))
            .collect();
        out.push(section("Hook", items, format!("admin-plugin-hooks-{pid}")));
    }
    let records = model.admin.hook_executions.iter().filter(|record| record.plugin_id == plugin_id).take(3).collect::<Vec<_>>();
    if !records.is_empty() {
        let keys = style::unique_keys(records.iter().map(|record| record.execution_id.as_str()));
        let items = records.into_iter().zip(&keys).map(|(record, key)| execution_item(record, key)).collect::<Vec<_>>();
        let list = widget(column(8.0)).children(items).into_any();
        out.push(section_with_body("执行记录", list, format!("admin-plugin-executions-{pid}")));
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChipTone {
    Plain,
    Muted,
    Danger,
}

fn dependency_tone(status: &str) -> ChipTone {
    match status {
        "missing" | "unavailable" | "error" => ChipTone::Danger,
        "disabled" => ChipTone::Muted,
        _ => ChipTone::Plain,
    }
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

fn section(title: &str, items: Vec<AnyView>, key: String) -> AnyView {
    let list = widget(row(6.0).wrap(true).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children(items).into_any();
    section_with_body(title, list, key)
}

/// `.plugin-manager__section`：56 宽标签列加占满余下宽度的内容列，间距 8，顶对齐。
/// 用横排而不是网格：Nana 网格的自动行高按内容最窄时量，折行芯片会把行撑高。
fn section_with_body(title: &str, body: AnyView, key: String) -> AnyView {
    let content = widget(Stack::column(0.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((body,));
    widget(Stack::row(8.0).align(AlignSpec::Start).width(LengthSpec::Fill))
        .children((widget(style::fixed(column(0.0), Some(56.0), None)).children((widget(style::label_px(title, 12.0, 400, Role::Muted, 24.0)),)), content))
        .key(key)
        .into_any()
}

/// 一条执行记录：状态小块，加标题、槽位、消息和时间。
fn execution_item(record: &PluginHookExecutionRecord, key: &str) -> AnyView {
    let tone = match record.status.as_str() {
        "failed" => ChipTone::Danger,
        "blocked" => ChipTone::Muted,
        _ => ChipTone::Plain,
    };
    let mut lines = vec![
        widget(row(6.0).wrap(true))
            .children((
                widget(label(record.hook_label.clone().unwrap_or_else(|| record.hook_action.clone()), 12.0, 700, Role::Text)),
                widget(label(record.hook_slot.clone(), 12.0, 400, Role::Muted)),
            ))
            .into_any(),
    ];
    if !record.message.is_empty() {
        lines.push(widget(wrapping(style::label_lh(record.message.clone(), 12.0, 400, Role::Muted, 1.45))).into_any());
    }
    lines.push(widget(style::label_lh(support::hook_time_label(&record.started_at), 12.0, 400, Role::Muted, 1.45)).into_any());
    widget(row(8.0).align(AlignSpec::Start))
        .children((
            dependency_chip(support::hook_status_label(&record.status), tone),
            widget(Stack::column(3.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children(lines),
        ))
        .key(format!("admin-plugin-execution-{key}"))
        .into_any()
}

/// 卡片底部的操作：设置、启用或禁用，用户插件多一个删除。靠右排。
fn card_actions(plugin: &PluginManifest, managing: bool) -> AnyView {
    let plugin_id = plugin.plugin_id.clone();
    let pid = style::key_part(&plugin_id);
    let enabled = plugin.enabled;
    let settings_id = plugin_id.clone();
    let toggle_id = plugin_id.clone();
    let mut buttons = vec![
        widget(action("设置", Some(icons::SETTINGS), Tone::Plain, managing))
            .key(format!("admin-plugin-settings-{pid}"))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ToggleSettings(settings_id.clone())));
            })
            .into_any(),
        widget(action(if enabled { "禁用" } else { "启用" }, Some(icons::POWER), Tone::Plain, managing))
            .key(format!("admin-plugin-toggle-{pid}"))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetEnabled { plugin_id: toggle_id.clone(), enabled: !enabled }));
            })
            .into_any(),
    ];
    if support::can_delete_plugin(plugin) {
        let delete_id = plugin_id.clone();
        buttons.push(
            widget(action("删除", Some(icons::TRASH2), Tone::Danger, managing))
                .key(format!("admin-plugin-delete-{pid}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RequestDelete(delete_id.clone())));
                })
                .into_any(),
        );
    }
    widget(row(8.0).wrap(true).width(LengthSpec::Fill).justify(nana_ui::runtime::JustifySpec::End))
        .children(buttons)
        .key(format!("admin-plugin-actions-{pid}"))
        .into_any()
}

/// `.plugin-manager__settings`：上边线、上内边距 12，竖排间距 12。
/// 头部是设置标题、说明和「打开目录」；下面是来源认证页和字段表单。
fn settings_section(model: &ShellViewModel, plugin: &PluginManifest) -> AnyView {
    let plugin_id = plugin.plugin_id.clone();
    let pid = style::key_part(&plugin_id);
    let mut head_text = vec![widget(label(support::plugin_settings_label(plugin), 13.0, 700, Role::Text)).key(format!("admin-plugin-settings-title-{pid}")).into_any()];
    let description = support::plugin_settings_description(plugin);
    if !description.is_empty() {
        head_text.push(
            widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0))
                .children((widget(wrapping(style::label_lh(description, 12.0, 400, Role::Muted, 1.5))).key(format!("admin-plugin-settings-desc-{pid}")),))
                .into_any(),
        );
    }
    let directory_id = plugin_id.clone();
    let head = widget(style::spread(12.0, AlignSpec::Start))
        .children((
            widget(Stack::column(0.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children(head_text),
            widget(action("打开目录", Some(icons::FOLDER_OPEN), Tone::Plain, model.admin.managing))
                .key(format!("admin-plugin-directory-{pid}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::OpenDataDirectory(directory_id.clone())));
                })
                .into_any(),
        ))
        .key(format!("admin-plugin-settings-head-{pid}"))
        .into_any();
    let mut body = vec![head];
    if support::has_source_authentication(plugin) {
        body.push(super::source_page::source_auth_settings(model, plugin));
    }
    if let Some(fields) = super::super::admin_fields::fields_form(model, plugin) {
        body.push(fields);
    }
    widget(style::top_rule(pad(column(12.0), 12.0, 0.0, 0.0, 0.0)))
        .children(body)
        .key(format!("admin-plugin-settings-section-{pid}"))
        .into_any()
}

/// 删除确认要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PluginDeleteView {
    pub message: String,
    /// 插件操作进行中：按钮禁用、确认写「删除中...」，三种关闭手势都不关。
    pub managing: bool,
}

impl PluginDeleteView {
    pub(crate) fn project(model: &ShellViewModel) -> Option<Self> {
        let plugin_id = model.admin.pending_delete.as_ref()?;
        let name = model
            .admin
            .plugins
            .iter()
            .find(|plugin| &plugin.plugin_id == plugin_id)
            .map(|plugin| plugin.name.clone())
            .unwrap_or_else(|| plugin_id.clone());
        Some(Self { message: format!("删除插件“{name}”后将移除其 .momoplug 安装包。"), managing: model.admin.managing })
    }
}

/// 删除确认浮层。Vue 的 `ConfirmDialog`：标题「删除插件」，确认「删除」，忙时「删除中...」。
/// 外壳走统一对话框框架，常驻，忙碌和文案按字段原地改。
pub(crate) fn delete_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let view = signal(PluginDeleteView::project(model)?);
    Projected::register(view, PluginDeleteView::project);
    let busy = move || view.with(|view| view.managing);
    let cancel = intent_button("取消", ButtonKind::Ghost, busy, "admin-plugin-cancel-delete");
    let confirm = intent_button(
        move || if busy() { "删除中..." } else { "删除" }.to_string(),
        ButtonKind::Danger,
        busy,
        "admin-plugin-confirm-delete",
    );
    Some(
        DialogFrame::new("admin-plugin-delete-dialog", || "删除插件".to_string(), || ShellMessage::Admin(AdminMessage::CancelDelete))
            .busy(busy)
            .confirm(move || view.with(|view| view.message.clone()), cancel, confirm, || ShellMessage::Admin(AdminMessage::ConfirmDelete)),
    )
}
