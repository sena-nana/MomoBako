//! 系统日志面板。
//!
//! 照 `WorkspaceLogsPanel.vue`：页头带缓存数和命中数，工具条是搜索、插件和仓库下拉、
//! 暂停追踪、重置筛选、清空日志，下面是级别和来源两组筛选芯片，再下面是日志卡片列表。
//! 每条日志一张主背景卡片：级别徽章、来源芯片、时间、动作、消息、上下文标签和可展开的上下文。

use std::sync::Arc;

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, JustifySpec, LengthSpec, Select, SelectChanged, SelectOption, Stack, TextChanged, TextInput,
};
use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole as Role};

use super::super::{ShellMessage, ShellViewModel};
use super::icons;
use super::style::{self, action, column, label, mono, pad, row, wrapping, Soft, SoftFill, Tone};
use super::support;
use super::AdminMessage;
use crate::backend::services::repository::SystemLogRecord;

const LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];
const KINDS: [&str; 5] = ["host", "frontend-host", "frontend-plugin", "backend-plugin", "helper"];

/// 整个日志面板。
pub(crate) fn logs_panel(model: &ShellViewModel) -> AnyView {
    let admin = &model.admin;
    let filtered = admin.filtered_logs();
    let mut body = vec![
        style::workbench_header(
            "Logs",
            "系统日志",
            "统一查看宿主、插件与辅助进程的实时日志流。",
            vec![
                style::stat(format!("{} 条缓存", admin.logs.len()), "admin-log-cached"),
                style::stat(format!("{} 条命中", filtered.len()), "admin-log-hits"),
            ],
            "admin-log",
        ),
        toolbar(model),
        filters(model),
    ];
    if admin.logs.is_empty() {
        body.push(style::dashed_empty("还没有系统日志", "宿主、插件和辅助进程产生的关键操作会在这里持续汇总。", "admin-log-empty"));
    } else if filtered.is_empty() {
        body.push(style::dashed_empty("当前筛选没有命中", "保留最近日志缓存，调整级别、来源或关键字后可以继续查看。", "admin-log-empty"));
    } else {
        let keys = style::unique_keys(filtered.iter().map(|record| record.id.as_str()));
        let items = filtered.iter().zip(&keys).map(|(record, key)| log_item(model, record, key)).collect::<Vec<_>>();
        body.push(widget(column(10.0).with_layout(|layout| layout.min_height = Some(LengthSpec::Px(360.0)))).children(items).key("admin-log-list").into_any());
    }
    style::workbench_panel(body, "admin-log-panel")
}

/// 工具条：放不下时折行，搜索框基准 320 并伸展。
fn toolbar(model: &ShellViewModel) -> AnyView {
    let admin = &model.admin;
    let paused = admin.log_paused;
    let search = widget(style::field_frame().with_layout(|layout| {
        layout.width = Some(LengthSpec::Shrink);
        layout.flex_basis = Some(LengthSpec::Px(320.0));
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }))
    .children((
        widget(style::glyph(icons::SEARCH, 15.0, Role::Muted)),
        widget(style::bare_input(TextInput::new(admin.log_search.clone()).label("搜索日志").placeholder("搜索消息、动作、位置或上下文")))
            .key("admin-log-search")
            .on_cx(|_, event: &TextChanged, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetLogSearch(event.value.to_string())));
            }),
    ))
    .key("admin-log-search-field")
    .into_any();
    let plugins = support::unique_sorted(admin.logs.iter().filter_map(|record| record.source.plugin_id.clone()));
    let repos = support::unique_sorted(admin.logs.iter().filter_map(|record| record.source.repo_id.clone()));
    let plugin_select = select_field("插件", "全部插件", plugins, &admin.log_plugin_id, AdminMessage::SetLogPlugin, "admin-log-plugin");
    let repo_select = select_field("仓库", "全部仓库", repos, &admin.log_repo_id, AdminMessage::SetLogRepo, "admin-log-repo");
    let active = support::active_filter_count(&admin.log_levels, &admin.log_kinds, &admin.log_plugin_id, &admin.log_repo_id, &admin.log_search);
    let pause = widget(action(if paused { "恢复追踪" } else { "暂停追踪" }, Some(if paused { icons::PLAY } else { icons::PAUSE }), Tone::Plain, false))
        .key("admin-log-pause")
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetLogPaused(!paused))))
        .into_any();
    let reset = widget(action("重置筛选", Some(icons::ERASER), Tone::Plain, active == 0))
        .key("admin-log-reset")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ResetLogFilters)))
        .into_any();
    let clear = widget(action("清空日志", Some(icons::TRASH2), Tone::Plain, admin.logs.is_empty()))
        .key("clear-logs")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::ClearLogs))
        .into_any();
    widget(Stack::fill_row(10.0).wrap(true).grow(0.0).align(AlignSpec::Start))
        .children(vec![search, plugin_select, repo_select, pause, reset, clear])
        .key("admin-log-toolbar")
        .into_any()
}

/// `.logs-workbench__select`：框里左边 12/700 弱色标题，右边透明下拉框，最小宽 180。
fn select_field(
    title: &str,
    all: &str,
    values: Vec<String>,
    current: &str,
    message: fn(String) -> AdminMessage,
    key: &'static str,
) -> AnyView {
    let mut options = vec![SelectOption::new("", all.to_string())];
    options.extend(values.into_iter().map(|value| SelectOption::new(value.clone(), value)));
    let mut select = Select::new(Some(current.to_string())).options(options);
    {
        let style = &mut select.style;
        style.interaction.base.background_mix = Some(SemanticColorMix::alpha(Role::Background, 0.0));
        style.interaction.base.border_mix = Some(SemanticColorMix::alpha(Role::Border, 0.0));
        let layout = Arc::make_mut(&mut style.layout);
        // Vue 的下拉框是 `width: 100%`，在最小 180 宽的框里扣掉标题和间距后约 122 宽。
        layout.width = Some(LengthSpec::Shrink);
        layout.min_width = Some(LengthSpec::Px(122.0));
        layout.height = Some(LengthSpec::Px(32.0));
        layout.font_size = Some(14.0);
        layout.padding_left = Some(LengthSpec::Px(0.0));
    }
    let frame = style::field_frame().with_layout(|layout| {
        layout.width = Some(LengthSpec::Shrink);
        layout.min_width = Some(LengthSpec::Px(180.0));
        layout.flex_grow = Some(0.0);
        layout.flex_shrink = Some(0.0);
        layout.gap = Some(LengthSpec::Px(10.0));
        layout.justify_content = JustifySpec::SpaceBetween;
    });
    widget(frame)
        .children((
            widget(label(title, 12.0, 700, Role::Muted)),
            widget(select).key(key).on_cx(move |_, event: &SelectChanged, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(message(event.value.to_string())));
            }),
        ))
        .key(format!("{key}-field"))
        .into_any()
}

/// 级别和来源两组芯片，标题列 42 宽。
fn filters(model: &ShellViewModel) -> AnyView {
    let admin = &model.admin;
    let levels = LEVELS
        .into_iter()
        .map(|value| {
            let active = admin.log_levels.iter().any(|level| level == value);
            filter_chip(&support::level_label(value), active, AdminMessage::ToggleLogLevel(value.into()), format!("admin-log-level-{value}"))
        })
        .collect::<Vec<_>>();
    let kinds = KINDS
        .into_iter()
        .map(|value| {
            let active = admin.log_kinds.iter().any(|kind| kind == value);
            filter_chip(&support::source_kind_label(value), active, AdminMessage::ToggleLogKind(value.into()), format!("admin-log-kind-{value}"))
        })
        .collect::<Vec<_>>();
    widget(column(10.0))
        .children((filter_group("级别", levels), filter_group("来源", kinds)))
        .key("admin-log-filters")
        .into_any()
}

fn filter_group(title: &str, chips: Vec<AnyView>) -> AnyView {
    widget(Stack::bar(10.0).align(AlignSpec::Start))
        .children((
            widget(pad(style::fixed(column(0.0), Some(42.0), None), 5.0, 0.0, 0.0, 0.0)).children((widget(label(title, 12.0, 700, Role::Muted)),)),
            widget(Stack::row(8.0).wrap(true).width(LengthSpec::Shrink).grow(1.0).shrink(1.0).min_width(LengthSpec::Px(0.0))).children(chips),
        ))
        .into_any()
}

/// `.workspace-filter-chip`：高 26、左右 9、药丸、1px 边线、主背景、12/500 弱色；
/// 选中时强调色边线、`accent-soft` 底和强调色字。Vue 里全局 `button { height: 32px }` 把它
/// 顶成 32，和左侧标题的 5px 顶距对不齐；这里按组件声明的 26 画。
fn filter_chip(text: &str, active: bool, message: AdminMessage, key: String) -> AnyView {
    let (foreground, background, border) = if active {
        (Role::Accent, Role::AccentSoft, Role::Accent)
    } else {
        (Role::Muted, Role::Background, Role::Border)
    };
    let mut button = Button::new(text.to_string());
    let mut node = nana_ui::runtime::NodeStyle {
        foreground: Some(foreground),
        background: Some(background),
        border: Some(border),
        interaction: nana_ui::runtime::InteractionStyle {
            hovered: nana_ui::runtime::SemanticPaint {
                foreground: Some(Role::Accent),
                background: Some(Role::AccentSoft),
                border: Some(Role::Accent),
                ..Default::default()
            },
            ..Default::default()
        },
        text_horizontal_alignment: nana_ui::runtime::TextHorizontalAlignment::Center,
        text_vertical_alignment: nana_ui::runtime::TextVerticalAlignment::Center,
        ..Default::default()
    };
    {
        let layout = Arc::make_mut(&mut node.layout);
        layout.height = Some(LengthSpec::Px(26.0));
        layout.min_height = Some(LengthSpec::Px(26.0));
        layout.padding_left = Some(LengthSpec::Px(9.0));
        layout.padding_right = Some(LengthSpec::Px(9.0));
        layout.border_width = Some(1.0);
        layout.border_radius = Some(style::PILL);
        layout.font_size = Some(12.0);
        layout.font_weight = Some(500);
        layout.line_height = Some(nana_ui_core::LineHeightSpec::Relative(style::LINE));
        layout.white_space_nowrap = true;
        layout.flex_shrink = Some(0.0);
    }
    button = button.style(node);
    widget(button)
        .key(key)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(message.clone())))
        .into_any()
}

/// 一条日志。
fn log_item(model: &ShellViewModel, record: &SystemLogRecord, id: &str) -> AnyView {
    let mut badges = vec![level_badge(&record.level, id), style::hint_chip(support::source_kind_label(&record.source.kind), format!("admin-log-kind-chip-{id}"))];
    if let Some(source_label) = record.source.label.as_deref().filter(|text| !text.is_empty()) {
        badges.push(style::hint_chip(source_label, format!("admin-log-source-{id}")));
    }
    let head = widget(style::spread(12.0, AlignSpec::Center))
        .children((
            widget(row(8.0).wrap(true).shrink(1.0)).children(badges),
            widget(mono(support::log_time_label(&record.timestamp), 12.0, 400, Role::Faint)).key(format!("admin-log-time-{id}")),
        ))
        .into_any();
    let mut meta = vec![meta_chip(record.category.clone())];
    if let Some(plugin_id) = record.source.plugin_id.as_deref() {
        meta.push(meta_chip(format!("插件 {plugin_id}")));
    }
    if let Some(repo_id) = record.source.repo_id.as_deref() {
        meta.push(meta_chip(format!("仓库 {repo_id}")));
    }
    let location = support::location_label(record);
    if !location.is_empty() {
        meta.push(meta_chip(format!("位置 {location}")));
    }
    let body = widget(column(8.0))
        .children((
            widget(column(6.0)).children((
                widget(label(record.action.clone(), 14.0, 700, Role::Text)).key(format!("admin-log-action-{id}")),
                widget(wrapping(style::label_lh(record.message.clone(), 14.0, 400, Role::Text, 1.6))).key(format!("admin-log-message-{id}")),
            )),
            widget(row(8.0).wrap(true).width(LengthSpec::Fill)).children(meta),
        ))
        .into_any();
    let mut children = vec![head, body];
    if record.context.as_object().is_some_and(|object| !object.is_empty()) {
        children.push(context_block(model, record));
    }
    let card = pad(column(12.0), 14.0, 16.0, 14.0, 16.0).surface(Role::Background).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Xl);
    widget(card).children(children).key(format!("admin-log-row-{id}")).into_any()
}

/// `.logs-workbench__level`：高 28、左右 12、药丸、12/700，底色按级别和主背景混色。
fn level_badge(level: &str, id: &str) -> AnyView {
    let (soft, color): (Soft, Option<[u8; 3]>) = match level {
        "debug" => (Soft::RgbMix { rgb: [0x5c, 0x70, 0x88], ratio: 0.18, base: Role::Background }, Some([0x5c, 0x70, 0x88])),
        "info" => (Soft::RoleMix { role: Role::Accent, ratio: 0.18, base: Role::Background }, None),
        "warn" => (Soft::RgbMix { rgb: [0xdc, 0xa3, 0x3a], ratio: 0.22, base: Role::Background }, Some([0xb6, 0x7a, 0x09])),
        _ => (Soft::RoleMix { role: Role::Danger, ratio: 0.18, base: Role::Background }, None),
    };
    let role = if level == "info" { Role::Accent } else { Role::Danger };
    let mut text = label(support::level_label(level), 12.0, 700, role);
    if let Some(rgb) = color {
        text = style::fixed_color(text, rgb);
    }
    let body = style::rounded(pad(style::fixed(row(0.0), None, Some(28.0)), 0.0, 12.0, 0.0, 12.0), None).painter(SoftFill::new(soft, None));
    widget(body).children((widget(text).key(format!("admin-log-level-badge-{id}")),)).into_any()
}

/// `.logs-workbench__meta span`：最小高 24、内边距 3/9、药丸、`bg-subtle`、等宽 11 号弱色。
fn meta_chip(text: String) -> AnyView {
    let body = style::rounded(pad(row(0.0), 3.0, 9.0, 3.0, 9.0), None).surface(Role::Subtle).with_layout(|layout| {
        layout.min_height = Some(LengthSpec::Px(24.0));
    });
    widget(body).children((widget(mono(text, 11.0, 400, Role::Muted)),)).into_any()
}

/// 可展开的上下文。收起时只有「上下文」标题，展开后是等宽 JSON。
fn context_block(model: &ShellViewModel, record: &SystemLogRecord) -> AnyView {
    let id = record.id.clone();
    let open = model.admin.log_context_open.contains(&id);
    let toggle_id = id.clone();
    let mut rows = vec![widget(Button::new("上下文").style(summary_style()))
        .key(format!("admin-log-context-{}", style::key_part(&id)))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ToggleLogContext(toggle_id.clone()))))
        .into_any()];
    if open {
        let text = serde_json::to_string_pretty(&record.context).unwrap_or_else(|_| record.context.to_string());
        rows.push(
            widget(pad(column(0.0), 12.0, 12.0, 12.0, 12.0).surface(Role::Subtle).radius(RadiusTier::Lg))
                .children((widget(wrapping(style::mono_lh(text, 12.0, 400, Role::Text, 1.6))),))
                .into_any(),
        );
    }
    widget(style::top_rule(pad(column(10.0), 10.0, 0.0, 0.0, 0.0))).children(rows).into_any()
}

/// 上下文标题按钮：12/700 弱色，无底无边，左对齐。
fn summary_style() -> nana_ui::runtime::NodeStyle {
    let mut node = nana_ui::runtime::NodeStyle {
        foreground: Some(Role::Muted),
        text_horizontal_alignment: nana_ui::runtime::TextHorizontalAlignment::Start,
        text_vertical_alignment: nana_ui::runtime::TextVerticalAlignment::Center,
        ..Default::default()
    };
    let layout = Arc::make_mut(&mut node.layout);
    layout.font_size = Some(12.0);
    layout.font_weight = Some(700);
    layout.line_height = Some(nana_ui_core::LineHeightSpec::Relative(style::LINE));
    layout.width = Some(LengthSpec::Shrink);
    node
}
