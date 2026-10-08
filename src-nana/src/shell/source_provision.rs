//! 用来源登录结果创建仓库。
//!
//! 只在登录返回里真有插件标识、账号配置、名称和路径时调用
//! `momobako.repository.create`。配置从返回值里取，丢掉 cookie 一类字段。
//! 缺任何一项就拒绝并记日志，不编造成功。

use serde_json::{Map, Value};

use super::super::ShellViewModel;
use super::AdminEffect;
use crate::backend::services::repository::PluginManifest;

/// 提交按钮旁要补的条件。和真正提交用同一套判断。
pub(super) fn missing_labels(model: &ShellViewModel) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if model.admin.source_auth.plugin_id.trim().is_empty() {
        missing.push("登录结果没有插件标识");
    }
    if account_config(model.admin.source_auth.payload.as_ref(), false).is_none() {
        missing.push("登录结果没有账号配置");
    }
    if model.admin.source_repo_name.trim().is_empty() {
        missing.push("缺仓库名称");
    }
    if model.admin.source_repo_path.trim().is_empty() {
        missing.push("缺仓库路径");
    }
    missing
}

/// 字段齐全才留下创建请求。缺字段只记日志和错误，不调用宿主。
pub(super) fn submit(model: &mut ShellViewModel) {
    if model.admin.source_creating {
        eprintln!("Nana 来源建仓进行中，不能重复提交");
        return;
    }
    let plugin_id = model.admin.source_auth.plugin_id.trim().to_string();
    let name = model.admin.source_repo_name.trim().to_string();
    let path = model.admin.source_repo_path.trim().to_string();
    let payload = model.admin.source_auth.payload.clone();
    if plugin_id.is_empty() || name.is_empty() || path.is_empty() {
        eprintln!("Nana 来源建仓还缺插件标识、名称或路径");
        model.admin.action_message.clear();
        model.admin.action_error = "来源建仓还缺插件标识、名称或路径。".into();
        return;
    }
    let Some(payload) = payload else {
        eprintln!("Nana 来源建仓还没有登录返回");
        model.admin.action_message.clear();
        model.admin.action_error = "还没有宿主登录结果，没有创建仓库。".into();
        return;
    };
    let Some(backend_config) = account_config(Some(&payload), true) else {
        eprintln!("Nana 来源建仓的登录结果没有可用的账号配置");
        model.admin.action_message.clear();
        model.admin.action_error = "登录结果没有账号配置，没有创建仓库。".into();
        return;
    };
    let Some(account_id) = account_id(&payload) else {
        eprintln!("Nana 来源建仓的登录结果没有账号标识");
        model.admin.action_message.clear();
        model.admin.action_error = "登录结果没有账号标识，没有创建仓库。".into();
        return;
    };
    let Some(plugin) = model.admin.plugins.iter().find(|plugin| plugin.plugin_id == plugin_id).cloned() else {
        eprintln!("Nana 来源建仓找不到插件：{plugin_id}");
        model.admin.action_message.clear();
        model.admin.action_error = format!("找不到插件 {plugin_id}，没有创建仓库。");
        return;
    };
    let Some(repo_id) = repository_id(&plugin, &account_id) else {
        eprintln!("Nana 来源建仓缺少仓库标识前缀：{plugin_id}");
        model.admin.action_message.clear();
        model.admin.action_error = "插件没有仓库标识前缀，没有创建仓库。".into();
        return;
    };
    model.admin.action_error.clear();
    model.admin.action_message = "正在创建仓库…".into();
    model.admin.source_creating = true;
    model.admin.effects.push(AdminEffect::CreateSourceRepository {
        repo_id,
        name,
        path,
        backend_plugin_id: plugin_id,
        backend_config,
    });
}

/// 写下创建协议的真实结果，并在成功后刷新仓库列表。
pub(super) fn finish(model: &mut ShellViewModel, result: Result<String, String>) {
    model.admin.source_creating = false;
    match result {
        Ok(notice) => {
            model.admin.action_error.clear();
            model.admin.action_message = notice;
            model.workspace.request_repository_refresh();
        }
        Err(error) => {
            eprintln!("Nana 来源建仓失败：{error}");
            model.admin.action_message.clear();
            model.admin.action_error = if error.is_empty() { "创建仓库失败。".into() } else { error };
        }
    }
}

/// 登录返回里的公开账号配置。敏感字段丢掉，没有凭据引用就拒绝。
fn account_config(payload: Option<&Value>, log: bool) -> Option<Value> {
    let payload = payload?;
    let mut config = Map::new();
    if let Some(object) = payload.get("backendConfig").and_then(|item| item.as_object()) {
        for (key, value) in object {
            if secret_key(key) {
                if log {
                    eprintln!("Nana 来源建仓丢弃登录配置里的敏感字段：{key}");
                }
                continue;
            }
            config.insert(key.clone(), value.clone());
        }
    }
    if let Some(account_id) = account_id(payload) {
        config.insert("accountId".into(), Value::String(account_id));
    }
    if let Some(credential) = text_at(payload.get("credentialRef"))
        .or_else(|| config.get("credentialRef").and_then(|item| item.as_str()).map(str::to_string))
    {
        config.insert("credentialRef".into(), Value::String(credential));
    }
    let credential = config.get("credentialRef").and_then(|item| item.as_str()).map(str::trim).filter(|text| !text.is_empty());
    if credential.is_none() || config.is_empty() {
        if log {
            eprintln!("Nana 来源登录结果没有安全凭据引用");
        }
        return None;
    }
    Some(Value::Object(config))
}

fn repository_id(plugin: &PluginManifest, account_id: &str) -> Option<String> {
    let prefix = provisioning_text(plugin, "repoIdPrefix").filter(|text| !text.is_empty()).unwrap_or_else(|| plugin.kind.clone());
    let prefix = prefix.trim();
    if prefix.is_empty() || account_id.trim().is_empty() {
        return None;
    }
    Some(format!("{prefix}-{account_id}"))
}

fn provisioning_text(plugin: &PluginManifest, key: &str) -> Option<String> {
    plugin
        .contributes
        .get("source")
        .and_then(|item| item.get("authentication"))
        .and_then(|item| item.get("repositoryProvisioning"))
        .and_then(|item| item.get(key))
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn account_id(payload: &Value) -> Option<String> {
    text_at(payload.get("accountId"))
        .or_else(|| payload.get("backendConfig").and_then(|item| text_at(item.get("accountId"))))
        .or_else(|| payload.get("account").and_then(|item| text_at(item.get("id"))))
}

fn text_at(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if let Some(text) = value.as_str().map(str::trim).filter(|text| !text.is_empty()) {
        return Some(text.to_string());
    }
    if let Some(number) = value.as_i64() {
        return Some(number.to_string());
    }
    if let Some(number) = value.as_u64() {
        return Some(number.to_string());
    }
    None
}

fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("cookie") || key.contains("password") || key.contains("secret") || key == "token"
}
