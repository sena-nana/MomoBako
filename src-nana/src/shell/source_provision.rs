//! 来源登录与建仓流程。
//!
//! 照 `SourceAuthenticationSettings.vue`：连接新账号或重新登录先建扫码会话；检查登录结果后，
//! 仓库标识、名称和路径都从登录结果与 `repositoryProvisioning` 推出。已有仓库就更新后端配置
//! （需要本地缓存时再配置缓存目录）并切过去；没有就跳过首次同步建仓再切过去。随后在后台同步，
//! 完成后刷新仓库，失败写「后台同步失败：…」。cookie、密码、secret、token 一类字段一律丢掉。

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use super::super::{ShellMessage, ShellViewModel, WorkspaceRepository};
use super::{AdminEffect, PluginCallOrigin};
use crate::backend::services::repository::{PluginManifest, RepositorySummary};

/// 流程里一步异步调用。结果带着它回来，归约按它决定下一步。
#[derive(Clone, Debug, PartialEq)]
pub enum SourceStep {
    /// 重新登录前读原仓库的账号，用来核对扫码账号。
    BeginStatus { repo_id: String },
    /// 创建扫码会话。
    CreateSession,
    /// 「检查」：读仓库登录状态。
    RefreshStatus { repo_id: String },
    /// 退出前没有缓存的状态时先读一次。
    ClearStatus { repo_id: String },
    /// 调插件的退出方法。
    Clear { repo_id: String },
    /// 退出后把仓库配置标成登录失效。
    ClearUpdate { repo_id: String },
    /// 检查扫码结果。
    Poll,
    /// 已有仓库：更新后端配置。
    ProvisionUpdate { repo_id: String, name: String },
    /// 已有仓库：配置本地缓存目录。
    ProvisionCache { repo_id: String, name: String },
    /// 新仓库：跳过首次同步创建。
    ProvisionCreate { repo_id: String, name: String },
    /// 后台同步。
    Sync { repo_id: String },
}

/// 一个来源插件设置页的状态。收起设置或换插件时重置，对应 Vue 组件卸载。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceAuthState {
    pub plugin_id: String,
    /// 创建会话返回的二维码会话。
    pub session: Option<Value>,
    pub target_repo_id: Option<String>,
    pub expected_account_id: Option<String>,
    pub cache_path: String,
    pub busy: bool,
    pub message: String,
    pub error: String,
    pub status_by_repo: BTreeMap<String, Value>,
    /// 退出时先读到的登录状态，退出调用回来后再用。
    pending_clear_status: Option<Value>,
}

impl SourceAuthState {
    /// 二维码会话键：`sessionId` 或 `unikey`。
    pub fn session_key(&self) -> String {
        let Some(session) = self.session.as_ref() else {
            return String::new();
        };
        text_at(session.get("sessionId")).or_else(|| text_at(session.get("unikey"))).unwrap_or_default()
    }

    /// 「检查登录结果」可用：有会话、缓存目录满足要求、没有调用在途。
    pub fn can_poll(&self, requires_cache: bool) -> bool {
        !self.session_key().is_empty() && (!requires_cache || !self.cache_path.trim().is_empty()) && !self.busy
    }

    fn reset_notice(&mut self) {
        self.message.clear();
        self.error.clear();
    }
}

/// 插件声明的认证对象。
pub(crate) fn authentication(plugin: &PluginManifest) -> Option<&Value> {
    plugin.contributes.get("source").and_then(|item| item.get("authentication")).filter(|item| !item.is_null())
}

fn auth_method(plugin: &PluginManifest, key: &str) -> Option<String> {
    authentication(plugin).and_then(|auth| text_at(auth.get(key)))
}

fn provisioning(plugin: &PluginManifest, key: &str) -> Option<String> {
    authentication(plugin).and_then(|auth| auth.get("repositoryProvisioning")).and_then(|item| text_at(item.get(key)))
}

/// `repositoryProvisioning.requiresLocalCache`。
pub(crate) fn requires_local_cache(plugin: &PluginManifest) -> bool {
    authentication(plugin)
        .and_then(|auth| auth.get("repositoryProvisioning"))
        .and_then(|item| item.get("requiresLocalCache"))
        .and_then(Value::as_bool)
        == Some(true)
}

/// 这个来源插件名下的仓库：插件标识和历史标识都算。
pub(crate) fn source_repositories<'a>(model: &'a ShellViewModel, plugin: &PluginManifest) -> Vec<&'a WorkspaceRepository> {
    let mut accepted = vec![plugin.plugin_id.as_str()];
    accepted.extend(plugin.legacy_plugin_ids.iter().map(String::as_str));
    accepted.extend(plugin.compat.legacy_plugin_ids.iter().map(String::as_str));
    model
        .workspace
        .repositories
        .iter()
        .filter(|repository| accepted.iter().any(|id| repository.backend_plugin_id == *id))
        .collect()
}

/// 仓库的完整摘要，带登录状态和本地缓存。
pub(crate) fn summary<'a>(model: &'a ShellViewModel, repo_id: &str) -> Option<&'a RepositorySummary> {
    model.admin.repository_summaries.iter().find(|item| item.repo_id == repo_id)
}

/// 仓库行上的登录状态：先看本页查过的，再看仓库摘要。都没有是「未检查」。
pub(crate) fn status_label(model: &ShellViewModel, repo_id: &str) -> &'static str {
    if let Some(status) = model.admin.source_auth.status_by_repo.get(repo_id) {
        let logged_in = status.get("loggedIn").and_then(Value::as_bool) == Some(true);
        let expired = status.get("loginExpired").and_then(Value::as_bool) == Some(true);
        return if logged_in && !expired { "已登录" } else { "登录失效" };
    }
    match summary(model, repo_id).and_then(|item| item.authentication.as_ref()) {
        None => "未检查",
        Some(auth) if auth.logged_in && !auth.login_expired => "已登录",
        Some(_) => "登录失效",
    }
}

/// 仓库行上的路径：有本地缓存时显示缓存目录。
pub(crate) fn display_path(model: &ShellViewModel, repository: &WorkspaceRepository) -> String {
    summary(model, &repository.repo_id)
        .and_then(|item| item.local_cache.as_ref())
        .and_then(|cache| cache.path.clone())
        .unwrap_or_else(|| repository.path.clone())
}

fn plugin_of(model: &ShellViewModel, plugin_id: &str) -> Option<PluginManifest> {
    model.admin.plugins.iter().find(|plugin| plugin.plugin_id == plugin_id).cloned()
}

/// 进入或离开某个插件的设置时调用：不是同一个插件就清空状态。
pub(crate) fn bind_plugin(model: &mut ShellViewModel, plugin_id: Option<&str>) {
    let current = model.admin.source_auth.plugin_id.as_str();
    if plugin_id != Some(current) {
        model.admin.source_auth = SourceAuthState { plugin_id: plugin_id.unwrap_or_default().to_string(), ..SourceAuthState::default() };
    }
}

fn call(model: &mut ShellViewModel, plugin_id: &str, method: String, payload: Value, repository_id: Option<String>, step: SourceStep) {
    model.admin.push_effect(AdminEffect::CallPlugin {
        plugin_id: plugin_id.to_string(),
        method,
        payload,
        repository_id,
        origin: PluginCallOrigin::SourceAuth(step),
    });
}

/// 失败收尾：写错误、放开按钮、记日志。
fn fail(model: &mut ShellViewModel, error: String) {
    eprintln!("Nana 来源登录失败：{error}");
    model.admin.source_auth.error = error;
    model.admin.source_auth.busy = false;
}

/// 连接新账号或重新登录。重新登录先读原账号，再建会话。
pub(crate) fn begin(model: &mut ShellViewModel, plugin_id: &str, repo_id: Option<String>) {
    let Some(plugin) = plugin_of(model, plugin_id) else {
        eprintln!("Nana 来源登录找不到插件：{plugin_id}");
        return;
    };
    bind_plugin(model, Some(plugin_id));
    if model.admin.source_auth.busy || authentication(&plugin).is_none() {
        return;
    }
    let cache_path = match repo_id.as_deref() {
        Some(repo_id) if requires_local_cache(&plugin) => summary(model, repo_id)
            .and_then(|item| item.local_cache.as_ref())
            .and_then(|cache| cache.path.clone())
            .unwrap_or_default(),
        Some(repo_id) => model.workspace.repositories.iter().find(|item| item.repo_id == repo_id).map(|item| item.path.clone()).unwrap_or_default(),
        None => String::new(),
    };
    let state = &mut model.admin.source_auth;
    state.busy = true;
    state.reset_notice();
    state.session = None;
    state.target_repo_id = repo_id.clone();
    state.cache_path = cache_path;
    state.expected_account_id = None;
    match repo_id {
        Some(repo_id) => match auth_method(&plugin, "statusMethod") {
            Some(method) => call(model, plugin_id, method, json!({}), Some(repo_id.clone()), SourceStep::BeginStatus { repo_id }),
            None => fail(model, "认证声明没有 statusMethod。".into()),
        },
        None => create_session(model, &plugin),
    }
}

fn create_session(model: &mut ShellViewModel, plugin: &PluginManifest) {
    match auth_method(plugin, "createSessionMethod") {
        Some(method) => call(model, &plugin.plugin_id, method, json!({ "qrImage": true, "qrimg": true }), None, SourceStep::CreateSession),
        None => fail(model, "认证声明没有 createSessionMethod。".into()),
    }
}

/// 取消扫码会话。调用在途时不动。
pub(crate) fn cancel(model: &mut ShellViewModel) {
    let state = &mut model.admin.source_auth;
    if state.busy {
        return;
    }
    state.session = None;
    state.target_repo_id = None;
    state.expected_account_id = None;
    state.cache_path.clear();
    state.reset_notice();
}

/// 「检查」：只带仓库标识查状态，后端配置由宿主注入。
pub(crate) fn refresh_status(model: &mut ShellViewModel, plugin_id: &str, repo_id: &str) {
    let Some(plugin) = plugin_of(model, plugin_id) else {
        eprintln!("Nana 来源状态查询找不到插件：{plugin_id}");
        return;
    };
    bind_plugin(model, Some(plugin_id));
    if model.admin.source_auth.busy {
        return;
    }
    let Some(method) = auth_method(&plugin, "statusMethod") else {
        eprintln!("Nana 来源认证没有 statusMethod：{plugin_id}");
        return;
    };
    model.admin.source_auth.busy = true;
    model.admin.source_auth.reset_notice();
    call(model, plugin_id, method, json!({}), Some(repo_id.to_string()), SourceStep::RefreshStatus { repo_id: repo_id.into() });
}

/// 退出登录：先拿状态（本页查过就用缓存），再调退出方法。
pub(crate) fn clear(model: &mut ShellViewModel, plugin_id: &str, repo_id: &str) {
    let Some(plugin) = plugin_of(model, plugin_id) else {
        eprintln!("Nana 来源退出找不到插件：{plugin_id}");
        return;
    };
    bind_plugin(model, Some(plugin_id));
    if model.admin.source_auth.busy {
        return;
    }
    model.admin.source_auth.busy = true;
    model.admin.source_auth.reset_notice();
    if let Some(status) = model.admin.source_auth.status_by_repo.get(repo_id).cloned() {
        model.admin.source_auth.pending_clear_status = Some(status);
        call_clear(model, &plugin, repo_id);
        return;
    }
    match auth_method(&plugin, "statusMethod") {
        Some(method) => call(model, plugin_id, method, json!({}), Some(repo_id.into()), SourceStep::ClearStatus { repo_id: repo_id.into() }),
        None => fail(model, "认证声明没有 statusMethod。".into()),
    }
}

fn call_clear(model: &mut ShellViewModel, plugin: &PluginManifest, repo_id: &str) {
    match auth_method(plugin, "clearMethod") {
        Some(method) => call(model, &plugin.plugin_id, method, json!({}), Some(repo_id.into()), SourceStep::Clear { repo_id: repo_id.into() }),
        None => fail(model, "认证声明没有 clearMethod。".into()),
    }
}

/// 检查扫码结果。会话缺失、缓存目录未选或调用在途时不发。
pub(crate) fn poll(model: &mut ShellViewModel, plugin_id: &str) {
    let Some(plugin) = plugin_of(model, plugin_id) else {
        eprintln!("Nana 来源扫码检查找不到插件：{plugin_id}");
        return;
    };
    if !model.admin.source_auth.can_poll(requires_local_cache(&plugin)) {
        eprintln!("Nana 来源扫码还不能检查：没有会话、缺缓存目录或调用在途");
        return;
    }
    let Some(method) = auth_method(&plugin, "pollSessionMethod") else {
        eprintln!("Nana 来源认证没有 pollSessionMethod：{plugin_id}");
        return;
    };
    let key = model.admin.source_auth.session_key();
    model.admin.source_auth.busy = true;
    model.admin.source_auth.reset_notice();
    call(model, plugin_id, method, json!({ "key": key, "sessionId": key, "persistSession": true }), None, SourceStep::Poll);
}

/// 一步结束。成功按步骤推进，失败写错误。
pub(crate) fn finish(model: &mut ShellViewModel, step: SourceStep, result: Result<Value, String>) {
    let plugin_id = model.admin.source_auth.plugin_id.clone();
    let Some(plugin) = plugin_of(model, &plugin_id) else {
        eprintln!("Nana 来源登录结果没有对应插件：{plugin_id}");
        model.admin.source_auth.busy = false;
        return;
    };
    if let SourceStep::Sync { repo_id } = &step {
        match result {
            Ok(_) => model.workspace.request_repository_refresh(),
            Err(error) => {
                eprintln!("Nana 来源仓库后台同步失败：{repo_id}：{error}");
                model.admin.source_auth.error = format!("后台同步失败：{error}");
            }
        }
        return;
    }
    let payload = match result {
        Ok(payload) => payload,
        Err(error) => return fail(model, error),
    };
    match step {
        SourceStep::BeginStatus { .. } => {
            model.admin.source_auth.expected_account_id = account_id(&payload);
            create_session(model, &plugin);
        }
        SourceStep::CreateSession => {
            let state = &mut model.admin.source_auth;
            state.session = Some(payload);
            state.message = "请扫码并在手机端确认，然后检查登录结果。".into();
            state.busy = false;
        }
        SourceStep::RefreshStatus { repo_id } => {
            model.admin.source_auth.status_by_repo.insert(repo_id, payload);
            model.admin.source_auth.busy = false;
        }
        SourceStep::ClearStatus { repo_id } => {
            model.admin.source_auth.pending_clear_status = Some(payload);
            call_clear(model, &plugin, &repo_id);
        }
        SourceStep::Clear { repo_id } => {
            let status = model.admin.source_auth.pending_clear_status.take().unwrap_or(Value::Null);
            let credential = text_at(status.get("credentialRef"));
            match (account_id(&status), credential) {
                (Some(account), Some(_)) => match public_backend_config(&plugin, &status, &account, "") {
                    Ok(mut config) => {
                        config.insert("loginExpired".into(), Value::Bool(true));
                        model.admin.push_effect(AdminEffect::UpdateBackendConfig {
                            repo_id: repo_id.clone(),
                            backend_config: Value::Object(config),
                            step: SourceStep::ClearUpdate { repo_id },
                        });
                    }
                    Err(error) => fail(model, error),
                },
                _ => finish_clear(model, &repo_id),
            }
        }
        SourceStep::ClearUpdate { repo_id } => {
            model.workspace.request_repository_refresh();
            finish_clear(model, &repo_id);
        }
        SourceStep::Poll => provision(model, &plugin, payload),
        SourceStep::ProvisionUpdate { repo_id, name } => {
            let cache = model.admin.source_auth.cache_path.trim().to_string();
            if requires_local_cache(&plugin) && !cache.is_empty() {
                model.admin.push_effect(AdminEffect::ConfigureSourceCache {
                    repo_id: repo_id.clone(),
                    path: cache,
                    step: SourceStep::ProvisionCache { repo_id, name },
                });
            } else {
                complete(model, &repo_id, format!("已更新 {name} 的登录状态，正在同步。"));
            }
        }
        SourceStep::ProvisionCache { repo_id, name } => {
            complete(model, &repo_id, format!("已更新 {name} 的登录状态，正在同步。"));
        }
        SourceStep::ProvisionCreate { repo_id, name } => {
            adopt_created(model, &payload);
            complete(model, &repo_id, format!("已创建 {name}，正在同步。"));
        }
        SourceStep::Sync { .. } => {}
    }
}

fn finish_clear(model: &mut ShellViewModel, repo_id: &str) {
    let name = model.workspace.repositories.iter().find(|item| item.repo_id == repo_id).map(|item| item.name.clone()).unwrap_or_else(|| repo_id.to_string());
    let state = &mut model.admin.source_auth;
    state.status_by_repo.insert(repo_id.to_string(), json!({ "loggedIn": false, "loginExpired": true }));
    state.message = format!("已退出 {name}，仓库和缓存仍保留。");
    state.busy = false;
}

/// 扫码结果落成仓库。没有账号或还没确认时只写提示。
fn provision(model: &mut ShellViewModel, plugin: &PluginManifest, result: Value) {
    let account = account_id(&result);
    let confirmed = result.get("loggedIn").and_then(Value::as_bool) == Some(true)
        || result.get("backendConfig").is_some_and(|item| !item.is_null())
        || text_at(result.get("credentialRef")).is_some();
    let Some(account) = account.filter(|_| confirmed) else {
        let state = &mut model.admin.source_auth;
        state.message = text_at(result.get("message")).unwrap_or_else(|| "尚未完成确认，请稍后重新检查。".into());
        state.busy = false;
        return;
    };
    if let Some(expected) = model.admin.source_auth.expected_account_id.clone()
        && expected != account
    {
        return fail(model, "扫码账号与当前仓库不一致，请使用原账号重新登录。".into());
    }
    let repo_id = model
        .admin
        .source_auth
        .target_repo_id
        .clone()
        .unwrap_or_else(|| format!("{}-{account}", provisioning(plugin, "repoIdPrefix").unwrap_or_else(|| plugin.kind.clone())));
    let cache = model.admin.source_auth.cache_path.trim().to_string();
    let config = match public_backend_config(plugin, &result, &account, &cache) {
        Ok(config) => config,
        Err(error) => return fail(model, error),
    };
    let existing = source_repositories(model, plugin).into_iter().find(|item| item.repo_id == repo_id).map(|item| item.name.clone());
    match existing {
        Some(name) => model.admin.push_effect(AdminEffect::UpdateBackendConfig {
            repo_id: repo_id.clone(),
            backend_config: Value::Object(config),
            step: SourceStep::ProvisionUpdate { repo_id, name },
        }),
        None => {
            let name = repository_name(plugin, &result, &account);
            let scheme = provisioning(plugin, "sourceUriScheme").unwrap_or_else(|| plugin.kind.clone());
            let path = if cache.is_empty() { format!("{scheme}://account/{account}") } else { cache };
            model.admin.push_effect(AdminEffect::CreateSourceRepository {
                repo_id,
                name,
                path,
                backend_plugin_id: plugin.plugin_id.clone(),
                backend_config: Value::Object(config),
            });
        }
    }
}

/// 建仓或更新完成：切到这个仓库，收起会话，排后台同步。
fn complete(model: &mut ShellViewModel, repo_id: &str, message: String) {
    model.workspace.request_repository_refresh();
    model.reduce(ShellMessage::SelectWorkspaceRepository(repo_id.to_string()));
    let state = &mut model.admin.source_auth;
    state.message = message;
    state.session = None;
    state.busy = false;
    model.admin.push_effect(AdminEffect::SyncRepository { repo_id: repo_id.to_string() });
}

/// 把建仓返回的仓库先放进列表，切换不用等列表刷新。
fn adopt_created(model: &mut ShellViewModel, payload: &Value) {
    let Some(repository) = payload.get("repository") else {
        eprintln!("Nana 来源建仓返回没有仓库摘要：{payload}");
        return;
    };
    let Some(repo_id) = text_at(repository.get("repoId")) else {
        eprintln!("Nana 来源建仓返回没有仓库标识");
        return;
    };
    if model.workspace.repositories.iter().any(|item| item.repo_id == repo_id) {
        return;
    }
    let backend = repository.get("backend");
    let cache = repository.get("localCache");
    model.workspace.repositories.push(WorkspaceRepository {
        repo_id,
        name: text_at(repository.get("name")).unwrap_or_default(),
        path: text_at(repository.get("path")).unwrap_or_default(),
        status: text_at(repository.get("status")).unwrap_or_else(|| "ready".into()),
        backend_plugin_id: backend.and_then(|item| text_at(item.get("pluginId"))).unwrap_or_default(),
        capabilities: backend
            .and_then(|item| item.get("capabilities"))
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        cache_required: cache.and_then(|item| item.get("required")).and_then(Value::as_bool) == Some(true),
        cache_status: cache.and_then(|item| text_at(item.get("status"))).unwrap_or_default(),
    });
}

/// 新仓库名称：资料昵称、账号用户名，最后是「插件名 账号」。
fn repository_name(plugin: &PluginManifest, result: &Value, account: &str) -> String {
    result
        .get("profile")
        .and_then(|item| text_at(item.get("nickname")))
        .or_else(|| result.get("account").and_then(|item| text_at(item.get("userName"))))
        .unwrap_or_else(|| format!("{} {account}", plugin.name))
}

/// 登录结果里的账号：`accountId`、`backendConfig.accountId`、`account.id` 依次找。
pub(crate) fn account_id(payload: &Value) -> Option<String> {
    text_at(payload.get("accountId"))
        .or_else(|| payload.get("backendConfig").and_then(|item| text_at(item.get("accountId"))))
        .or_else(|| payload.get("account").and_then(|item| text_at(item.get("id"))))
}

/// 可以存进仓库的后端配置：去掉敏感字段，补账号、凭据引用、来源地址、缓存目录和同步时间。
/// 没有凭据引用就拒绝，和 Vue 同一句话。
pub(crate) fn public_backend_config(plugin: &PluginManifest, result: &Value, account: &str, cache: &str) -> Result<Map<String, Value>, String> {
    let mut config = Map::new();
    if let Some(object) = result.get("backendConfig").and_then(Value::as_object) {
        for (key, value) in object {
            if secret_key(key) {
                eprintln!("Nana 来源登录配置丢弃敏感字段：{key}");
                continue;
            }
            config.insert(key.clone(), value.clone());
        }
    }
    let credential = text_at(result.get("credentialRef")).or_else(|| text_at(config.get("credentialRef")));
    let Some(credential) = credential else {
        return Err("Source 未返回安全凭据引用，已拒绝保存登录配置。".into());
    };
    let scheme = provisioning(plugin, "sourceUriScheme").unwrap_or_else(|| plugin.kind.clone());
    config.insert("accountId".into(), Value::String(account.to_string()));
    config.insert("credentialRef".into(), Value::String(credential));
    config.insert("sourceUri".into(), Value::String(format!("{scheme}://account/{account}")));
    if !cache.trim().is_empty() {
        config.insert("localCachePath".into(), Value::String(cache.trim().to_string()));
    }
    config.insert("lastSyncAt".into(), Value::String(super::support::now_iso8601()));
    Ok(config)
}

/// Vue 的过滤规则：含 cookie、password、secret，或恰好是 token。
fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("cookie") || key.contains("password") || key.contains("secret") || key == "token"
}

/// 字符串或数字都当文本；空白不算。
fn text_at(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(text) = value.as_str().map(str::trim).filter(|text| !text.is_empty()) {
        return Some(text.to_string());
    }
    if value.is_number() {
        return Some(value.to_string());
    }
    None
}

#[cfg(test)]
#[path = "source_provision_tests.rs"]
mod tests;
