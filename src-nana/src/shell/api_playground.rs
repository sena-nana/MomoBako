//! API Playground 的状态和请求。
//!
//! 照 `External/Plugins/api-playground/src/register.js`：契约来自 API 设计快照，快照为空时
//! 用同一组三个外部 API 兜底；可按传输方式和关键字筛选端点，HTTP 端点可改方法、目标和鉴权，
//! 发送后把状态、响应头和响应体写回。外部 HTTP 走宿主的 http:// 连接；Nana 没有 Tauri
//! 命令通道，「Core」端点照 Vue 在缺 `invokeCommand` 时的报错处理；插件端点走插件调用。

#[path = "api_playground_view.rs"]
pub(crate) mod view;
#[path = "api_http.rs"]
pub(crate) mod http;

use std::time::Instant;

use serde_json::{json, Value};

pub(crate) use http::{HttpRequest, HttpResponse};

use super::super::ShellViewModel;
use super::{AdminEffect, PluginCallOrigin};
use crate::backend::services::repository::ApiDefinition;

const HTTP_METHODS: [&str; 5] = ["GET", "POST", "PATCH", "DELETE", "HEAD"];

/// 一个规范化后的端点。字段同 `normalizeEndpoint`。
#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    pub group: String,
    pub transport: String,
    pub method: String,
    pub path: String,
    pub summary: String,
    pub command: Option<String>,
    pub plugin_id: Option<String>,
    pub plugin_method: Option<String>,
    pub requires_auth: bool,
    pub request_template: Option<Value>,
}

impl Endpoint {
    /// 下拉框和选中态用的稳定键。
    pub fn key(&self) -> String {
        match self.transport.as_str() {
            "tauri-command" => format!("tauri-command:{}", self.command.clone().unwrap_or_else(|| self.path.clone())),
            "plugin-call" => format!(
                "plugin-call:{}:{}",
                self.plugin_id.clone().unwrap_or_default(),
                self.plugin_method.clone().unwrap_or_default()
            ),
            transport => format!("{transport}:{}:{}", self.method, self.path),
        }
    }

    /// 下拉选项文字：`HTTP 分组 / 方法 目标`。
    pub fn option_label(&self) -> String {
        let short = match self.transport.as_str() {
            "external-http" => "HTTP".to_string(),
            "tauri-command" => "CORE".to_string(),
            "plugin-call" => "PLUG".to_string(),
            other => other.to_uppercase(),
        };
        format!("{short} {} / {} {}", self.group, self.method, self.label())
    }

    fn label(&self) -> String {
        match self.transport.as_str() {
            "tauri-command" => self.command.clone().unwrap_or_else(|| self.path.clone()),
            "plugin-call" => self.plugin_target(),
            _ => trim_external_prefix(&self.path),
        }
    }

    fn plugin_target(&self) -> String {
        format!("{}:{}", self.plugin_id.clone().unwrap_or_default(), self.plugin_method.clone().unwrap_or_default())
    }

    /// 传输方式的显示名：HTTP、Core、Plugin。
    pub fn transport_label(&self) -> String {
        match self.transport.as_str() {
            "external-http" => "HTTP".into(),
            "tauri-command" => "Core".into(),
            "plugin-call" => "Plugin".into(),
            other => other.to_string(),
        }
    }

    fn from_definition(definition: &ApiDefinition) -> Self {
        let mut endpoint = Self {
            group: definition.group.clone(),
            transport: definition.transport.clone(),
            method: definition.method.clone(),
            path: definition.path.clone(),
            summary: definition.summary.clone(),
            command: definition.command.clone(),
            plugin_id: definition.plugin_id.clone(),
            plugin_method: definition.plugin_method.clone(),
            requires_auth: definition.requires_auth.unwrap_or(false),
            request_template: definition.request_template.clone(),
        };
        endpoint.normalize();
        endpoint
    }

    /// 补齐传输方式、方法和路径，同 Vue `normalizeEndpoint`。
    fn normalize(&mut self) {
        if self.transport.trim().is_empty() {
            self.transport = if self.plugin_id.is_some() || self.plugin_method.is_some() || self.method == "PLUGIN" {
                "plugin-call".into()
            } else if self.command.is_some() || self.method == "INVOKE" {
                "tauri-command".into()
            } else {
                "external-http".into()
            };
        }
        if self.method.trim().is_empty() {
            self.method = if self.transport == "tauri-command" { "INVOKE".into() } else { "GET".into() };
        }
        if self.path.trim().is_empty() {
            self.path = self.command.clone().or_else(|| self.plugin_method.clone()).unwrap_or_default();
        }
    }
}

/// 快照为空时的三个外部 API，和 Vue 的 `FALLBACK_ENDPOINTS` 一致。
fn fallback_endpoints() -> Vec<Endpoint> {
    let http = |method: &str, path: &str, summary: &str, auth: bool, template: Option<Value>| Endpoint {
        group: "External Asset API".into(),
        transport: "external-http".into(),
        method: method.into(),
        path: path.into(),
        summary: summary.into(),
        command: None,
        plugin_id: None,
        plugin_method: None,
        requires_auth: auth,
        request_template: template,
    };
    vec![
        http("GET", "/external/v1/health", "检查外部 API 服务状态。", false, None),
        http("GET", "/external/v1/repositories", "列出可接收外部素材的本地仓库。", true, None),
        http(
            "POST",
            "/external/v1/assets:add",
            "从远程 URL 添加素材到仓库。",
            true,
            Some(json!({
                "repoId": "",
                "parentPath": "",
                "client": { "id": "momobako.api-playground", "name": "API Playground", "version": "0.1.0" },
                "items": [{
                    "kind": "remoteUrl",
                    "url": "https://example.com/image.png",
                    "filename": "image.png",
                    "metadata": { "sourceUrl": "https://example.com/image.png" }
                }]
            })),
        ),
    ]
}

/// 当前可用的端点：快照里有就用快照，否则兜底。
pub(crate) fn endpoints(model: &ShellViewModel) -> Vec<Endpoint> {
    let snapshot = model
        .admin
        .api_design
        .as_ref()
        .map(|api| api.endpoints.iter().map(Endpoint::from_definition).collect::<Vec<_>>())
        .unwrap_or_default();
    if snapshot.is_empty() { fallback_endpoints() } else { snapshot }
}

/// Playground 上的输入和请求消息。
#[derive(Clone, Debug)]
pub enum ApiMessage {
    SetTransport(String),
    SetKeyword(String),
    SelectEndpoint(String),
    SetMethod(String),
    SetTarget(String),
    SetAuth(bool),
    SetRequestText(String),
    Refresh,
    Send,
    Copy,
    HttpFinished(Result<HttpResponse, String>),
    PluginFinished(Result<Value, String>),
}

/// 页面状态。端点列表每次从快照现算，这里只存选择和输入。
#[derive(Clone, Debug, Default)]
pub struct ApiState {
    pub selected_key: String,
    pub transport_filter: String,
    pub keyword: String,
    pub method: String,
    pub path: String,
    pub custom_path: String,
    pub request_text: String,
    pub include_auth: bool,
    pub response_status: String,
    pub response_headers: String,
    pub response_body: String,
    pub duration_ms: Option<u128>,
    pub sending: bool,
    pub notice: String,
    pub error: String,
    started: Option<Instant>,
}

impl ApiState {
    pub fn transport(&self) -> &str {
        if self.transport_filter.is_empty() { "all" } else { &self.transport_filter }
    }
}

/// 选中的端点：键对得上就用它，否则第一个。第一次进来时按它填好方法和目标。
pub(crate) fn selected(model: &mut ShellViewModel) -> Option<Endpoint> {
    let list = endpoints(model);
    let current = list.iter().find(|endpoint| endpoint.key() == model.admin.api.selected_key).cloned().or_else(|| list.first().cloned())?;
    if current.key() != model.admin.api.selected_key {
        model.admin.api.selected_key = current.key();
        apply_endpoint(&mut model.admin.api, &current);
    }
    Some(current)
}

/// 只读查询：当前选中的端点。
pub(crate) fn selected_ref(model: &ShellViewModel) -> Option<Endpoint> {
    let list = endpoints(model);
    list.iter().find(|endpoint| endpoint.key() == model.admin.api.selected_key).cloned().or_else(|| list.first().cloned())
}

/// 按筛选条件留下的端点。没有命中时下拉框仍列全部，和 Vue 一致。
pub(crate) fn visible(model: &ShellViewModel) -> Vec<Endpoint> {
    let state = &model.admin.api;
    let query = state.keyword.trim().to_lowercase();
    endpoints(model)
        .into_iter()
        .filter(|endpoint| state.transport() == "all" || endpoint.transport == state.transport())
        .filter(|endpoint| {
            if query.is_empty() {
                return true;
            }
            [
                Some(endpoint.group.clone()),
                Some(endpoint.method.clone()),
                Some(endpoint.path.clone()),
                Some(endpoint.summary.clone()),
                endpoint.command.clone(),
                endpoint.plugin_id.clone(),
                endpoint.plugin_method.clone(),
            ]
            .into_iter()
            .flatten()
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
            .contains(&query)
        })
        .collect()
}

/// 选中端点后重置方法、目标、鉴权和请求体。
fn apply_endpoint(state: &mut ApiState, endpoint: &Endpoint) {
    state.method = endpoint.method.clone();
    state.path = endpoint.path.clone();
    state.custom_path.clear();
    state.include_auth = endpoint.requires_auth;
    state.request_text = request_template(endpoint);
}

/// 请求体模板：HTTP 的 GET/HEAD 为空，命令和插件默认 `{}`。
fn request_template(endpoint: &Endpoint) -> String {
    if endpoint.transport == "external-http" && (endpoint.method == "GET" || endpoint.method == "HEAD") {
        return String::new();
    }
    let template = endpoint.request_template.clone().or_else(|| match endpoint.transport.as_str() {
        "tauri-command" | "plugin-call" => Some(json!({})),
        _ => None,
    });
    template.map(|value| serde_json::to_string_pretty(&value).unwrap_or_default()).unwrap_or_default()
}

/// 外部 API 的根地址：去掉 `/external/v1` 后缀。
pub(crate) fn root_url(model: &ShellViewModel) -> String {
    let base = model.admin.external.as_ref().map(|item| item.base_url.clone()).unwrap_or_default();
    let trimmed = base.trim_end_matches('/');
    trimmed.strip_suffix("/external/v1").unwrap_or(trimmed).to_string()
}

/// 实际请求地址：自定义目标优先。
pub(crate) fn request_url(model: &ShellViewModel) -> String {
    let state = &model.admin.api;
    let target = if state.custom_path.trim().is_empty() { state.path.clone() } else { state.custom_path.trim().to_string() };
    join_url(&root_url(model), &target)
}

fn join_url(root: &str, path: &str) -> String {
    if root.is_empty() || path.is_empty() {
        return String::new();
    }
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return path.to_string();
    }
    format!("{}/{}", root.trim_end_matches('/'), path.trim_start_matches('/'))
}

fn trim_external_prefix(path: &str) -> String {
    match path.strip_prefix("/external/v1") {
        Some(rest) if rest.is_empty() => "/".into(),
        Some(rest) if rest.starts_with('/') => rest.to_string(),
        _ => path.to_string(),
    }
}

/// 页头副标题：传输方式加目标。
pub(crate) fn request_summary(model: &ShellViewModel) -> String {
    let Some(endpoint) = selected_ref(model) else {
        return "等待契约".into();
    };
    format!("{} {}", endpoint.transport_label(), target_text(model, &endpoint))
}

/// 目标框里的文字：HTTP 是完整地址，命令是命令名，插件是 `插件:方法`。
pub(crate) fn target_text(model: &ShellViewModel, endpoint: &Endpoint) -> String {
    match endpoint.transport.as_str() {
        "external-http" => {
            let url = request_url(model);
            if url.is_empty() { endpoint.path.clone() } else { url }
        }
        "tauri-command" => endpoint.command.clone().unwrap_or_else(|| endpoint.path.clone()),
        "plugin-call" => endpoint.plugin_target(),
        _ => endpoint.path.clone(),
    }
}

/// 发送按钮可用：有端点、没在发送，并且该传输方式需要的目标齐全。
pub(crate) fn can_send(model: &ShellViewModel) -> bool {
    let state = &model.admin.api;
    let Some(endpoint) = selected_ref(model) else {
        return false;
    };
    if state.sending {
        return false;
    }
    match endpoint.transport.as_str() {
        "external-http" => {
            let target = if state.custom_path.trim().is_empty() { state.path.clone() } else { state.custom_path.clone() };
            !root_url(model).is_empty() && !state.method.is_empty() && !target.trim().is_empty()
        }
        "tauri-command" => endpoint.command.is_some() || !endpoint.path.is_empty(),
        "plugin-call" => endpoint.plugin_id.is_some() && endpoint.plugin_method.is_some(),
        _ => false,
    }
}

/// Token 预览：前 10 位加后 6 位。
pub(crate) fn token_preview(model: &ShellViewModel) -> String {
    super::support::mask_token(model.admin.external.as_ref().map(|item| item.token.as_str()))
}

/// 归约 Playground 消息。
pub(crate) fn reduce(model: &mut ShellViewModel, message: ApiMessage) {
    let _ = selected(model);
    match message {
        ApiMessage::SetTransport(value) => {
            model.admin.api.transport_filter = value;
            keep_visible_selection(model);
        }
        ApiMessage::SetKeyword(value) => {
            model.admin.api.keyword = value;
            keep_visible_selection(model);
        }
        ApiMessage::SelectEndpoint(key) => {
            if let Some(endpoint) = endpoints(model).into_iter().find(|endpoint| endpoint.key() == key) {
                model.admin.api.selected_key = key;
                apply_endpoint(&mut model.admin.api, &endpoint);
            } else {
                eprintln!("Nana API Playground 找不到端点：{key}");
            }
        }
        ApiMessage::SetMethod(value) => {
            if HTTP_METHODS.contains(&value.as_str()) {
                model.admin.api.method = value;
            }
        }
        ApiMessage::SetTarget(value) => model.admin.api.custom_path = value,
        ApiMessage::SetAuth(value) => model.admin.api.include_auth = value,
        ApiMessage::SetRequestText(value) => model.admin.api.request_text = value,
        ApiMessage::Refresh => {
            model.admin.api.notice.clear();
            model.admin.api.error.clear();
            model.admin.begin_settings_load();
        }
        ApiMessage::Send => send(model),
        ApiMessage::Copy => copy(model),
        ApiMessage::HttpFinished(result) => finish_http(model, result),
        ApiMessage::PluginFinished(result) => finish_plugin(model, result),
    }
}

/// 筛选后选中项不在列表里时，换成第一个可见端点。
fn keep_visible_selection(model: &mut ShellViewModel) {
    let items = visible(model);
    if items.is_empty() || items.iter().any(|endpoint| endpoint.key() == model.admin.api.selected_key) {
        return;
    }
    let first = items[0].clone();
    model.admin.api.selected_key = first.key();
    apply_endpoint(&mut model.admin.api, &first);
}

fn reset_response(state: &mut ApiState) {
    state.notice.clear();
    state.error.clear();
    state.response_status.clear();
    state.response_headers.clear();
    state.response_body.clear();
    state.duration_ms = None;
}

fn parse_request(text: &str, label: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(trimmed).map_err(|error| format!("{label}：{error}"))
}

/// 发送当前请求。参数无效时直接写错误，不发请求。
fn send(model: &mut ShellViewModel) {
    reset_response(&mut model.admin.api);
    let Some(endpoint) = selected_ref(model) else {
        model.admin.api.error = "当前 API 尚未准备好。".into();
        return;
    };
    if !can_send(model) {
        model.admin.api.error = "当前 API 尚未准备好。".into();
        return;
    }
    model.admin.api.sending = true;
    model.admin.api.started = Some(Instant::now());
    match endpoint.transport.as_str() {
        "external-http" => {
            let state = &model.admin.api;
            let mut headers = Vec::new();
            if state.include_auth
                && let Some(token) = model.admin.external.as_ref().map(|item| item.token.clone()).filter(|token| !token.is_empty())
            {
                headers.push(("Authorization".to_string(), format!("Bearer {token}")));
            }
            let mut body = None;
            if state.method != "GET" && state.method != "HEAD" {
                match parse_request(&state.request_text, "请求 JSON 无效") {
                    Ok(value) => {
                        headers.push(("Content-Type".into(), "application/json".into()));
                        body = Some(value.to_string());
                    }
                    Err(error) => return fail(model, error),
                }
            }
            let request = HttpRequest { method: state.method.clone(), url: request_url(model), headers, body };
            model.admin.push_effect(AdminEffect::HttpRequest(request));
        }
        "tauri-command" => fail(model, "当前插件 SDK 未提供 invokeCommand。".into()),
        "plugin-call" => match parse_request(&model.admin.api.request_text, "插件 Payload JSON 无效") {
            Ok(payload) => model.admin.push_effect(AdminEffect::CallPlugin {
                plugin_id: endpoint.plugin_id.clone().unwrap_or_default(),
                method: endpoint.plugin_method.clone().unwrap_or_default(),
                payload,
                repository_id: None,
                origin: PluginCallOrigin::Playground,
            }),
            Err(error) => fail(model, error),
        },
        other => fail(model, format!("unsupported transport: {other}")),
    }
}

/// 请求失败：状态写 ERROR，响应体是 `{"error": 原因}`。
fn fail(model: &mut ShellViewModel, error: String) {
    eprintln!("Nana API Playground 请求失败：{error}");
    let state = &mut model.admin.api;
    state.duration_ms = state.started.take().map(|started| started.elapsed().as_millis());
    state.response_status = "ERROR".into();
    state.response_body = serde_json::to_string_pretty(&json!({ "error": error })).unwrap_or_default();
    state.error = error;
    state.sending = false;
}

fn finish_http(model: &mut ShellViewModel, result: Result<HttpResponse, String>) {
    let response = match result {
        Ok(response) => response,
        Err(error) => return fail(model, error),
    };
    let state = &mut model.admin.api;
    state.duration_ms = state.started.take().map(|started| started.elapsed().as_millis());
    state.response_status = format!("{} {}", response.status, response.status_text).trim().to_string();
    let headers = response.headers.iter().map(|(name, value)| (name.to_ascii_lowercase(), Value::String(value.clone()))).collect::<serde_json::Map<_, _>>();
    state.response_headers = serde_json::to_string_pretty(&Value::Object(headers)).unwrap_or_default();
    state.response_body = format_text(&response.body);
    state.sending = false;
}

fn finish_plugin(model: &mut ShellViewModel, result: Result<Value, String>) {
    let response = match result {
        Ok(response) => response,
        Err(error) => return fail(model, error),
    };
    let endpoint = selected_ref(model);
    let state = &mut model.admin.api;
    state.duration_ms = state.started.take().map(|started| started.elapsed().as_millis());
    state.response_status = "OK".into();
    state.response_headers = serde_json::to_string_pretty(&json!({
        "transport": "plugin-call",
        "pluginId": endpoint.as_ref().and_then(|item| item.plugin_id.clone()),
        "method": endpoint.as_ref().and_then(|item| item.plugin_method.clone()),
    }))
    .unwrap_or_default();
    state.response_body = if response.is_null() { String::new() } else { serde_json::to_string_pretty(&response).unwrap_or_default() };
    state.sending = false;
}

/// 能解析成 JSON 就格式化，否则原样。
fn format_text(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(text) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| text.to_string()),
        Err(_) => text.to_string(),
    }
}

/// 复制请求：HTTP 是 curl，命令是 `invoke(...)`，插件是调用 JSON。
fn copy(model: &mut ShellViewModel) {
    model.admin.api.notice.clear();
    model.admin.api.error.clear();
    let Some(endpoint) = selected_ref(model) else {
        return;
    };
    let state = &model.admin.api;
    let snippet = match endpoint.transport.as_str() {
        "external-http" => {
            let token = if state.include_auth { model.admin.external.as_ref().map(|item| item.token.clone()).unwrap_or_default() } else { String::new() };
            curl(&state.method, &request_url(model), &token, &state.request_text)
        }
        "tauri-command" => {
            let command = endpoint.command.clone().unwrap_or_else(|| endpoint.path.clone());
            let args = if state.request_text.trim().is_empty() { "{}".to_string() } else { state.request_text.trim().to_string() };
            format!("await invoke({}, {args});", Value::String(command))
        }
        "plugin-call" => match parse_request(&state.request_text, "插件 Payload JSON 无效") {
            Ok(payload) => serde_json::to_string_pretty(&json!({
                "pluginId": endpoint.plugin_id,
                "method": endpoint.plugin_method,
                "payload": payload,
            }))
            .unwrap_or_default(),
            Err(error) => {
                model.admin.api.error = format!("复制失败：{error}");
                return;
            }
        },
        _ => state.request_text.clone(),
    };
    model.admin.push_effect(AdminEffect::CopyText(snippet));
    model.admin.api.notice = "请求已复制。".into();
}

fn curl(method: &str, url: &str, token: &str, body: &str) -> String {
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
    let mut lines = vec![format!("curl -X {method} {}", quote(url))];
    if !token.is_empty() {
        lines.push(format!("  -H {}", quote(&format!("Authorization: Bearer {token}"))));
    }
    if method != "GET" && method != "HEAD" {
        lines.push(format!("  -H {}", quote("Content-Type: application/json")));
        if !body.trim().is_empty() {
            lines.push(format!("  --data {}", quote(body.trim())));
        }
    }
    lines.join(" \\\n")
}
