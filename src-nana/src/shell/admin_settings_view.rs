//! 设置页。
//!
//! 结构照 `src/pages/Settings.vue`：页头、音频播放、外观、仓库服务、外部素材接入、
//! 插件管理、缓存、API 设计。页根是普通竖排，外边距和纵向滚动由壳层的主区提供。

use std::sync::Arc;

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, BoxPaint, IconButton, InteractionStyle, LayoutBox, LengthSpec, NodeStyle, PaintContext, PaintText, Painter,
    RangeChanged, RangeField, Select, SelectChanged, SelectOption, SemanticPaint, Stack, TextHorizontalAlignment,
    TextVerticalAlignment,
};
use nana_ui_core::{GridTrack, Icon, RadiusTier, SemanticColorRole as Role, ThemeMode};

use super::super::{ShellMessage, ShellViewModel};
use super::icons;
use super::style::{self, action, align_end, column, label, pad, row, Soft, SoftFill, Tone};
use super::support;
use super::AdminMessage;

/// 设置页整页。卡片之间留 12，插件管理面板之后不留。
pub(crate) fn settings_page(model: &ShellViewModel) -> AnyView {
    let children = vec![
        spaced(page_header(), 16.0),
        spaced(audio_card(model), 12.0),
        spaced(appearance_card(model), 12.0),
        spaced(repository_card(model), 12.0),
        spaced(external_card(model), 12.0),
        super::plugins_view::manager_panel(model, &super::plugins_view::SETTINGS_COPY),
        spaced(cache_card(model), 12.0),
        spaced(api_card(model), 12.0),
    ];
    widget(column(0.0)).children(children).key("admin-settings").into_any()
}

/// 给块加下外边距，对应 Vue `.card { margin-bottom: 12px }` 和页头的 16。
fn spaced(view: AnyView, bottom: f32) -> AnyView {
    widget(pad(column(0.0), 0.0, 0.0, bottom, 0.0)).children((view,)).into_any()
}

/// `.page-header`：18/600 标题，下面 13 号弱色说明，间距 4。
fn page_header() -> AnyView {
    widget(column(4.0))
        .children((
            widget(label("设置", 18.0, 600, Role::Text)).key("admin-settings-title"),
            widget(label("管理仓库服务、插件、缓存与 API 契约。", 13.0, 400, Role::Muted)).key("admin-settings-subline"),
        ))
        .key("admin-settings-header")
        .into_any()
}

/// `.card`：`bg-elev` 底、md 圆角、1px 透明边加 14/16 内边距。
pub(crate) fn card(key: &'static str, children: Vec<AnyView>) -> AnyView {
    widget(pad(column(0.0), 15.0, 17.0, 15.0, 17.0).surface(Role::Surface).radius(RadiusTier::Md))
        .children(children)
        .key(key)
        .into_any()
}

/// `.card h2`：13/600 弱色、大写、字距 0.5，下边距 8。
pub(crate) fn card_title(text: &str, key: &'static str) -> AnyView {
    let mut title = label(text.to_uppercase(), 13.0, 600, Role::Muted);
    Arc::make_mut(&mut title.style.layout).letter_spacing = Some(0.5);
    widget(pad(column(0.0), 0.0, 0.0, 8.0, 0.0)).children((widget(title).key(key),)).into_any()
}

/// `.settings-row` 的位置：首行上内边距 4，末行下内边距 4 且没有分割线。
#[derive(Clone, Copy, PartialEq, Eq)]
enum RowPlace {
    Middle,
    Last,
}

/// `.settings-row`：左边粗体标签加弱色说明，右边控件，上下 12，分割线 `border-soft`。
fn settings_row(title: &str, hint: &str, control: AnyView, place: RowPlace, key: &'static str) -> AnyView {
    let bottom = if place == RowPlace::Last { 4.0 } else { 12.0 };
    let body = pad(style::spread(16.0, AlignSpec::Center), 12.0, 0.0, bottom, 0.0);
    let body = if place == RowPlace::Last { body } else { style::bottom_rule(body) };
    let text = widget(Stack::column(2.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
        widget(label(title, 13.0, 500, Role::Text)).key(format!("{key}-label")),
        widget(label(hint, 12.0, 500, Role::Muted)).key(format!("{key}-hint")),
    ));
    widget(body).children((text, widget(row(0.0).shrink(0.0)).children((control,)))).key(key).into_any()
}

/// 设置卡里的提示行，对应 `.settings-notice`，错误时用危险色。
fn notice(text: String, error: bool, key: &'static str) -> AnyView {
    widget(pad(column(0.0), 10.0, 0.0, 0.0, 0.0))
        .children((widget(label(text, 12.0, 400, if error { Role::Danger } else { Role::Muted })).key(key),))
        .into_any()
}

/// 音频播放：喇叭图标加下拉框。实现音频能力的插件从插件清单里找；一个都没有时下拉框禁用，
/// 写出实际播放音频的内置解码器。
fn audio_card(model: &ShellViewModel) -> AnyView {
    let audio = support::audio_view(&model.player.candidates, &model.admin.plugins, &model.player.preferences);
    let options = audio.choices.iter().map(|choice| SelectOption::new(choice.plugin_id.clone(), choice.label.clone()));
    let mut select = Select::new(Some(audio.selected.clone())).options(options).disabled(!audio.selectable);
    {
        let layout = Arc::make_mut(&mut select.style.layout);
        layout.width = Some(LengthSpec::Shrink);
        // 没有可选项时 Vue 的空下拉框仍有内边距和箭头，约 42 宽。
        layout.min_width = Some(LengthSpec::Px(42.0));
        layout.height = Some(LengthSpec::Px(32.0));
        layout.font_size = Some(14.0);
        layout.line_height = Some(nana_ui_core::LineHeightSpec::Relative(style::LINE));
    }
    let picker = widget(row(0.0))
        .children((
            widget(style::glyph(icons::VOLUME2, 14.0, Role::Text)).key("admin-audio-icon"),
            widget(select).key("admin-audio-player").on_cx(|_, event: &SelectChanged, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetAudioPlayer(Some(event.value.to_string()))));
            }),
        ))
        .into_any();
    // 下面有提示时这一行不是卡片最后一个子元素，Vue 的 `:last-child` 规则不生效：保留分割线和 12 的下内边距。
    let place = if audio.notice.is_some() { RowPlace::Middle } else { RowPlace::Last };
    let mut body = vec![
        card_title("音频播放", "admin-audio-title"),
        settings_row("默认音频播放器", "按插件 ID 固定选择；不可用时只回退到官方播放器。", picker, place, "admin-audio-row"),
    ];
    if let Some((text, error)) = audio.notice {
        body.push(notice(text, error, "admin-audio-notice"));
    }
    card("admin-audio-card", body)
}

/// 外观：主题、圆角样式和圆角半径。
fn appearance_card(model: &ShellViewModel) -> AnyView {
    let theme = segmented(
        vec![theme_segment(ThemeMode::Dark, icons::MOON, "暗色"), theme_segment(ThemeMode::Light, icons::SUN, "浅色")],
        "admin-theme",
    );
    let corner = model.admin.corner_style.as_str();
    let corners = segmented(
        vec![
            corner_segment("smooth", icons::SQUARE_ROUND_CORNER, "平滑", corner == "smooth"),
            corner_segment("round", icons::RADIUS, "普通", corner == "round"),
        ],
        "admin-corner",
    );
    card(
        "admin-appearance-card",
        vec![
            card_title("外观", "admin-appearance-title"),
            settings_row("主题", "选择应用配色，立即生效并记忆到本地。", theme, RowPlace::Middle, "admin-theme-row"),
            settings_row("圆角", "选择平滑超椭圆或普通圆角，立即全局生效。", corners, RowPlace::Middle, "admin-corner-row"),
            settings_row("圆角半径", "调节普通与平滑圆角的全局半径。", radius_control(model), RowPlace::Last, "admin-radius-row"),
        ],
    )
}

/// `.segmented`：主背景、1px 边线、md 圆角、内边距 2、间距 2、高 34。
fn segmented(items: Vec<AnyView>, key: &'static str) -> AnyView {
    let body = style::fixed(row(2.0), None, Some(34.0));
    let stack = pad(body, 2.0, 2.0, 2.0, 2.0).surface(Role::Background).outline(Role::Border, 1.0).radius(RadiusTier::Md);
    widget(stack).children(items).key(key).into_any()
}

/// 分段按钮的固定外形：高 28、左右 12、sm 圆角、13/500、图标 14、间距 6。
fn segment_style(active: bool) -> NodeStyle {
    let (foreground, background) = if active { (Role::Text, Some(Role::Active)) } else { (Role::Muted, None) };
    let mut node = NodeStyle {
        foreground: Some(foreground),
        background,
        radius: Some(RadiusTier::Sm),
        interaction: InteractionStyle {
            hovered: SemanticPaint {
                foreground: Some(Role::Text),
                background: Some(if active { Role::SelectedHover } else { Role::Hover }),
                ..SemanticPaint::default()
            },
            pressed: SemanticPaint {
                foreground: Some(Role::Text),
                background: Some(if active { Role::SelectedPressed } else { Role::Active }),
                ..SemanticPaint::default()
            },
            ..InteractionStyle::default()
        },
        text_horizontal_alignment: TextHorizontalAlignment::Center,
        text_vertical_alignment: TextVerticalAlignment::Center,
        ..NodeStyle::default()
    };
    let layout = Arc::make_mut(&mut node.layout);
    layout.height = Some(LengthSpec::Px(28.0));
    layout.min_height = Some(LengthSpec::Px(28.0));
    layout.padding_left = Some(LengthSpec::Px(12.0));
    layout.padding_right = Some(LengthSpec::Px(12.0));
    layout.padding_top = Some(LengthSpec::Px(0.0));
    layout.padding_bottom = Some(LengthSpec::Px(0.0));
    layout.font_size = Some(13.0);
    layout.font_weight = Some(500);
    layout.line_height = Some(nana_ui_core::LineHeightSpec::Relative(style::LINE));
    layout.white_space_nowrap = true;
    layout.flex_shrink = Some(0.0);
    node
}

/// 圆角样式分段。当前样式来自已保存的偏好。
fn corner_segment(value: &'static str, icon: Icon, text: &'static str, active: bool) -> AnyView {
    let button = nana_ui::runtime::Button::new(text).icon(icon).icon_size(14.0).icon_gap(6.0).style(segment_style(active));
    widget(button)
        .key(format!("admin-corner-{value}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetCornerStyle(value.into())));
        })
        .into_any()
}

/// 主题分段。Vue 只有暗色和浅色两档，选中的是当前生效的主题，
/// 所以选中态在绘制时按主题解析，而不是读构建期的设置值。
fn theme_segment(mode: ThemeMode, icon: Icon, text: &'static str) -> AnyView {
    let value = if mode == ThemeMode::Dark { "dark" } else { "light" };
    let mut style = segment_style(false);
    style.foreground = None;
    style.interaction = InteractionStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Px(70.0));
        layout.min_width = Some(LengthSpec::Px(70.0));
    }
    let style = style.painter(ThemeSegment { mode, icon, text });
    let mut button = IconButton::new(icon, text);
    button.colors_from_style = true;
    button.style = style;
    widget(button)
        .key(format!("settings-theme-{value}"))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::SettingsThemeChanged(value.into()));
        })
        .into_any()
}

/// 主题分段的绘制：当前主题就是选中态，`bg-active` 底、正文色；否则弱色。
#[derive(Clone, Copy)]
struct ThemeSegment {
    mode: ThemeMode,
    icon: Icon,
    text: &'static str,
}

impl Painter for ThemeSegment {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let active = cx.theme_mode() == self.mode;
        let state = cx.state();
        let bounds = cx.bounds();
        let background = if active {
            Some(if state.hovered { Role::SelectedHover } else { Role::Active })
        } else if state.hovered {
            Some(Role::Hover)
        } else {
            None
        };
        if let Some(role) = background {
            let radius = cx.radius(RadiusTier::Sm);
            cx.rounded_rect(bounds, radius, BoxPaint::fill(role));
        }
        let color = if active || state.hovered { Role::Text } else { Role::Muted };
        let icon_box = LayoutBox { x: 12.0, y: (bounds.height - 14.0) / 2.0, width: 14.0, height: 14.0 };
        cx.icon(icon_box, self.icon, color);
        let text_box = LayoutBox { x: 32.0, y: 0.0, width: (bounds.width - 32.0).max(0.0), height: bounds.height };
        let mut text = PaintText::new(self.text).color(color).size(13.0).weight(500);
        text.vertical = TextVerticalAlignment::Center;
        text.horizontal = TextHorizontalAlignment::Start;
        cx.text(text_box, text);
    }

    fn paint_key(&self) -> u64 {
        let mode = if self.mode == ThemeMode::Dark { 1 } else { 2 };
        mode * 1_000_003 + self.text.len() as u64
    }
}

/// `.radius-control`：220 宽滑杆加 44 宽右对齐的「Npx」。
/// Vue 的滑杆轨道因 `appearance: none` 看不见，这里保留轨道，按设计意图画出。
fn radius_control(model: &ShellViewModel) -> AnyView {
    let radius = model.admin.corner_radius;
    let mut range = RangeField::new(radius, support::CORNER_RADIUS_MIN, support::CORNER_RADIUS_MAX, 1.0)
        .label("圆角半径")
        .show_label(false)
        .show_value(false);
    {
        let layout = Arc::make_mut(&mut range.style.layout);
        layout.width = Some(LengthSpec::Px(220.0));
        layout.min_width = Some(LengthSpec::Px(140.0));
        layout.height = Some(LengthSpec::Px(16.0));
    }
    let output = widget(style::fixed(row(0.0).justify(nana_ui::runtime::JustifySpec::End), Some(44.0), None))
        .children((widget(align_end(label(format!("{}px", format_radius(radius)), 12.0, 400, Role::Muted))).key("admin-radius-value"),));
    widget(row(10.0))
        .children((
            widget(range).key("admin-corner-radius").on_cx(|_, event: &RangeChanged, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SetCornerRadius(event.value.to_string())));
            }),
            output,
        ))
        .into_any()
}

/// 半径按整数显示，带小数时保留原值。
fn format_radius(radius: f64) -> String {
    if radius.fract() == 0.0 { format!("{}", radius as i64) } else { format!("{radius}") }
}

/// `.kv li`：左边弱色标签，右边正文色值，上下 6，分割线 `border-soft`，末行无线。
pub(crate) fn kv_row(name: &str, value: impl Into<String>, last: bool, key: impl Into<String>) -> AnyView {
    let key = key.into();
    let body = pad(style::spread(12.0, AlignSpec::Start), 6.0, 0.0, 6.0, 0.0);
    let body = if last { body } else { style::bottom_rule(body) };
    widget(body)
        .children((
            widget(label(name, 14.0, 400, Role::Muted)).key(format!("{key}-label")),
            widget(Stack::row(0.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).shrink(1.0).justify(nana_ui::runtime::JustifySpec::End))
                .children((widget(align_end(label(value, 14.0, 400, Role::Text))).key(format!("{key}-value")),)),
        ))
        .key(key)
        .into_any()
}

/// 仓库服务：已注册仓库数、并发模型、同步方式和已接入后端。
fn repository_card(model: &ShellViewModel) -> AnyView {
    card(
        "admin-repository-card",
        vec![
            card_title("仓库服务", "admin-repository-title"),
            kv_row("已注册仓库", model.workspace.repositories.len().to_string(), false, "admin-repo-count"),
            kv_row("并发模型", "SQLite WAL + 乐观锁", false, "admin-repo-concurrency"),
            kv_row("同步方式", "全量扫描 + 事件表", false, "admin-repo-sync"),
            kv_row("已接入后端", support::backend_summary(&model.admin.backends), true, "admin-backends"),
        ],
    )
}

/// 外部素材接入：标题右侧状态徽章、四行连接信息、四个按钮和提示。
fn external_card(model: &ShellViewModel) -> AnyView {
    let status = model.admin.external.as_ref();
    let ready = status.is_some_and(|item| item.ready);
    let loaded = status.is_some();
    let text = |value: Option<&str>| value.map(str::to_string).unwrap_or_else(|| "未加载".into());
    let json = status.map(|item| support::connection_json(&item.base_url, &item.token, &item.version, &item.started_at)).unwrap_or_default();
    let base_url = status.map(|item| item.base_url.clone()).unwrap_or_default();
    let token = status.map(|item| item.token.clone()).unwrap_or_default();
    let head = widget(pad(style::spread(12.0, AlignSpec::Center), 0.0, 0.0, 8.0, 0.0))
        .children((
            widget(label("外部素材接入", 13.0, 600, Role::Muted)).key("admin-external-title"),
            status_badge(support::external_status_label(status.map(|item| item.ready)), ready),
        ))
        .into_any();
    let actions = widget(pad(row(8.0).wrap(true), 12.0, 0.0, 0.0, 0.0))
        .children((
            copy_action("复制 Base URL", "Base URL", base_url, icons::COPY, !loaded, "admin-copy-url"),
            copy_action("复制 Token", "Token", token, icons::COPY, !loaded, "admin-copy-token"),
            copy_action("复制 JSON", "连接 JSON", json.clone(), icons::FILE_JSON, json.is_empty(), "admin-copy-json"),
            widget(action("导出 JSON", Some(icons::DOWNLOAD), Tone::Primary, json.is_empty()))
                .key("admin-export-json")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ExportExternal)))
                .into_any(),
        ))
        .into_any();
    let mut body = vec![
        head,
        kv_row("连接文件", text(status.map(|item| item.connection_file_path.as_str())), false, "admin-external-file"),
        kv_row("Base URL", text(status.map(|item| item.base_url.as_str())), false, "admin-external-url"),
        kv_row("Token", support::mask_token(status.map(|item| item.token.as_str())), false, "admin-external-token"),
        kv_row("启动时间", text(status.map(|item| item.started_at.as_str())), true, "admin-external-started"),
        actions,
    ];
    if !model.admin.external_error.is_empty() {
        body.push(notice(model.admin.external_error.clone(), true, "admin-external-error"));
    } else if !model.admin.external_message.is_empty() {
        body.push(notice(model.admin.external_message.clone(), false, "admin-external-message"));
    }
    card("admin-external-card", body)
}

/// `.settings-status`：高 24、左右 8、药丸、12/600，图标 13；就绪时成功色柔和底。
fn status_badge(text: &str, ready: bool) -> AnyView {
    let body = style::rounded(pad(style::fixed(row(5.0), None, Some(24.0)), 0.0, 8.0, 0.0, 8.0), None);
    let (body, color) = if ready {
        (body.painter(SoftFill::new(Soft::Success, None)), Role::Success)
    } else {
        (body.surface(Role::Background), Role::Muted)
    };
    widget(body)
        .children((
            widget(style::glyph(icons::CHECK_CIRCLE2, 13.0, color)),
            widget(label(text, 12.0, 600, color)).key("admin-external-status"),
        ))
        .into_any()
}

fn copy_action(text: &'static str, field: &'static str, value: String, icon: Icon, disabled: bool, key: &'static str) -> AnyView {
    widget(action(text, Some(icon), Tone::Plain, disabled))
        .key(key)
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::CopyExternal { label: field.into(), value: value.clone() }));
        })
        .into_any()
}

/// 缓存：三格容量和最近条目。没有快照时容量写 0，和 Vue 一样不另造数字。
fn cache_card(model: &ShellViewModel) -> AnyView {
    let config = model.admin.cache.as_ref().map(|cache| &cache.config);
    let metrics = [
        (config.map(|item| item.metadata_capacity).unwrap_or(0), "Metadata LRU", "admin-cache"),
        (config.map(|item| item.thumbnail_capacity).unwrap_or(0), "Thumbnail LRU", "admin-cache-thumbnail"),
        (config.map(|item| item.query_capacity).unwrap_or(0), "Query LRU", "admin-cache-query"),
    ];
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid_columns = Some(vec![GridTrack::Fr(1.0), GridTrack::Fr(1.0), GridTrack::Fr(1.0)]);
        layout.gap = Some(LengthSpec::Px(10.0));
        layout.width = Some(LengthSpec::Fill);
        layout.padding_bottom = Some(LengthSpec::Px(12.0));
    });
    let cells = metrics.into_iter().map(|(value, name, key)| metric(value, name, key)).collect::<Vec<_>>();
    let mut body = vec![card_title("缓存", "admin-cache-title"), widget(grid).children(cells).into_any()];
    let entries = model.admin.cache.as_ref().map(|cache| cache.entries.as_slice()).unwrap_or(&[]);
    for (index, entry) in entries.iter().enumerate() {
        body.push(kv_row(
            &format!("{} / {}", entry.cache_type, entry.key),
            entry.last_accessed_at.clone(),
            index + 1 == entries.len(),
            format!("admin-cache-entry-{index}"),
        ));
    }
    card("admin-cache-card", body)
}

/// `.settings-metric`：主背景、lg 圆角、内边距 12，数据库图标 16，数字 14/700 加 12 号弱色名称。
fn metric(value: usize, name: &str, key: &'static str) -> AnyView {
    widget(pad(row(10.0), 12.0, 12.0, 12.0, 12.0).surface(Role::Background).radius(RadiusTier::Lg).with_layout(|layout| {
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }))
    .children((
        widget(style::glyph(icons::DATABASE, 16.0, Role::Text)),
        widget(Stack::column(0.0).width(LengthSpec::Shrink)).children((
            widget(label(value.to_string(), 14.0, 700, Role::Text)).key(key),
            widget(label(name, 12.0, 400, Role::Muted)).key(format!("{key}-name")),
        )),
    ))
    .into_any()
}

/// API 设计：传输方式和端点列表。没有快照时写「本地服务契约未加载」。
fn api_card(model: &ShellViewModel) -> AnyView {
    let transport = model.admin.api_design.as_ref().map(|api| api.transport.clone()).unwrap_or_else(|| "本地服务契约未加载".into());
    let mut body = vec![
        card_title("API 设计", "admin-api-title"),
        widget(row(4.0))
            .children((
                widget(style::glyph(icons::SERVER_COG, 13.0, Role::Muted)),
                widget(label(transport, 14.0, 400, Role::Muted)).key("admin-api-transport"),
            ))
            .into_any(),
    ];
    let endpoints = model.admin.api_design.as_ref().map(|api| api.endpoints.as_slice()).unwrap_or(&[]);
    for (index, endpoint) in endpoints.iter().enumerate() {
        body.push(kv_row(
            &format!("{} / {} {}", endpoint.group, endpoint.method, endpoint.path),
            endpoint.summary.clone(),
            index + 1 == endpoints.len(),
            format!("admin-api-endpoint-{index}"),
        ));
    }
    card("admin-api-card", body)
}
