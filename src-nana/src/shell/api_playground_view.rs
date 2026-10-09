//! API Playground 的界面。
//!
//! 版式照 `.api-playground`：页头（眉题、22 号标题、等宽副标题，右侧刷新契约和发送）、
//! 提示、四个元信息块，下面左右两栏：左栏请求（传输与搜索、端点与方法、说明、目标、鉴权、
//! 请求 JSON、复制和发送），右栏响应（状态、耗时、响应头和响应体）。

use std::sync::Arc;

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Checkbox, JustifySpec, LengthSpec, Select, SelectChanged, SelectOption, Stack, TextArea, TextChanged,
    TextInput, ToggleChanged,
};
use nana_ui_core::{GridTrack, RadiusTier, SemanticColorMix, SemanticColorRole as Role};

use super::super::super::{ShellMessage, ShellViewModel};
use super::super::style::{self, action, column, label, mono, pad, row, wrapping, Tone};
use super::super::AdminMessage;
use super::{can_send, endpoints, request_summary, selected_ref, target_text, token_preview, visible, ApiMessage};

const TRANSPORTS: [(&str, &str); 4] = [("all", "全部"), ("external-http", "HTTP"), ("tauri-command", "Core"), ("plugin-call", "Plugin")];

fn api(message: ApiMessage) -> ShellMessage {
    ShellMessage::Admin(AdminMessage::Api(message))
}

/// 整个工具页，内边距 18，竖排间距 14，占满工具页高度。
pub(crate) fn playground(model: &ShellViewModel) -> AnyView {
    let state = &model.admin.api;
    let loading = model.admin.loading_settings;
    let mut body = vec![header(model, loading)];
    if !state.error.is_empty() {
        body.push(style::state_notice(state.error.clone(), true, "admin-api-error"));
    } else if model.admin.api_design.is_none() && !model.admin.load_error.is_empty() && !loading {
        body.push(style::state_notice(format!("API 契约加载失败：{}", model.admin.load_error), true, "admin-api-load-error"));
    }
    if !state.notice.is_empty() {
        body.push(style::state_notice(state.notice.clone(), false, "admin-api-notice"));
    } else if model.admin.external.is_none() && !model.admin.load_error.is_empty() && !loading {
        body.push(style::state_notice(format!("外部 API 连接未加载：{}", model.admin.load_error), false, "admin-api-connection"));
    }
    body.push(meta(model));
    body.push(columns(model));
    widget(pad(Stack::fill_column(14.0), 18.0, 18.0, 18.0, 18.0)).children(body).key("admin-api-playground").into_any()
}

fn header(model: &ShellViewModel, loading: bool) -> AnyView {
    let sending = model.admin.api.sending;
    let left = widget(Stack::column(0.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
        widget(style::eyebrow_text("API Playground")).key("admin-api-eyebrow"),
        widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0)).children((widget(style::label_lh("后端 API 调试", 22.0, 700, Role::Text, 1.25)).key("admin-api-title"),)),
        widget(pad(column(0.0), 8.0, 0.0, 0.0, 0.0)).children((widget(wrapping(mono(request_summary(model), 12.0, 400, Role::Muted))).key("admin-api-summary"),)),
    ));
    let actions = widget(row(12.0).wrap(true).align(AlignSpec::Start)).children((
        widget(action("刷新契约", None, Tone::Plain, loading)).key("admin-api-refresh").on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(api(ApiMessage::Refresh))),
        widget(action(if sending { "发送中" } else { "发送" }, None, Tone::Primary, !can_send(model)))
            .key("admin-api-send")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(api(ApiMessage::Send))),
    ));
    widget(style::spread(12.0, AlignSpec::Start)).children((left, actions)).into_any()
}

/// 四个元信息块：API 数、Base URL、Token 和契约传输方式。
fn meta(model: &ShellViewModel) -> AnyView {
    let base = model.admin.external.as_ref().map(|item| item.base_url.clone()).filter(|text| !text.is_empty()).unwrap_or_else(|| "未加载".into());
    let contract = model.admin.api_design.as_ref().map(|api| api.transport.clone()).unwrap_or_else(|| "未加载".into());
    let blocks = [
        ("API", endpoints(model).len().to_string()),
        ("Base URL", base),
        ("Token", token_preview(model)),
        ("契约", contract),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (name, value))| {
        widget(pad(row(4.0), 4.0, 9.0, 4.0, 9.0).surface(Role::Background).radius(RadiusTier::Sm).with_layout(|layout| {
            layout.min_height = Some(LengthSpec::Px(26.0));
        }))
        .children((widget(label(name, 12.0, 400, Role::Muted)), widget(mono(value, 12.0, 600, Role::Text)).key(format!("admin-api-meta-{index}"))))
        .into_any()
    })
    .collect::<Vec<_>>();
    widget(row(8.0).wrap(true).width(LengthSpec::Fill)).children(blocks).key("admin-api-meta").into_any()
}

/// 左右两栏：请求栏最小 260、最宽 420，响应栏占余下宽度。
fn columns(model: &ShellViewModel) -> AnyView {
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid_columns = Some(vec![
            style::capped(260.0, 420.0),
            GridTrack::MinMax { min_px: 0.0, fr: 1.0, max_px: None },
        ]);
        layout.gap = Some(LengthSpec::Px(12.0));
        layout.width = Some(LengthSpec::Fill);
        layout.flex_grow = Some(1.0);
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.align_items = AlignSpec::Stretch;
    });
    widget(grid).children((request_column(model), response_column(model))).into_any()
}

/// 两栏共用的框：主背景、`border-soft` 边线、md 圆角、1px 边加 14 内边距。
fn frame(gap: f32) -> Stack {
    pad(column(gap), 14.0, 14.0, 14.0, 14.0)
        .surface(Role::Background)
        .outline(Role::BorderSoft, 1.0)
        .radius(RadiusTier::Md)
        .with_layout(|layout| {
            layout.min_height = Some(LengthSpec::Px(0.0));
            layout.height = Some(LengthSpec::Fill);
        })
}

/// 字段：12/700 弱色标题在上，控件在下，间距 6。键随标题，输入框的键路径不随兄弟节点变化。
fn field(title: &str, control: AnyView) -> AnyView {
    widget(column(6.0).min_width(LengthSpec::Px(0.0)))
        .children((widget(label(title, 12.0, 700, Role::Muted)), control))
        .key(format!("admin-api-field-{title}"))
        .into_any()
}

/// 原生下拉框的外观：`bg-subtle` 底、`border-strong` 边线，高 32，14 号字。
fn native_select(select: Select) -> Select {
    let mut select = select;
    select.style.interaction.base.background_mix = Some(SemanticColorMix::alpha(Role::Subtle, 1.0));
    select.style.interaction.base.border_mix = Some(SemanticColorMix::alpha(Role::BorderStrong, 1.0));
    let layout = Arc::make_mut(&mut select.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.height = Some(LengthSpec::Px(32.0));
    layout.font_size = Some(14.0);
    select
}

fn two_columns(first: GridTrack, second: GridTrack, children: (AnyView, AnyView), key: &'static str) -> AnyView {
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid_columns = Some(vec![first, second]);
        layout.gap = Some(LengthSpec::Px(10.0));
        layout.width = Some(LengthSpec::Fill);
    });
    widget(grid).children(children).key(key).into_any()
}

fn request_column(model: &ShellViewModel) -> AnyView {
    let state = &model.admin.api;
    let endpoint = selected_ref(model);
    let is_http = endpoint.as_ref().is_some_and(|item| item.transport == "external-http");
    let http_read = is_http && (state.method == "GET" || state.method == "HEAD");
    let transport = native_select(
        Select::new(Some(state.transport().to_string())).options(TRANSPORTS.iter().map(|(value, text)| SelectOption::new(*value, *text))),
    );
    let search = style::native_input(TextInput::new(state.keyword.clone()).label("Search").placeholder("plugin / repository / command"));
    let filters = two_columns(
        GridTrack::Px(126.0),
        GridTrack::MinMax { min_px: 0.0, fr: 1.0, max_px: None },
        (
            field("Transport", widget(transport).key("admin-api-transport").on_cx(|_, event: &SelectChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetTransport(event.value.to_string())))).into_any()),
            field("Search", widget(search).key("admin-api-keyword").on_cx(|_, event: &TextChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetKeyword(event.value.to_string())))).into_any()),
        ),
        "admin-api-filters",
    );
    let shown = {
        let items = visible(model);
        if items.is_empty() { endpoints(model) } else { items }
    };
    let endpoint_select = native_select(
        Select::new(Some(state.selected_key.clone())).options(shown.iter().map(|item| SelectOption::new(item.key(), item.option_label()))),
    );
    let methods: Vec<String> = if is_http { ["GET", "POST", "PATCH", "DELETE", "HEAD"].iter().map(|item| item.to_string()).collect() } else { vec![state.method.clone()] };
    let method_select = native_select(Select::new(Some(state.method.clone())).options(methods.iter().map(|item| SelectOption::new(item.clone(), item.clone()))).disabled(!is_http));
    let endpoint_row = two_columns(
        GridTrack::MinMax { min_px: 0.0, fr: 1.0, max_px: None },
        GridTrack::Px(112.0),
        (
            field("Endpoint", widget(endpoint_select).key("admin-api-endpoint").on_cx(|_, event: &SelectChanged, cx| cx.dispatch_program_all(api(ApiMessage::SelectEndpoint(event.value.to_string())))).into_any()),
            field("Method", widget(method_select).key("admin-api-method").on_cx(|_, event: &SelectChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetMethod(event.value.to_string())))).into_any()),
        ),
        "admin-api-endpoint-row",
    );
    let mut body = vec![filters, endpoint_row];
    if let Some(summary) = endpoint.as_ref().map(|item| item.summary.clone()).filter(|text| !text.is_empty()) {
        body.push(widget(wrapping(style::label_lh(summary, 12.0, 400, Role::Muted, 1.5))).key("admin-api-endpoint-summary").into_any());
    }
    let target_value = match endpoint.as_ref() {
        Some(_) if is_http => if state.custom_path.is_empty() { state.path.clone() } else { state.custom_path.clone() },
        Some(item) => target_text(model, item),
        None => String::new(),
    };
    let target = style::native_input(
        TextInput::new(target_value).label("Target").placeholder(if is_http { "/external/v1/health" } else { "" }).read_only(!is_http),
    );
    body.push(field("Target", widget(target).key("admin-api-target").on_cx(|_, event: &TextChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetTarget(event.value.to_string())))).into_any()));
    if is_http {
        // `.api-playground__check input` 是 16 见方。
        let checkbox = style::native_checkbox(Checkbox::new("", state.include_auth), 16.0);
        body.push(
            widget(row(8.0))
                .children((
                    widget(checkbox).key("admin-api-auth").on_cx(|_, event: &ToggleChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetAuth(event.checked)))),
                    widget(label("Authorization: Bearer token", 12.0, 700, Role::Muted)),
                ))
                .key("admin-api-auth-row")
                .into_any(),
        );
    }
    let mut area = TextArea::new(state.request_text.clone()).placeholder("{ }").disabled(http_read);
    area.style = code_area_style(260.0, http_read);
    body.push(
        widget(column(6.0).grow(1.0).with_layout(|layout| layout.min_height = Some(LengthSpec::Px(0.0))))
            .children((
                widget(label("Request JSON", 12.0, 700, Role::Muted)),
                widget(area).key("admin-api-request").on_cx(|_, event: &TextChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetRequestText(event.value.to_string())))),
            ))
            .key("admin-api-request-field")
            .into_any(),
    );
    let sending = state.sending;
    body.push(
        widget(row(12.0).wrap(true).width(LengthSpec::Fill).justify(JustifySpec::End))
            .children((
                widget(action("复制请求", None, Tone::Plain, endpoint.is_none())).key("admin-api-copy").on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(api(ApiMessage::Copy))),
                widget(action(if sending { "发送中" } else { "发送请求" }, None, Tone::Primary, !can_send(model)))
                    .key("admin-api-send-request")
                    .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(api(ApiMessage::Send))),
            ))
            .key("admin-api-actions")
            .into_any(),
    );
    widget(frame(12.0).with_layout(|layout| layout.overflow_y = nana_ui_core::OverflowSpec::Hidden)).children(body).key("admin-api-request-column").into_any()
}

/// 等宽文本域：12/1.5、内边距 10、sm 圆角、`border-strong` 边线、`bg-subtle`。
fn code_area_style(min_height: f32, muted: bool) -> nana_ui::runtime::NodeStyle {
    let mut node = style::native_field_style(min_height);
    if muted {
        node.foreground = Some(Role::Muted);
    }
    node.text_vertical_alignment = nana_ui::runtime::TextVerticalAlignment::Top;
    let layout = Arc::make_mut(&mut node.layout);
    layout.height = None;
    layout.min_height = Some(LengthSpec::Px(min_height));
    layout.flex_grow = Some(1.0);
    layout.white_space_nowrap = false;
    layout.font_family = Some(style::MONO_FAMILY.into());
    layout.font_size = Some(12.0);
    layout.line_height = Some(nana_ui_core::LineHeightSpec::Relative(1.5));
    layout.padding_top = Some(LengthSpec::Px(10.0));
    layout.padding_bottom = Some(LengthSpec::Px(10.0));
    node
}

fn response_column(model: &ShellViewModel) -> AnyView {
    let state = &model.admin.api;
    let status = if state.response_status.is_empty() { "未发送".to_string() } else { state.response_status.clone() };
    let mut head = vec![widget(row(8.0)).children((widget(label("Response", 12.0, 700, Role::Muted)), widget(mono(status, 13.0, 700, Role::Text)).key("admin-api-status"))).into_any()];
    if let Some(duration) = state.duration_ms {
        head.push(widget(label(format!("{duration} ms"), 12.0, 400, Role::Muted)).key("admin-api-duration").into_any());
    }
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        // CSS 只写了行轨道，隐式的一列会拉满宽度；Nana 网格要写明。
        layout.grid_columns = Some(vec![GridTrack::Fr(1.0)]);
        layout.grid_rows = Some(vec![
            GridTrack::MinMax { min_px: 120.0, fr: 0.34, max_px: None },
            GridTrack::MinMax { min_px: 180.0, fr: 1.0, max_px: None },
        ]);
        layout.gap = Some(LengthSpec::Px(10.0));
        layout.width = Some(LengthSpec::Fill);
        layout.flex_grow = Some(1.0);
        layout.min_height = Some(LengthSpec::Px(0.0));
    });
    let block = |title: &str, text: String, key: &'static str| {
        let mut area = TextArea::new(text);
        area.read_only = true;
        area.style = code_area_style(0.0, false);
        {
            let layout = Arc::make_mut(&mut area.style.layout);
            layout.height = Some(LengthSpec::Fill);
        }
        widget(column(6.0).with_layout(|layout| {
            layout.min_height = Some(LengthSpec::Px(0.0));
            layout.height = Some(LengthSpec::Fill);
        }))
        .children((widget(label(title, 12.0, 700, Role::Muted)), widget(area).key(key)))
        .into_any()
    };
    widget(frame(12.0).with_layout(|layout| layout.overflow_y = nana_ui_core::OverflowSpec::Hidden))
        .children((
            widget(style::spread(12.0, AlignSpec::Center)).children(head),
            widget(grid).children((block("Headers", state.response_headers.clone(), "admin-api-headers"), block("Body", state.response_body.clone(), "admin-api-body"))),
        ))
        .key("admin-api-response-column")
        .into_any()
}
