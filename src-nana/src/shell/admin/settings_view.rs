//! 设置页。
//!
//! 结构：页头、音频播放、外观、仓库服务、外部素材接入、
//! 插件管理、缓存、API 设计。页根是普通竖排，外边距和纵向滚动由壳层的主区提供。
//!
//! 每张卡是 NanaUI 的设置分组（`settings_group`）：可调的项是 `settings_row`（主题和圆角样式用
//! `SegmentedControl`，音频的提示写在行的提示行里），只读的键值是 `LabeledValue`，服务状态是
//! `StatusBadge`，外部接入的错误和结果是分组下面的 `callout`。缓存的三格容量 NanaUI 没有对应组件，
//! 照原样拼。
//!
//! 页面只建一次，只读 [`SettingsSignals`] 和插件管理面板的信号：卡片里的文字、下拉框、分段、滑杆和
//! 按钮状态按字段绑定；缓存条目和 API 端点按内容做键；复制按钮点下去时现读连接信息。

use std::sync::Arc;

use nana_ui::icons_tabler::{ALERT_CIRCLE, INFO_CIRCLE};
use nana_ui::runtime::view::{
    AnyView, El, GroupRow, IntoProp, IntoView, SettingsGroupView, Signal, callout, each, fields,
    group_row_on_line, labeled_value, segmented, settings_group, settings_row, widget,
};
use nana_ui::runtime::{
    Activate, LengthSpec, RangeField, RangeInput, SegmentedOption, SegmentedOptionChosen, Select,
    SelectChanged, SelectOption, Stack, StatusTone, ValidationIntent,
};
use nana_ui_core::{GridTrack, Icon, RadiusTier, SemanticColorRole as Role};

use super::super::ShellMessage;
use super::AdminMessage;
use super::bind::ActionDisabled;
use super::icons;
use super::plugins_state::PluginPanelSignals;
use super::settings_state::{ExternalCard, KvRow, SettingsSignals};
use super::style::{self, Tone, action, align_end, column, label, pad, row};
use super::support;

/// 设置页整页。卡片之间留 12，插件管理面板之后不留。
pub(crate) fn settings_page(signals: SettingsSignals, plugins: PluginPanelSignals) -> AnyView {
    let children = vec![
        spaced(page_header(), 16.0),
        spaced(audio_card(signals), 12.0),
        spaced(appearance_card(signals), 12.0),
        spaced(repository_card(signals), 12.0),
        spaced(external_card(signals.external), 12.0),
        super::plugins_view::manager_panel(plugins, &super::plugins_view::SETTINGS_COPY),
        spaced(cache_card(signals), 12.0),
        spaced(api_card(signals), 12.0),
    ];
    widget(column(0.0))
        .children(children)
        .key("admin-settings")
        .into_any()
}

/// 给块加下外边距：卡片之间 12，页头 16。
fn spaced(view: AnyView, bottom: f32) -> AnyView {
    widget(pad(column(0.0), 0.0, 0.0, bottom, 0.0))
        .children((view,))
        .into_any()
}

/// 页头：18/600 标题，下面 13 号弱色说明，间距 4。
fn page_header() -> AnyView {
    widget(column(4.0))
        .children((
            widget(label("设置", 18.0, 600, Role::Text)).key("admin-settings-title"),
            widget(label(
                "管理仓库服务、插件、缓存与 API 契约。",
                13.0,
                400,
                Role::Muted,
            ))
            .key("admin-settings-subline"),
        ))
        .key("admin-settings-header")
        .into_any()
}

/// 一张设置卡：NanaUI 的设置分组（标题在上，凸起卡片在下，行间是缩进的细线），外面一层带键的列。
fn card(key: &'static str, title: &str, rows: Vec<GroupRow>) -> AnyView {
    card_with(key, settings_group(Some(title.to_string()), rows))
}

/// 给设置分组套一层带键的列，键给测试和滚动定位用。
fn card_with(key: &'static str, group: SettingsGroupView) -> AnyView {
    widget(column(0.0)).children((group,)).key(key).into_any()
}

/// 音频播放：喇叭图标加下拉框。实现音频能力的插件从插件清单里找；一个都没有时下拉框禁用，
/// 写出实际播放音频的内置解码器。回退或缺失的提示写在这一行下面的提示行里，错误时用错误档。
/// 下拉框没有自己的名字，设置行把行标题给它当读屏名称。
fn audio_card(signals: SettingsSignals) -> AnyView {
    let audio = signals.audio;
    let initial = audio.get_untracked();
    let options = |options: &[(String, String)]| {
        options
            .iter()
            .map(|(value, text)| SelectOption::new(value.clone(), text.clone()))
            .collect::<Vec<_>>()
    };
    // 按最长的选项定宽，不占满整行，行标题和说明留在左边。
    let mut select = Select::new(Some(initial.selected.clone()))
        .options(options(&initial.options))
        .disabled(!initial.selectable)
        .fit_options(true);
    Arc::make_mut(&mut select.style.layout).width = Some(LengthSpec::Shrink);
    let picker = widget(row(8.0))
        .children((
            widget(style::glyph(icons::VOLUME2, 14.0, Role::Text)).key("admin-audio-icon"),
            widget(select)
                .prop::<Vec<SelectOption>, fields::select::options>(move || {
                    audio.with(|audio| options(&audio.options))
                })
                .prop::<Option<Arc<str>>, fields::select::value>(move || {
                    audio.with(|audio| Some(Arc::from(audio.selected.as_str())))
                })
                .prop::<bool, fields::select::disabled>(move || {
                    audio.with(|audio| !audio.selectable)
                })
                .key("admin-audio-player")
                .on_cx(|_, event: &SelectChanged, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetAudioPlayer(
                        Some(event.value.to_string()),
                    )));
                }),
        ))
        .into_any();
    let audio_row = settings_row("默认音频播放器")
        .hint(Some(Arc::from(
            "按插件 ID 固定选择；不可用时只回退到官方播放器。",
        )))
        .notice(move || {
            audio.with(|audio| {
                audio
                    .notice
                    .as_ref()
                    .map(|(text, _)| Arc::from(text.as_str()))
            })
        })
        .notice_tone(move || {
            if audio.with(|audio| audio.notice.as_ref().is_some_and(|(_, error)| *error)) {
                ValidationIntent::Danger
            } else {
                ValidationIntent::Warning
            }
        })
        .control(picker)
        .key("admin-audio-row");
    card(
        "admin-audio-card",
        "音频播放",
        vec![group_row_on_line(audio_row)],
    )
}

/// 外观：主题、圆角样式和圆角半径。主题有暗色、浅色和「跟随系统」（默认）三档。
fn appearance_card(signals: SettingsSignals) -> AnyView {
    let theme = signals.theme;
    let theme_picker = segmented()
        .children((
            choice(
                "dark",
                icons::MOON,
                "暗色",
                theme,
                "settings-theme",
                |value| ShellMessage::SettingsThemeChanged(value.into()),
            ),
            choice(
                "light",
                icons::SUN,
                "浅色",
                theme,
                "settings-theme",
                |value| ShellMessage::SettingsThemeChanged(value.into()),
            ),
            choice(
                "system",
                icons::MONITOR,
                "跟随系统",
                theme,
                "settings-theme",
                |value| ShellMessage::SettingsThemeChanged(value.into()),
            ),
        ))
        .key("admin-theme");
    let corner = signals.corner_style;
    let corner_message =
        |value: &str| ShellMessage::Admin(AdminMessage::SetCornerStyle(value.into()));
    let corner_picker = segmented()
        .children((
            choice(
                "smooth",
                icons::SQUARE_ROUND_CORNER,
                "平滑",
                corner,
                "admin-corner",
                corner_message,
            ),
            choice(
                "round",
                icons::RADIUS,
                "普通",
                corner,
                "admin-corner",
                corner_message,
            ),
        ))
        .key("admin-corner");
    card(
        "admin-appearance-card",
        "外观",
        vec![
            group_row_on_line(
                settings_row("主题")
                    .hint(Some(Arc::from("选择应用配色，立即生效并记忆到本地。")))
                    .control(theme_picker)
                    .key("admin-theme-row"),
            ),
            group_row_on_line(
                settings_row("圆角")
                    .hint(Some(Arc::from("选择平滑超椭圆或普通圆角，立即全局生效。")))
                    .control(corner_picker)
                    .key("admin-corner-row"),
            ),
            group_row_on_line(
                settings_row("圆角半径")
                    .hint(Some(Arc::from("调节普通与平滑圆角的全局半径。")))
                    .control(radius_control(signals.corner_radius))
                    .key("admin-radius-row"),
            ),
        ],
    )
}

/// 分段选择的一项：选中的是设置里的那一档（主题跟随系统时选中「跟随系统」，不按系统现在是深是浅
/// 去点亮暗色或浅色）。选中时发 `message(value)`，立即生效并写进设置文件；选中态等投影回来再跟着换。
fn choice(
    value: &'static str,
    icon: Icon,
    text: &'static str,
    current: Signal<String>,
    key: &'static str,
    message: fn(&str) -> ShellMessage,
) -> El<SegmentedOption> {
    widget(SegmentedOption::new(text).icon(icon))
        .selected(move || current.with(|current| current == value))
        .key(format!("{key}-{value}"))
        .on_cx(move |_, _: &SegmentedOptionChosen, cx| {
            cx.dispatch_program_all(message(value));
        })
}

/// 圆角控件：220 宽滑杆加 44 宽右对齐的「Npx」。滑杆的值和文字跟着已保存的半径。
/// 滑杆保留轨道，按设计意图画出。
fn radius_control(radius: Signal<f64>) -> AnyView {
    let mut range = RangeField::new(
        radius.get_untracked(),
        support::CORNER_RADIUS_MIN,
        support::CORNER_RADIUS_MAX,
        1.0,
    )
    .label("圆角半径")
    .show_label(false)
    .show_value(false);
    {
        let layout = Arc::make_mut(&mut range.style.layout);
        layout.width = Some(LengthSpec::Px(220.0));
        layout.min_width = Some(LengthSpec::Px(140.0));
        layout.height = Some(LengthSpec::Px(16.0));
    }
    let output = widget(style::fixed(
        row(0.0).justify(nana_ui::runtime::JustifySpec::End),
        Some(44.0),
        None,
    ))
    .children((style::bound(
        align_end(label(String::new(), 12.0, 400, Role::Muted)),
        move || radius.with(|radius| format!("{}px", format_radius(*radius))),
    )
    .key("admin-radius-value"),));
    widget(row(10.0))
        .children((
            // 拖动中每一格都写入，所以只听 `RangeInput`。
            // 松手的 `RangeChanged` 是同一格的提交，再听一次会把这次拖动写两遍。范围仍是 0 到 20。
            widget(range)
                .prop::<f64, fields::slider::value>(radius)
                .key("admin-corner-radius")
                .on_cx(|_, event: &RangeInput, cx| {
                    cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetCornerRadius(
                        event.value.to_string(),
                    )));
                }),
            output,
        ))
        .into_any()
}

/// 半径按整数显示，带小数时保留原值。
fn format_radius(radius: f64) -> String {
    if radius.fract() == 0.0 {
        format!("{}", radius as i64)
    } else {
        format!("{radius}")
    }
}

/// 键值行：NanaUI 的标签和值，标签靠左、值靠右。值可以绑定。
fn kv_row(name: &str, value: impl IntoProp<String>, key: impl Into<String>) -> AnyView {
    let value = value.into_source();
    labeled_value(name.to_string())
        .value(move || Arc::<str>::from(value.get()))
        .compact(false)
        .key(key.into())
        .into_any()
}

/// 分组里的一行键值，放在卡片的内容线上。
fn kv_line(name: &str, value: impl IntoProp<String>, key: &'static str) -> GroupRow {
    group_row_on_line(kv_row(name, value, key))
}

/// 按内容做键的一列键值行：条目没有编号，内容变了那一行重建。整列是分组里的一行，没有条目时连同
/// 分隔线一起藏起来。
fn kv_rows(rows: impl Fn() -> Vec<KvRow> + Send + 'static) -> GroupRow {
    let rows = nana_ui::runtime::view::computed(rows);
    group_row_on_line(each(rows, KvRow::clone, |row: KvRow| {
        kv_row(&row.name, row.value.clone(), row.key.clone())
    }))
    .visible(move || rows.with(|rows| !rows.is_empty()))
}

/// 仓库服务：已注册仓库数、并发模型、同步方式和已接入后端。
fn repository_card(signals: SettingsSignals) -> AnyView {
    let repository = signals.repository;
    card(
        "admin-repository-card",
        "仓库服务",
        vec![
            kv_line(
                "已注册仓库",
                move || repository.with(|card| card.count.clone()),
                "admin-repo-count",
            ),
            kv_line("并发模型", "SQLite WAL + 乐观锁", "admin-repo-concurrency"),
            kv_line("同步方式", "全量扫描 + 事件表", "admin-repo-sync"),
            kv_line(
                "已接入后端",
                move || repository.with(|card| card.backends.clone()),
                "admin-backends",
            ),
        ],
    )
}

/// 外部连接卡片里的一个值。
fn external_text(
    external: Signal<ExternalCard>,
    pick: fn(&ExternalCard) -> &String,
) -> impl Fn() -> String + Send + 'static {
    move || external.with(|card| pick(card).clone())
}

/// 外部素材接入：服务状态徽章和四行连接信息；卡片下面是四个按钮和提示条。
fn external_card(external: Signal<ExternalCard>) -> AnyView {
    let not_loaded = move || external.with(|card| !card.loaded);
    let no_json = move || external.with(|card| card.json.is_empty());
    let actions = widget(row(8.0).wrap(true))
        .children((
            copy_action(
                "复制 Base URL",
                "Base URL",
                |card| &card.base_url,
                icons::COPY,
                not_loaded,
                "admin-copy-url",
                external,
            ),
            copy_action(
                "复制 Token",
                "Token",
                |card| &card.raw_token,
                icons::COPY,
                not_loaded,
                "admin-copy-token",
                external,
            ),
            copy_action(
                "复制 JSON",
                "连接 JSON",
                |card| &card.json,
                icons::FILE_JSON,
                no_json,
                "admin-copy-json",
                external,
            ),
            widget(action(
                "导出 JSON",
                Some(icons::DOWNLOAD),
                Tone::Primary,
                false,
            ))
            .prop::<bool, ActionDisabled>(no_json)
            .key("admin-export-json")
            .on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ExportExternal))
            })
            .into_any(),
        ))
        .into_any();
    // 出错时只显示错误，不同时显示上一次的操作结果。提示条文字为空时自己藏起来。
    let message = move || {
        external.with(|card| {
            if card.error.is_empty() {
                card.message.clone()
            } else {
                String::new()
            }
        })
    };
    let footer = widget(column(10.0)).children((
        actions,
        widget(column(0.0))
            .key("admin-external-error")
            .children((callout(
                ALERT_CIRCLE,
                StatusTone::Danger,
                external_text(external, |card| &card.error),
            ),)),
        widget(column(0.0))
            .key("admin-external-message")
            .children((callout(INFO_CIRCLE, StatusTone::Info, message),)),
    ));
    let status = settings_row("服务状态")
        .control(status_badge(external))
        .key("admin-external-status-row");
    let group = settings_group(
        Some("外部素材接入".to_string()),
        vec![
            group_row_on_line(status),
            kv_line(
                "连接文件",
                external_text(external, |card| &card.file),
                "admin-external-file",
            ),
            kv_line(
                "Base URL",
                external_text(external, |card| &card.url),
                "admin-external-url",
            ),
            kv_line(
                "Token",
                external_text(external, |card| &card.token),
                "admin-external-token",
            ),
            kv_line(
                "启动时间",
                external_text(external, |card| &card.started),
                "admin-external-started",
            ),
        ],
    )
    .footer(footer);
    card_with("admin-external-card", group)
}

/// 服务状态徽章：NanaUI 的状态徽章，就绪时成功色，否则中性色。
fn status_badge(external: Signal<ExternalCard>) -> AnyView {
    nana_ui::runtime::view::status_badge(move || {
        external.with(|card| Arc::<str>::from(card.status.as_str()))
    })
    .tone(move || {
        if external.with(|card| card.ready) {
            StatusTone::Success
        } else {
            StatusTone::Neutral
        }
    })
    .key("admin-external-status")
    .into_any()
}

/// 复制按钮：点下去时现读这一刻的连接信息。
fn copy_action(
    text: &'static str,
    field: &'static str,
    pick: fn(&ExternalCard) -> &String,
    icon: Icon,
    disabled: impl Fn() -> bool + Send + 'static,
    key: &'static str,
    external: Signal<ExternalCard>,
) -> AnyView {
    widget(action(text, Some(icon), Tone::Plain, false))
        .prop::<bool, ActionDisabled>(disabled)
        .key(key)
        .on_cx(move |_, _: &Activate, cx| {
            let value = external.with_untracked(|card| pick(card).clone());
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::CopyExternal {
                label: field.into(),
                value,
            }));
        })
        .into_any()
}

/// 缓存：当前资源库的缩略图、其余缓存文件和总占用三格，下面逐项列出各缓存目录的文件数和大小。
fn cache_card(signals: SettingsSignals) -> AnyView {
    let cache = signals.cache;
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid.columns = Some(vec![
            GridTrack::Fr(1.0),
            GridTrack::Fr(1.0),
            GridTrack::Fr(1.0),
        ]);
        layout.gap = Some(LengthSpec::Px(10.0));
        layout.width = Some(LengthSpec::Fill);
        layout.padding_top = Some(LengthSpec::Px(12.0));
        layout.padding_bottom = Some(LengthSpec::Px(12.0));
    });
    let cells = [
        ("缩略图", "admin-cache-thumbnail"),
        ("缓存文件", "admin-cache-files"),
        ("总占用", "admin-cache-bytes"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (name, key))| {
        metric(
            move || cache.with(|card| card.metrics[index].clone()),
            name,
            key,
        )
    })
    .collect::<Vec<_>>();
    card(
        "admin-cache-card",
        "缓存",
        vec![
            group_row_on_line(widget(grid).children(cells)),
            kv_rows(move || cache.with(|card| card.entries.clone())),
        ],
    )
}

/// 缓存容量格：主背景、lg 圆角、内边距 12，数据库图标 16，数字 14/700 加 12 号弱色名称。
/// NanaUI 没有统计格组件，照原样拼。
fn metric(value: impl Fn() -> String + Send + 'static, name: &str, key: &'static str) -> AnyView {
    widget(
        pad(row(10.0), 12.0, 12.0, 12.0, 12.0)
            .surface(Role::Background)
            .radius(RadiusTier::Lg)
            .with_layout(|layout| {
                layout.width = Some(LengthSpec::Fill);
                layout.min_width = Some(LengthSpec::Px(0.0));
            }),
    )
    .children((
        widget(style::glyph(icons::DATABASE, 16.0, Role::Text)),
        widget(Stack::column(0.0).width(LengthSpec::Shrink)).children((
            style::bound(label(String::new(), 14.0, 700, Role::Text), value).key(key),
            widget(label(name, 12.0, 400, Role::Muted)).key(format!("{key}-name")),
        )),
    ))
    .into_any()
}

/// API 设计：传输方式（一行安静的提示）和端点列表。没有快照时写「本地服务契约未加载」。
fn api_card(signals: SettingsSignals) -> AnyView {
    let api = signals.api;
    let transport = widget(column(0.0))
        .key("admin-api-transport")
        .children((callout(icons::SERVER_COG, StatusTone::Neutral, move || {
            api.with(|card| card.transport.clone())
        })
        .quiet(),));
    card(
        "admin-api-card",
        "API 设计",
        vec![
            group_row_on_line(transport),
            kv_rows(move || api.with(|card| card.endpoints.clone())),
        ],
    )
}
