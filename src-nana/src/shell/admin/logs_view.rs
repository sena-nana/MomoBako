//! 系统日志面板。
//!
//! 页头带缓存数和命中数（NanaUI 标签），工具条是搜索、插件和仓库下拉、
//! 暂停追踪、重置筛选、清空日志，下面是级别和来源两组筛选芯片（NanaUI `Chip`），再下面是日志卡片列表。
//! 读历史时是 NanaUI 的转圈，没有日志时是 NanaUI 的 `EmptyState`。
//! 每条日志一张主背景卡片：级别徽章（`StatusBadge`）、种类和来源标签、时间、动作、消息、上下文标签和
//! 可展开的上下文。
//!
//! 页面主体和日志列表都是滚动区。
//! 面板至少撑满主体，列表分走页头、工具条和筛选以下的高度、自己滚动：追踪时跟随末尾，暂停后停在原位。
//! 列表要定高才能跟随末尾。列表最矮收到 [`LIST_FLOOR`]（有意小于原来的下限
//! 360），常见的矮窗口里追踪时最新一条不用滚页面就看得到；矮到连下限都放不下时主体滚动，页头和列表都滚得到。
//!
//! 面板只建一次，只读 [`LogsSignals`]：计数、下拉框、按钮和芯片的选中态按字段绑定；日志卡片按
//! 日志 id 做键，卡片里的文字、可有可无的芯片和上下文都是绑定和显隐，新日志到达只多建一张卡片。

use std::sync::Arc;

use nana_ui::runtime::view::{
    AnyView, IntoView, Item, Signal, Store, StoreList, StorePath, chip, css, fields, node_ref,
    status_badge, tag_frame, widget,
};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, JustifySpec, LengthSpec, ScrollAxes, ScrollView, Select,
    SelectChanged, SelectOption, Stack, StatusTone, TextChanged, TextInput,
};
use nana_ui_core::{
    ButtonKind, RadiusTier, SemanticColorMix, SemanticColorRole as Role, TextWeight, TypeRole,
};

use super::super::ShellMessage;
use super::AdminMessage;
use super::bind::{ActionDisabled, ButtonIcon, FollowEnd, row_flag, row_text};
use super::icons;
use super::logs_state::{LogRowView, LogsSignals, LogsToolbar};
use super::marks;
use super::style::{self, action, column, label, mono, pad, row, wrapping};
use super::support;

const LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];
const KINDS: [&str; 5] = [
    "host",
    "frontend-host",
    "frontend-plugin",
    "backend-plugin",
    "helper",
];

/// 日志行在 Store 里的句柄。
type LogItem = Item<Store<Vec<LogRowView>>, String, LogRowView>;

/// 整个日志面板。
pub(crate) fn logs_panel(signals: LogsSignals) -> AnyView {
    let head = signals.head;
    let empty = signals.empty;
    let loading = signals.loading;
    let rows = signals.rows;
    let follow = signals.follow_end;
    let list = rows
        .keyed(LogRowView::key)
        .each(log_item)
        .gap(10.0)
        .css(css! { padding-right: 4px; })
        .key("admin-log-list");
    let body = vec![
        style::workbench_header(
            "Logs",
            "系统日志",
            "统一查看宿主、插件与辅助进程的实时日志流。",
            vec![
                marks::bound_tag(
                    move || head.with(|head| head.cached.clone()),
                    "admin-log-cached",
                )
                .into_any(),
                marks::bound_tag(
                    move || head.with(|head| head.hits.clone()),
                    "admin-log-hits",
                )
                .into_any(),
            ],
            "admin-log",
        ),
        toolbar(signals),
        filters(signals.filters),
        marks::loading("正在加载系统日志", "admin-log-loading")
            .visible(move || loading.get())
            .into_any(),
        marks::empty(
            move || {
                empty.with(|empty| {
                    empty
                        .map(|(title, _)| title.to_string())
                        .unwrap_or_default()
                })
            },
            move || {
                empty.with(|empty| {
                    empty
                        .map(|(_, detail)| detail.to_string())
                        .unwrap_or_default()
                })
            },
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
    style::workbench_panel(body, "admin-log-panel")
}

/// 日志列表最矮收到多高。原来的下限是 360，这里有意改小：
/// 一张单行消息的日志卡片从消息行到卡片底边约 70，列表跟到末尾时要把最新一条的消息和标签露出来，
/// 取 72。960×480 的窗口里页头、工具条和筛选以下还剩 74，放得下，最新一条不用滚页面就看得到；
/// 窗口矮到连这 72 都放不下时主体才滚。
pub(crate) const LIST_FLOOR: f32 = 72.0;

/// 日志列表的滚动区：从 [`LIST_FLOOR`] 起伸展，分走面板里页头、工具条和筛选以外的高度。
/// 基准写定这个下限，面板按内容量高度时列表只算下限、不按里面的日志算；里面的日志再多也只把列表自己的
/// 盒子算进主体的滚动范围，窗口放得下时主体不滚。
fn list_scroll() -> ScrollView {
    ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Px(LIST_FLOOR));
        layout.min_height = Some(LengthSpec::Px(LIST_FLOOR));
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(0.0);
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
        widget(style::bare_input(
            TextInput::new(search_value.get_untracked())
                .label("搜索日志")
                .placeholder("搜索消息、动作、位置或上下文"),
        ))
        .model(search_value)
        .key("admin-log-search")
        .on_cx(|_, event: &TextChanged, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetLogSearch(
                event.value.to_string(),
            )));
        }),
    ))
    .key("admin-log-search-field")
    .into_any();
    let plugin_select = select_field(
        "插件",
        "全部插件",
        toolbar,
        |toolbar| (&toolbar.plugins, &toolbar.plugin),
        AdminMessage::SetLogPlugin,
        "admin-log-plugin",
    );
    let repo_select = select_field(
        "仓库",
        "全部仓库",
        toolbar,
        |toolbar| (&toolbar.repos, &toolbar.repo),
        AdminMessage::SetLogRepo,
        "admin-log-repo",
    );
    let paused = move || toolbar.with(|toolbar| toolbar.paused);
    let pause = widget(action(
        "暂停追踪",
        Some(icons::PAUSE),
        ButtonKind::Ghost,
        false,
    ))
    .prop::<String, fields::button::label>(move || {
        if paused() {
            "恢复追踪"
        } else {
            "暂停追踪"
        }
        .to_string()
    })
    .prop::<Option<nana_ui_core::Icon>, ButtonIcon>(move || {
        Some(if paused() { icons::PLAY } else { icons::PAUSE })
    })
    .key("admin-log-pause")
    .on_cx(move |_, _: &Activate, cx| {
        // 追踪状态随归约变，点下去时现读。
        let paused = toolbar.with_untracked(|toolbar| toolbar.paused);
        cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetLogPaused(!paused)));
    })
    .into_any();
    let reset = widget(action(
        "重置筛选",
        Some(icons::ERASER),
        ButtonKind::Ghost,
        false,
    ))
    .prop::<bool, ActionDisabled>(move || toolbar.with(|toolbar| toolbar.no_filters))
    .key("admin-log-reset")
    .on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ResetLogFilters))
    })
    .into_any();
    let clearing = move || toolbar.with(|toolbar| toolbar.clearing);
    let clear = widget(action(
        "清空日志",
        Some(icons::TRASH2),
        ButtonKind::Ghost,
        false,
    ))
    .prop::<bool, ActionDisabled>(move || toolbar.with(|toolbar| toolbar.clear_disabled))
    .prop::<bool, fields::button::loading>(clearing)
    .key("clear-logs")
    .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::ClearLogs))
    .into_any();
    widget(
        Stack::fill_row(10.0)
            .wrap(true)
            .grow(0.0)
            .align(AlignSpec::Start),
    )
    .children(vec![
        search,
        plugin_select,
        repo_select,
        pause,
        reset,
        clear,
    ])
    .key("admin-log-toolbar")
    .into_any()
}

/// 日志下拉框：框里左边 12/700 弱色标题，右边透明下拉框，最小宽 180。
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
        options.extend(
            values
                .iter()
                .map(|value| SelectOption::new(value.clone(), value.clone())),
        );
        options
    };
    let initial = toolbar.get_untracked();
    let (values, current) = pick(&initial);
    // 照原生 `<select>` 按最长的选项定宽：选哪一项外框都不动，撑到选项需要的宽度。
    let mut select = Select::new(Some(current.clone()))
        .options(options(values))
        .fit_options(true);
    {
        let style = &mut select.style;
        style.interaction.base.background_mix =
            Some(SemanticColorMix::alpha(Role::Background, 0.0));
        style.interaction.base.border_mix = Some(SemanticColorMix::alpha(Role::Border, 0.0));
        let layout = Arc::make_mut(&mut style.layout);
        // 宽度按内容（最长的选项加箭头），下限是最小 180 宽的框扣掉标题和间距后的
        // 约 122 宽，选项都很短时不窄于它。
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
            widget(label(title, TypeRole::Meta, TextWeight::Bold, Role::Muted)).node_ref(caption),
            widget(select)
                .prop::<Vec<SelectOption>, fields::select::options>(move || {
                    toolbar.with(|toolbar| options(pick(toolbar).0))
                })
                .prop::<Option<Arc<str>>, fields::select::value>(move || {
                    toolbar.with(|toolbar| Some(Arc::from(pick(toolbar).1.as_str())))
                })
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
            let active =
                move || selected.with(|(levels, _)| levels.iter().any(|level| level == value));
            filter_chip(
                &support::level_label(value),
                active,
                AdminMessage::ToggleLogLevel(value.into()),
                format!("admin-log-level-{value}"),
            )
        })
        .collect::<Vec<_>>();
    let kinds = KINDS
        .into_iter()
        .map(|value| {
            let active = move || selected.with(|(_, kinds)| kinds.iter().any(|kind| kind == value));
            filter_chip(
                &support::source_kind_label(value),
                active,
                AdminMessage::ToggleLogKind(value.into()),
                format!("admin-log-kind-{value}"),
            )
        })
        .collect::<Vec<_>>();
    widget(column(8.0))
        .children((filter_group("级别", levels), filter_group("来源", kinds)))
        .key("admin-log-filters")
        .into_any()
}

fn filter_group(title: &str, chips: Vec<AnyView>) -> AnyView {
    widget(Stack::bar(10.0).align(AlignSpec::Start))
        .children((
            widget(pad(
                style::fixed(column(0.0), Some(42.0), None),
                5.0,
                0.0,
                0.0,
                0.0,
            ))
            .children((widget(label(
                title,
                TypeRole::Meta,
                TextWeight::Bold,
                Role::Muted,
            )),)),
            widget(
                Stack::row(8.0)
                    .wrap(true)
                    .width(LengthSpec::Shrink)
                    .grow(1.0)
                    .shrink(1.0)
                    .min_width(LengthSpec::Px(0.0)),
            )
            .children(chips),
        ))
        .into_any()
}

/// 筛选芯片：NanaUI 的 `Chip`，选中态绑在已选的级别和来源上；点了切换这一项。
fn filter_chip(
    text: &str,
    active: impl Fn() -> bool + Send + Copy + 'static,
    message: AdminMessage,
    key: String,
) -> AnyView {
    chip(text.to_string())
        .selected(active)
        .key(key)
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(message.clone()))
        })
        .into_any()
}

/// 一条日志。卡片按日志 id 只建一次，文字、可有可无的芯片和上下文都绑在这一行上。
fn log_item(item: LogItem) -> AnyView {
    let id = item.get_untracked().id;
    let key = style::key_part(&id);
    let source = marks::bound_tag(
        row_text(item, |row| &row.source),
        format!("admin-log-source-{key}"),
    )
    .visible(row_flag(item, |row| !row.source.is_empty()))
    .into_any();
    let badges = vec![
        level_badge(item, &key),
        marks::bound_tag(
            row_text(item, |row| &row.kind),
            format!("admin-log-kind-chip-{key}"),
        )
        .into_any(),
        source,
    ];
    let head = widget(style::spread(12.0, AlignSpec::Center))
        .children((
            widget(row(8.0).wrap(true).shrink(1.0)).children(badges),
            style::bound(
                mono(
                    String::new(),
                    TypeRole::Meta,
                    TextWeight::Regular,
                    Role::Faint,
                ),
                row_text(item, |row| &row.time),
            )
            .key(format!("admin-log-time-{key}")),
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
                style::bound(
                    label(
                        String::new(),
                        TypeRole::Section,
                        TextWeight::Bold,
                        Role::Text,
                    ),
                    row_text(item, |row| &row.action),
                )
                .key(format!("admin-log-action-{key}")),
                style::bound(
                    wrapping(style::label_lh(
                        String::new(),
                        TypeRole::Section,
                        TextWeight::Regular,
                        Role::Text,
                        1.6,
                    )),
                    row_text(item, |row| &row.message),
                )
                .key(format!("admin-log-message-{key}")),
            )),
            meta,
        ))
        .into_any();
    let card = pad(column(12.0), 14.0, 16.0, 14.0, 16.0)
        .surface(Role::Background)
        .border(Role::BorderSoft, 1.0)
        .radius(RadiusTier::Xl);
    widget(card)
        .children((head, body, context_block(item, &id)))
        .key(format!("admin-log-row-{key}"))
        .into_any()
}

/// 级别徽章的色调：调试中性、信息、警告、错误各一档。
fn level_tone(level: &str) -> StatusTone {
    match level {
        "debug" => StatusTone::Neutral,
        "info" => StatusTone::Info,
        "warn" => StatusTone::Warning,
        _ => StatusTone::Danger,
    }
}

/// 级别徽章：NanaUI 的 `StatusBadge`，文字是级别名，色调按级别。
fn level_badge(item: LogItem, key: &str) -> AnyView {
    let text = row_text(item, |row| &row.level_label);
    status_badge(move || std::sync::Arc::<str>::from(text()))
        .tone(move || level_tone(&item.try_with(|row| row.level.clone()).unwrap_or_default()))
        .key(format!("admin-log-level-badge-{key}"))
        .into_any()
}

/// 日志元信息标签：NanaUI 的 `tag_frame`，里面是等宽 11 号弱色字。
/// 分类总在；插件、仓库和位置是 `optional`，值为空时不显示。
fn meta_chip(item: LogItem, pick: fn(&LogRowView) -> &String, optional: bool) -> AnyView {
    widget(tag_frame())
        .visible(move || !optional || item.try_with(|row| !pick(row).is_empty()).unwrap_or(false))
        .children((style::bound(
            mono(
                String::new(),
                TypeRole::Hint,
                TextWeight::Regular,
                Role::Muted,
            ),
            row_text(item, pick),
        ),))
        .into_any()
}

/// 可展开的上下文。收起时只有「上下文」标题，展开后是等宽 JSON；没有上下文时整块不占布局。
fn context_block(item: LogItem, id: &str) -> AnyView {
    let toggle_id = id.to_string();
    let summary = widget(Button::new("上下文").style(summary_style()))
        .key(format!("admin-log-context-{}", style::key_part(id)))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ToggleLogContext(
                toggle_id.clone(),
            )))
        });
    let json = widget(
        pad(column(0.0), 12.0, 12.0, 12.0, 12.0)
            .surface(Role::Subtle)
            .radius(RadiusTier::Lg),
    )
    .visible(row_flag(item, |row| row.context_open))
    .children((style::bound(
        wrapping(style::mono_lh(
            String::new(),
            TypeRole::Meta,
            TextWeight::Regular,
            Role::Text,
            1.6,
        )),
        row_text(item, |row| &row.context),
    ),));
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
    node.type_role = Some(TypeRole::Meta);
    node.type_weight = Some(TextWeight::Bold);
    let layout = Arc::make_mut(&mut node.layout);
    layout.line_height = Some(nana_ui_core::LineHeightSpec::Relative(style::LINE));
    layout.width = Some(LengthSpec::Shrink);
    node
}
