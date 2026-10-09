//! API Playground 的界面。
//!
//! 版式照 `.api-playground`：页头（眉题、22 号标题、等宽副标题，右侧刷新契约和发送）、
//! 提示、四个元信息块，下面左右两栏：左栏请求（传输与搜索、端点与方法、说明、目标、鉴权、
//! 请求 JSON、复制和发送），右栏响应（状态、耗时、响应头和响应体）。
//!
//! 页面只建一次，只读 [`PlaygroundSignals`]：下拉框的选项和当前值、按钮的文案和禁用、提示和响应
//! 都是绑定；搜索框、目标框和请求 JSON 受控，草稿走 `ModelField`，组合输入中不被打断。

use std::sync::Arc;

use nana_ui::runtime::view::{fields, node_ref, signal, widget, AnyView, El, IntoView, NodeRef, Signal};
use nana_ui::runtime::{
    Activate, AlignSpec, Checkbox, JustifySpec, LengthSpec, Select, SelectChanged, SelectOption, Stack, TextArea, TextChanged,
    TextInput, ToggleChanged,
};
use nana_ui_core::{GridTrack, RadiusTier, SemanticColorMix, SemanticColorRole as Role};

use super::super::super::hot::ModelField;
use super::super::super::{ShellMessage, ShellViewModel};
use super::super::bind::{ActionDisabled, ReadOnly};
use super::super::style::{self, action, column, label, mono, pad, row, wrapping, Tone};
use super::super::AdminMessage;
use super::{can_send, endpoints, request_summary, selected_ref, target_text, token_preview, visible, ApiMessage};

const TRANSPORTS: [(&str, &str); 4] = [("all", "全部"), ("external-http", "HTTP"), ("tauri-command", "Core"), ("plugin-call", "Plugin")];

fn api(message: ApiMessage) -> ShellMessage {
    ShellMessage::Admin(AdminMessage::Api(message))
}

/// 页头、提示和元信息块。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PlaygroundHead {
    /// 设置数据在读：刷新契约禁用。
    pub loading: bool,
    pub summary: String,
    pub sending: bool,
    pub can_send: bool,
    /// 错误提示（危险色）和普通提示。
    pub error: String,
    pub notice: String,
    /// API 数、Base URL、Token 和契约传输方式。
    pub meta: [String; 4],
}

/// 请求栏的选项和状态。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PlaygroundRequest {
    pub transport: String,
    /// 端点下拉的 `(键, 文字)`。
    pub endpoints: Vec<(String, String)>,
    pub selected: String,
    pub has_endpoint: bool,
    pub methods: Vec<String>,
    pub method: String,
    pub is_http: bool,
    /// HTTP 的 GET、HEAD 不带请求体：请求 JSON 禁用、弱色。
    pub http_read: bool,
    pub summary: String,
    pub include_auth: bool,
}

/// 响应栏。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PlaygroundResponse {
    pub status: String,
    pub duration: Option<String>,
    pub headers: String,
    pub body: String,
}

/// API Playground 的信号。句柄都是 `Copy` 的 id。
#[derive(Clone, Copy)]
pub(crate) struct PlaygroundSignals {
    head: Signal<PlaygroundHead>,
    request: Signal<PlaygroundRequest>,
    response: Signal<PlaygroundResponse>,
    keyword: ModelField,
    target: ModelField,
    body: ModelField,
}

impl PlaygroundSignals {
    /// 空的信号，进路由之前由同步写入第一份投影。
    pub(crate) fn new() -> Self {
        Self {
            head: signal(PlaygroundHead::default()),
            request: signal(PlaygroundRequest::default()),
            response: signal(PlaygroundResponse::default()),
            keyword: ModelField::new(""),
            target: ModelField::new(""),
            body: ModelField::new(""),
        }
    }

    /// 按 ViewModel 写入页面的投影：只写变了的信号，输入框的草稿只在 ViewModel 的值变了时写。
    pub(crate) fn write(&self, model: &ShellViewModel) {
        let state = &model.admin.api;
        let admin = &model.admin;
        let loading = admin.loading_settings;
        let error = if !state.error.is_empty() {
            state.error.clone()
        } else if admin.api_design.is_none() && !admin.load_error.is_empty() && !loading {
            format!("API 契约加载失败：{}", admin.load_error)
        } else {
            String::new()
        };
        let notice = if !state.notice.is_empty() {
            state.notice.clone()
        } else if admin.external.is_none() && !admin.load_error.is_empty() && !loading {
            format!("外部 API 连接未加载：{}", admin.load_error)
        } else {
            String::new()
        };
        let base = admin.external.as_ref().map(|item| item.base_url.clone()).filter(|text| !text.is_empty()).unwrap_or_else(|| "未加载".into());
        let contract = admin.api_design.as_ref().map(|api| api.transport.clone()).unwrap_or_else(|| "未加载".into());
        self.head.try_set_if_changed(PlaygroundHead {
            loading,
            summary: request_summary(model),
            sending: state.sending,
            can_send: can_send(model),
            error,
            notice,
            meta: [endpoints(model).len().to_string(), base, token_preview(model), contract],
        });
        let endpoint = selected_ref(model);
        let is_http = endpoint.as_ref().is_some_and(|item| item.transport == "external-http");
        let shown = {
            let items = visible(model);
            if items.is_empty() { endpoints(model) } else { items }
        };
        self.request.try_set_if_changed(PlaygroundRequest {
            transport: state.transport().to_string(),
            endpoints: shown.iter().map(|item| (item.key(), item.option_label())).collect(),
            selected: state.selected_key.clone(),
            has_endpoint: endpoint.is_some(),
            methods: if is_http { ["GET", "POST", "PATCH", "DELETE", "HEAD"].iter().map(|item| item.to_string()).collect() } else { vec![state.method.clone()] },
            method: state.method.clone(),
            is_http,
            http_read: is_http && (state.method == "GET" || state.method == "HEAD"),
            summary: endpoint.as_ref().map(|item| item.summary.clone()).unwrap_or_default(),
            include_auth: state.include_auth,
        });
        self.response.try_set_if_changed(PlaygroundResponse {
            status: if state.response_status.is_empty() { "未发送".to_string() } else { state.response_status.clone() },
            duration: state.duration_ms.map(|duration| format!("{duration} ms")),
            headers: state.response_headers.clone(),
            body: state.response_body.clone(),
        });
        let target = match endpoint.as_ref() {
            Some(_) if is_http => if state.custom_path.is_empty() { state.path.clone() } else { state.custom_path.clone() },
            Some(item) => target_text(model, item),
            None => String::new(),
        };
        self.keyword.sync(&state.keyword);
        self.target.sync(&target);
        self.body.sync(&state.request_text);
    }
}

/// 整个工具页，内边距 18，竖排间距 14，占满工具页高度。
pub(crate) fn playground(signals: PlaygroundSignals) -> AnyView {
    let head = signals.head;
    let body = (
        header(signals),
        style::state_notice(move || head.with(|head| head.error.clone()), true, "admin-api-error")
            .visible(move || head.with(|head| !head.error.is_empty())),
        style::state_notice(move || head.with(|head| head.notice.clone()), false, "admin-api-notice")
            .visible(move || head.with(|head| !head.notice.is_empty())),
        meta(head),
        columns(signals),
    );
    widget(pad(Stack::fill_column(14.0), 18.0, 18.0, 18.0, 18.0)).children(body).key("admin-api-playground").into_any()
}

fn header(signals: PlaygroundSignals) -> AnyView {
    let head = signals.head;
    let left = widget(Stack::column(0.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
        widget(style::eyebrow_text("API Playground")).key("admin-api-eyebrow"),
        widget(pad(column(0.0), 4.0, 0.0, 0.0, 0.0)).children((widget(style::label_lh("后端 API 调试", 22.0, 700, Role::Text, 1.25)).key("admin-api-title"),)),
        widget(pad(column(0.0), 8.0, 0.0, 0.0, 0.0))
            .children((style::bound(wrapping(mono(String::new(), 12.0, 400, Role::Muted)), move || head.with(|head| head.summary.clone())).key("admin-api-summary"),)),
    ));
    let actions = widget(row(12.0).wrap(true).align(AlignSpec::Start)).children((
        widget(action("刷新契约", None, Tone::Plain, false))
            .prop::<bool, ActionDisabled>(move || head.with(|head| head.loading))
            .key("admin-api-refresh")
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(api(ApiMessage::Refresh))),
        send_button("发送", head).key("admin-api-send"),
    ));
    widget(style::spread(12.0, AlignSpec::Start)).children((left, actions)).into_any()
}

/// 发送按钮：发送中改文案，不能发送时禁用。
fn send_button(text: &'static str, head: Signal<PlaygroundHead>) -> El<nana_ui::runtime::Button> {
    widget(action(text, None, Tone::Primary, false))
        .prop::<String, fields::button::label>(move || if head.with(|head| head.sending) { "发送中" } else { text }.to_string())
        .prop::<bool, ActionDisabled>(move || head.with(|head| !head.can_send))
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(api(ApiMessage::Send)))
}

/// 四个元信息块：API 数、Base URL、Token 和契约传输方式。
fn meta(head: Signal<PlaygroundHead>) -> AnyView {
    let blocks = ["API", "Base URL", "Token", "契约"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            widget(pad(row(4.0), 4.0, 9.0, 4.0, 9.0).surface(Role::Background).radius(RadiusTier::Sm).with_layout(|layout| {
                layout.min_height = Some(LengthSpec::Px(26.0));
            }))
            .children((
                widget(label(name, 12.0, 400, Role::Muted)),
                style::bound(mono(String::new(), 12.0, 600, Role::Text), move || head.with(|head| head.meta[index].clone())).key(format!("admin-api-meta-{index}")),
            ))
            .into_any()
        })
        .collect::<Vec<_>>();
    widget(row(8.0).wrap(true).width(LengthSpec::Fill)).children(blocks).key("admin-api-meta").into_any()
}

/// 左右两栏：请求栏最小 260、最宽 420，响应栏占余下宽度。
fn columns(signals: PlaygroundSignals) -> AnyView {
    let grid = Stack::from_layout(nana_ui_core::LayoutStyle::default()).with_layout(|layout| {
        layout.display = Some(nana_ui_core::DisplaySpec::Grid);
        layout.grid_columns = Some(vec![style::capped(260.0, 420.0), GridTrack::MinMax { min_px: 0.0, fr: 1.0, max_px: None }]);
        layout.gap = Some(LengthSpec::Px(12.0));
        layout.width = Some(LengthSpec::Fill);
        layout.flex_grow = Some(1.0);
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.align_items = AlignSpec::Stretch;
    });
    widget(grid).children((request_column(signals), response_column(signals.response))).into_any()
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
/// 标题记进 `caption`，没有自己名字的下拉框用它当读屏名称。
fn field(title: &str, caption: NodeRef, control: AnyView) -> AnyView {
    widget(column(6.0).min_width(LengthSpec::Px(0.0)))
        .children((widget(label(title, 12.0, 700, Role::Muted)).node_ref(caption), control))
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

/// 下拉框选项。
fn options(items: &[(String, String)]) -> Vec<SelectOption> {
    items.iter().map(|(value, text)| SelectOption::new(value.clone(), text.clone())).collect()
}

fn request_column(signals: PlaygroundSignals) -> AnyView {
    let request = signals.request;
    let initial = request.get_untracked();
    let transport_caption = node_ref();
    let transport = native_select(Select::new(Some(initial.transport.clone())).options(TRANSPORTS.iter().map(|(value, text)| SelectOption::new(*value, *text))));
    let keyword = signals.keyword.signal();
    let search = style::native_input(TextInput::new(keyword.get_untracked()).label("Search").placeholder("plugin / repository / command"));
    let filters = two_columns(
        GridTrack::Px(126.0),
        GridTrack::MinMax { min_px: 0.0, fr: 1.0, max_px: None },
        (
            field(
                "Transport",
                transport_caption,
                widget(transport)
                    .prop::<Option<Arc<str>>, fields::select::value>(move || request.with(|request| Some(Arc::from(request.transport.as_str()))))
                    .labelled_by(transport_caption)
                    .key("admin-api-transport")
                    .on_cx(|_, event: &SelectChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetTransport(event.value.to_string()))))
                    .into_any(),
            ),
            field(
                "Search",
                node_ref(),
                widget(search)
                    .model(keyword)
                    .key("admin-api-keyword")
                    .on_cx(|_, event: &TextChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetKeyword(event.value.to_string()))))
                    .into_any(),
            ),
        ),
        "admin-api-filters",
    );
    let endpoint_caption = node_ref();
    let method_caption = node_ref();
    let endpoint_select = native_select(Select::new(Some(initial.selected.clone())).options(options(&initial.endpoints)));
    let method_options = |methods: &[String]| methods.iter().map(|item| SelectOption::new(item.clone(), item.clone())).collect::<Vec<_>>();
    let method_select = native_select(Select::new(Some(initial.method.clone())).options(method_options(&initial.methods)).disabled(!initial.is_http));
    let endpoint_row = two_columns(
        GridTrack::MinMax { min_px: 0.0, fr: 1.0, max_px: None },
        GridTrack::Px(112.0),
        (
            field(
                "Endpoint",
                endpoint_caption,
                widget(endpoint_select)
                    .prop::<Vec<SelectOption>, fields::select::options>(move || request.with(|request| options(&request.endpoints)))
                    .prop::<Option<Arc<str>>, fields::select::value>(move || request.with(|request| Some(Arc::from(request.selected.as_str()))))
                    .labelled_by(endpoint_caption)
                    .key("admin-api-endpoint")
                    .on_cx(|_, event: &SelectChanged, cx| cx.dispatch_program_all(api(ApiMessage::SelectEndpoint(event.value.to_string()))))
                    .into_any(),
            ),
            field(
                "Method",
                method_caption,
                widget(method_select)
                    .prop::<Vec<SelectOption>, fields::select::options>(move || request.with(|request| method_options(&request.methods)))
                    .prop::<Option<Arc<str>>, fields::select::value>(move || request.with(|request| Some(Arc::from(request.method.as_str()))))
                    .prop::<bool, fields::select::disabled>(move || request.with(|request| !request.is_http))
                    .labelled_by(method_caption)
                    .key("admin-api-method")
                    .on_cx(|_, event: &SelectChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetMethod(event.value.to_string()))))
                    .into_any(),
            ),
        ),
        "admin-api-endpoint-row",
    );
    let summary = style::bound(wrapping(style::label_lh(String::new(), 12.0, 400, Role::Muted, 1.5)), move || request.with(|request| request.summary.clone()))
        .visible(move || request.with(|request| !request.summary.is_empty()))
        .key("admin-api-endpoint-summary")
        .into_any();
    let target_value = signals.target.signal();
    let is_http = move || request.with(|request| request.is_http);
    let target = style::native_input(
        TextInput::new(target_value.get_untracked()).label("Target").placeholder(if initial.is_http { "/external/v1/health" } else { "" }).read_only(!initial.is_http),
    );
    let target = field(
        "Target",
        node_ref(),
        widget(target)
            .model(target_value)
            .prop::<Arc<str>, fields::text_input::placeholder>(move || Arc::from(if is_http() { "/external/v1/health" } else { "" }))
            .prop::<bool, ReadOnly>(move || !is_http())
            .key("admin-api-target")
            .on_cx(|_, event: &TextChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetTarget(event.value.to_string()))))
            .into_any(),
    );
    // `.api-playground__check input` 是 16 见方。勾选框没有自己的文字，用旁边那段说明当读屏名称。
    let auth_caption = node_ref();
    let checkbox = style::native_checkbox(Checkbox::new("", initial.include_auth), 16.0);
    let auth = widget(row(8.0))
        .visible(is_http)
        .children((
            widget(checkbox)
                .prop::<bool, fields::checkbox::checked>(move || request.with(|request| request.include_auth))
                .labelled_by(auth_caption)
                .key("admin-api-auth")
                .on_cx(|_, event: &ToggleChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetAuth(event.checked)))),
            widget(label("Authorization: Bearer token", 12.0, 700, Role::Muted)).node_ref(auth_caption),
        ))
        .key("admin-api-auth-row")
        .into_any();
    let http_read = move || request.with(|request| request.http_read);
    let body_value = signals.body.signal();
    let body_caption = node_ref();
    let mut area = TextArea::new(body_value.get_untracked()).placeholder("{ }").disabled(initial.http_read);
    area.style = code_area_style(260.0);
    let request_body = widget(column(6.0).grow(1.0).with_layout(|layout| layout.min_height = Some(LengthSpec::Px(0.0))))
        .children((
            widget(label("Request JSON", 12.0, 700, Role::Muted)).node_ref(body_caption),
            widget(area)
                .model(body_value)
                .prop::<bool, fields::text_area::disabled>(http_read)
                .foreground(move || Some(if http_read() { Role::Muted } else { Role::Text }))
                .labelled_by(body_caption)
                .key("admin-api-request")
                .on_cx(|_, event: &TextChanged, cx| cx.dispatch_program_all(api(ApiMessage::SetRequestText(event.value.to_string())))),
        ))
        .key("admin-api-request-field")
        .into_any();
    let head = signals.head;
    let actions = widget(row(12.0).wrap(true).width(LengthSpec::Fill).justify(JustifySpec::End))
        .children((
            widget(action("复制请求", None, Tone::Plain, false))
                .prop::<bool, ActionDisabled>(move || request.with(|request| !request.has_endpoint))
                .key("admin-api-copy")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(api(ApiMessage::Copy))),
            send_button("发送请求", head).key("admin-api-send-request"),
        ))
        .key("admin-api-actions")
        .into_any();
    widget(frame(12.0).with_layout(|layout| layout.overflow_y = nana_ui_core::OverflowSpec::Hidden))
        .children((filters, endpoint_row, summary, target, auth, request_body, actions))
        .key("admin-api-request-column")
        .into_any()
}

/// 等宽文本域：12/1.5、内边距 10、sm 圆角、`border-strong` 边线、`bg-subtle`。字色另外绑定。
fn code_area_style(min_height: f32) -> nana_ui::runtime::NodeStyle {
    let mut node = style::native_field_style(min_height);
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

fn response_column(response: Signal<PlaygroundResponse>) -> AnyView {
    let head = widget(style::spread(12.0, AlignSpec::Center)).children((
        widget(row(8.0)).children((
            widget(label("Response", 12.0, 700, Role::Muted)),
            style::bound(mono(String::new(), 13.0, 700, Role::Text), move || response.with(|response| response.status.clone())).key("admin-api-status"),
        )),
        style::bound(label(String::new(), 12.0, 400, Role::Muted), move || response.with(|response| response.duration.clone().unwrap_or_default()))
            .visible(move || response.with(|response| response.duration.is_some()))
            .key("admin-api-duration"),
    ));
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
    let block = |title: &str, pick: fn(&PlaygroundResponse) -> String, key: &'static str| {
        let caption = node_ref();
        let mut area = TextArea::new(response.with_untracked(pick));
        area.read_only = true;
        area.style = code_area_style(0.0);
        {
            let layout = Arc::make_mut(&mut area.style.layout);
            layout.height = Some(LengthSpec::Fill);
        }
        widget(column(6.0).with_layout(|layout| {
            layout.min_height = Some(LengthSpec::Px(0.0));
            layout.height = Some(LengthSpec::Fill);
        }))
        .children((
            widget(label(title, 12.0, 700, Role::Muted)).node_ref(caption),
            widget(area)
                .prop::<String, fields::text_area::value>(move || response.with(pick))
                .labelled_by(caption)
                .key(key),
        ))
        .into_any()
    };
    widget(frame(12.0).with_layout(|layout| layout.overflow_y = nana_ui_core::OverflowSpec::Hidden))
        .children((
            head,
            widget(grid).children((
                block("Headers", |response| response.headers.clone(), "admin-api-headers"),
                block("Body", |response| response.body.clone(), "admin-api-body"),
            )),
        ))
        .key("admin-api-response-column")
        .into_any()
}
