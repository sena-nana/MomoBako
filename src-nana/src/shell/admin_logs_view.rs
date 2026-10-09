//! 系统日志面板。
//!
//! 照 `WorkspaceLogsPanel.vue`：页头带缓存数和命中数，工具条是搜索、插件和仓库下拉、
//! 暂停追踪、重置筛选、清空日志，下面是级别和来源两组筛选芯片，再下面是日志卡片列表。
//! 每条日志一张主背景卡片：级别徽章、来源芯片、时间、动作、消息、上下文标签和可展开的上下文。
//!
//! 页头、工具条和筛选固定，日志列表（`.logs-workbench__list`）占满面板剩余高度、自己滚动：追踪时
//! 跟随末尾，暂停后停在原位。Vue 的列表不定高、实际滚不起来，这里按它的意图给列表定高。
//!
//! 面板只建一次，只读 [`LogsSignals`]：计数、下拉框、按钮和芯片的选中态按字段绑定；日志卡片按
//! 日志 id 做键，卡片里的文字、可有可无的芯片和上下文都是绑定和显隐，新日志到达只多建一张卡片。

use std::sync::Arc;

use nana_ui::runtime::view::{css, fields, node_ref, widget, AnyView, IntoView, Item, Signal, Store, StoreList, StorePath};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, JustifySpec, LengthSpec, NodeStyle, ScrollAxes, ScrollView, Select, SelectChanged, SelectOption,
    Stack, Text, TextChanged, TextInput,
};
use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole as Role};

use super::super::ShellMessage;
use super::bind::{row_flag, row_text, ActionDisabled, ButtonIcon, FollowEnd, StyleField};
use super::icons;
use super::logs_state::{LogRowView, LogsSignals, LogsToolbar};
use super::style::{self, action, column, label, mono, pad, row, wrapping, Soft, SoftFill, Tone};
use super::support;
use super::AdminMessage;

const LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];
const KINDS: [&str; 5] = ["host", "frontend-host", "frontend-plugin", "backend-plugin", "helper"];

/// 日志行在 Store 里的句柄。
type LogItem = Item<Store<Vec<LogRowView>>, String, LogRowView>;

/// 整个日志面板。
pub(crate) fn logs_panel(signals: LogsSignals) -> AnyView {
    let head = signals.head;
    let empty = signals.empty;
    let loading = signals.loading;
    let rows = signals.rows;
    let follow = signals.follow_end;
    let list = rows.keyed(LogRowView::key).each(log_item).gap(10.0).css(css! { padding-right: 4px; }).key("admin-log-list");
    let body = vec![
        style::workbench_header(
            "Logs",
            "系统日志",
            "统一查看宿主、插件与辅助进程的实时日志流。",
            vec![
                style::stat(move || head.with(|head| head.cached.clone()), "admin-log-cached"),
                style::stat(move || head.with(|head| head.hits.clone()), "admin-log-hits"),
            ],
            "admin-log",
        ),
        toolbar(signals),
        filters(signals.filters),
        style::state_notice("正在加载系统日志", false, "admin-log-loading").visible(move || loading.get()).into_any(),
        style::dashed_empty(
            move || empty.with(|empty| empty.map(|(title, _)| title.to_string()).unwrap_or_default()),
            move || empty.with(|empty| empty.map(|(_, detail)| detail.to_string()).unwrap_or_default()),
            "admin-log-empty",
        )
        .visible(move || empty.with(Option::is_some))
        .into_any(),
        widget(list_scroll())
            .prop::<bool, FollowEnd>(follow)
            .children((list,))
            .visible(move || !rows.is_empty())
            .key("admin-log-scroll")
            .into_any(),
    ];
    style::fill_panel(body, "admin-log-panel")
}

/// 日志列表的滚动区：分走面板里页头、工具条和筛选以外的高度。不设最小高（Vue 写的 360 建立在列表
/// 不定高上）：窗口矮到放不下时列表先让出高度，滚动区始终整个落在面板里，跟随末尾时最新一条看得见。
fn list_scroll() -> ScrollView {
    ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Fill);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.min_height = Some(LengthSpec::Px(0.0));
    })
}

/// 工具条：放不下时折行，搜索框基准 320 并伸展。搜索框受控。
fn toolbar(signals: LogsSignals) -> AnyView {
    let toolbar = signals.toolbar;
    let search_value = signals.search.signal();
    let search = widget(style::field_frame().with_layout(|layout| {
        layout.width = Some(LengthSpec::Shrink);
        layout.flex_basis = Some(LengthSpec::Px(320.0));
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }))
    .children((
        widget(style::glyph(icons::SEARCH, 15.0, Role::Muted)),
        widget(style::bare_input(TextInput::new(search_value.get_untracked()).label("搜索日志").placeholder("搜索消息、动作、位置或上下文")))
            .model(search_value)
            .key("admin-log-search")
            .on_cx(|_, event: &TextChanged, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetLogSearch(event.value.to_string())));
            }),
    ))
    .key("admin-log-search-field")
    .into_any();
    let plugin_select = select_field("插件", "全部插件", toolbar, |toolbar| (&toolbar.plugins, &toolbar.plugin), AdminMessage::SetLogPlugin, "admin-log-plugin");
    let repo_select = select_field("仓库", "全部仓库", toolbar, |toolbar| (&toolbar.repos, &toolbar.repo), AdminMessage::SetLogRepo, "admin-log-repo");
    let paused = move || toolbar.with(|toolbar| toolbar.paused);
    let pause = widget(action("暂停追踪", Some(icons::PAUSE), Tone::Plain, false))
        .prop::<String, fields::button::label>(move || if paused() { "恢复追踪" } else { "暂停追踪" }.to_string())
        .prop::<Option<nana_ui_core::Icon>, ButtonIcon>(move || Some(if paused() { icons::PLAY } else { icons::PAUSE }))
        .key("admin-log-pause")
        .on_cx(move |_, _: &Activate, cx| {
            // 追踪状态随归约变，点下去时现读。
            let paused = toolbar.with_untracked(|toolbar| toolbar.paused);
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetLogPaused(!paused)));
        })
        .into_any();
    let reset = widget(action("重置筛选", Some(icons::ERASER), Tone::Plain, false))
        .prop::<bool, ActionDisabled>(move || toolbar.with(|toolbar| toolbar.no_filters))
        .key("admin-log-reset")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ResetLogFilters)))
        .into_any();
    let clear = widget(action("清空日志", Some(icons::TRASH2), Tone::Plain, false))
        .prop::<bool, ActionDisabled>(move || toolbar.with(|toolbar| toolbar.no_logs))
        .key("clear-logs")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::ClearLogs))
        .into_any();
    widget(Stack::fill_row(10.0).wrap(true).grow(0.0).align(AlignSpec::Start))
        .children(vec![search, plugin_select, repo_select, pause, reset, clear])
        .key("admin-log-toolbar")
        .into_any()
}

/// `.logs-workbench__select`：框里左边 12/700 弱色标题，右边透明下拉框，最小宽 180。
/// 选项和当前值跟着工具条信号；下拉框用左边的标题当读屏名称。
fn select_field(
    title: &str,
    all: &'static str,
    toolbar: Signal<LogsToolbar>,
    pick: fn(&LogsToolbar) -> (&Vec<String>, &String),
    message: fn(String) -> AdminMessage,
    key: &'static str,
) -> AnyView {
    let options = move |values: &[String]| {
        let mut options = vec![SelectOption::new("", all.to_string())];
        options.extend(values.iter().map(|value| SelectOption::new(value.clone(), value.clone())));
        options
    };
    let initial = toolbar.get_untracked();
    let (values, current) = pick(&initial);
    let mut select = Select::new(Some(current.clone())).options(options(values));
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
    let caption = node_ref();
    widget(frame)
        .children((
            widget(label(title, 12.0, 700, Role::Muted)).node_ref(caption),
            widget(select)
                .prop::<Vec<SelectOption>, fields::select::options>(move || toolbar.with(|toolbar| options(pick(toolbar).0)))
                .prop::<Option<Arc<str>>, fields::select::value>(move || toolbar.with(|toolbar| Some(Arc::from(pick(toolbar).1.as_str()))))
                .labelled_by(caption)
                .key(key)
                .on_cx(move |_, event: &SelectChanged, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(message(event.value.to_string())));
                }),
        ))
        .key(format!("{key}-field"))
        .into_any()
}

/// 级别和来源两组芯片，标题列 42 宽。芯片固定，选中态绑在已选的级别和来源上。
fn filters(selected: Signal<(Vec<String>, Vec<String>)>) -> AnyView {
    let levels = LEVELS
        .into_iter()
        .map(|value| {
            let active = move || selected.with(|(levels, _)| levels.iter().any(|level| level == value));
            filter_chip(&support::level_label(value), active, AdminMessage::ToggleLogLevel(value.into()), format!("admin-log-level-{value}"))
        })
        .collect::<Vec<_>>();
    let kinds = KINDS
        .into_iter()
        .map(|value| {
            let active = move || selected.with(|(_, kinds)| kinds.iter().any(|kind| kind == value));
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

/// 芯片常态的字色、底色和描边：选中是强调色，未选是主背景、弱色字。
fn chip_roles(active: bool) -> (Role, Role, Role) {
    if active { (Role::Accent, Role::AccentSoft, Role::Accent) } else { (Role::Muted, Role::Background, Role::Border) }
}

/// `.workspace-filter-chip`：高 26、左右 9、药丸、1px 边线、主背景、12/500 弱色；
/// 选中时强调色边线、`accent-soft` 底和强调色字。Vue 里全局 `button { height: 32px }` 把它
/// 顶成 32，和左侧标题的 5px 顶距对不齐；这里按组件声明的 26 画。
fn filter_chip(text: &str, active: impl Fn() -> bool + Send + Copy + 'static, message: AdminMessage, key: String) -> AnyView {
    let (foreground, background, border) = chip_roles(active());
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
    widget(Button::new(text.to_string()).style(node))
        .foreground(move || Some(chip_roles(active()).0))
        .background(move || Some(chip_roles(active()).1))
        .border(move || Some(chip_roles(active()).2))
        .key(key)
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(message.clone())))
        .into_any()
}

/// 一条日志。卡片按日志 id 只建一次，文字、可有可无的芯片和上下文都绑在这一行上。
fn log_item(item: LogItem) -> AnyView {
    let id = item.get_untracked().id;
    let key = style::key_part(&id);
    let source = style::hint_chip(row_text(item, |row| &row.source), format!("admin-log-source-{key}"))
        .visible(row_flag(item, |row| !row.source.is_empty()))
        .into_any();
    let badges = vec![
        level_badge(item, &key),
        style::hint_chip(row_text(item, |row| &row.kind), format!("admin-log-kind-chip-{key}")).into_any(),
        source,
    ];
    let head = widget(style::spread(12.0, AlignSpec::Center))
        .children((
            widget(row(8.0).wrap(true).shrink(1.0)).children(badges),
            style::bound(mono(String::new(), 12.0, 400, Role::Faint), row_text(item, |row| &row.time)).key(format!("admin-log-time-{key}")),
        ))
        .into_any();
    let meta = widget(row(8.0).wrap(true).width(LengthSpec::Fill)).children((
        meta_chip(item, |row| &row.category, false),
        meta_chip(item, |row| &row.plugin, true),
        meta_chip(item, |row| &row.repo, true),
        meta_chip(item, |row| &row.location, true),
    ));
    let body = widget(column(8.0))
        .children((
            widget(column(6.0)).children((
                style::bound(label(String::new(), 14.0, 700, Role::Text), row_text(item, |row| &row.action)).key(format!("admin-log-action-{key}")),
                style::bound(wrapping(style::label_lh(String::new(), 14.0, 400, Role::Text, 1.6)), row_text(item, |row| &row.message))
                    .key(format!("admin-log-message-{key}")),
            )),
            meta,
        ))
        .into_any();
    let card = pad(column(12.0), 14.0, 16.0, 14.0, 16.0).surface(Role::Background).outline(Role::BorderSoft, 1.0).radius(RadiusTier::Xl);
    widget(card).children((head, body, context_block(item, &id))).key(format!("admin-log-row-{key}")).into_any()
}

/// 级别徽章的外框和字样：底色按级别和主背景混色，警告和调试用 Vue 写死的颜色。
fn level_look(level: &str) -> (NodeStyle, Text) {
    let (soft, color): (Soft, Option<[u8; 3]>) = match level {
        "debug" => (Soft::RgbMix { rgb: [0x5c, 0x70, 0x88], ratio: 0.18, base: Role::Background }, Some([0x5c, 0x70, 0x88])),
        "info" => (Soft::RoleMix { role: Role::Accent, ratio: 0.18, base: Role::Background }, None),
        "warn" => (Soft::RgbMix { rgb: [0xdc, 0xa3, 0x3a], ratio: 0.22, base: Role::Background }, Some([0xb6, 0x7a, 0x09])),
        _ => (Soft::RoleMix { role: Role::Danger, ratio: 0.18, base: Role::Background }, None),
    };
    let role = if level == "info" { Role::Accent } else { Role::Danger };
    let mut text = label(String::new(), 12.0, 700, role);
    if let Some(rgb) = color {
        text = style::fixed_color(text, rgb);
    }
    let body = style::rounded(pad(style::fixed(row(0.0), None, Some(28.0)), 0.0, 12.0, 0.0, 12.0), None).painter(SoftFill::new(soft, None));
    (body.node_style(), text)
}

/// `.logs-workbench__level`：高 28、左右 12、药丸、12/700，底色按级别和主背景混色。
fn level_badge(item: LogItem, key: &str) -> AnyView {
    let level = move || item.try_with(|row| row.level.clone()).unwrap_or_default();
    let (frame, text) = level_look(&level());
    widget(Stack::row(0.0).style(frame))
        .prop::<NodeStyle, StyleField>(move || level_look(&level()).0)
        .children((style::bound(text, row_text(item, |row| &row.level_label))
            .prop::<NodeStyle, StyleField>(move || level_look(&level()).1.style)
            .key(format!("admin-log-level-badge-{key}")),))
        .into_any()
}

/// `.logs-workbench__meta span`：最小高 24、内边距 3/9、药丸、`bg-subtle`、等宽 11 号弱色。
/// 分类总在；插件、仓库和位置是 `optional`，值为空时不显示。
fn meta_chip(item: LogItem, pick: fn(&LogRowView) -> &String, optional: bool) -> AnyView {
    let body = style::rounded(pad(row(0.0), 3.0, 9.0, 3.0, 9.0), None).surface(Role::Subtle).with_layout(|layout| {
        layout.min_height = Some(LengthSpec::Px(24.0));
    });
    widget(body)
        .visible(move || !optional || item.try_with(|row| !pick(row).is_empty()).unwrap_or(false))
        .children((style::bound(mono(String::new(), 11.0, 400, Role::Muted), row_text(item, pick)),))
        .into_any()
}

/// 可展开的上下文。收起时只有「上下文」标题，展开后是等宽 JSON；没有上下文时整块不占布局。
fn context_block(item: LogItem, id: &str) -> AnyView {
    let toggle_id = id.to_string();
    let summary = widget(Button::new("上下文").style(summary_style()))
        .key(format!("admin-log-context-{}", style::key_part(id)))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ToggleLogContext(toggle_id.clone()))));
    let json = widget(pad(column(0.0), 12.0, 12.0, 12.0, 12.0).surface(Role::Subtle).radius(RadiusTier::Lg))
        .visible(row_flag(item, |row| row.context_open))
        .children((style::bound(wrapping(style::mono_lh(String::new(), 12.0, 400, Role::Text, 1.6)), row_text(item, |row| &row.context)),));
    widget(style::top_rule(pad(column(10.0), 10.0, 0.0, 0.0, 0.0)))
        .visible(row_flag(item, |row| row.has_context))
        .children((summary, json))
        .into_any()
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
