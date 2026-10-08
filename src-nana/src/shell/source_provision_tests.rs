//! 来源登录与建仓流程的测试。
//!
//! 期望值来自 `SourceAuthenticationSettings.vue`：仓库标识、名称、路径从登录结果和
//! `repositoryProvisioning` 推出，已有仓库更新配置再配缓存，新仓库跳过首次同步创建，
//! 随后后台同步；敏感字段一律不进仓库配置。

use serde_json::{json, Value};

use crate::backend::services::repository::{
    PluginDependencyStatus, PluginManifest, RepositoryAuthenticationStatus, RepositoryBackendSummary, RepositoryLocalCacheStatus,
    RepositorySummary,
};

use super::super::super::workspace::{WorkspaceEffect, WorkspaceRepository};
use super::super::super::{ShellMessage, ShellViewModel};
use super::super::{AdminEffect, AdminMessage, PluginCallOrigin};
use super::{status_label, SourceStep};

const PLUGIN: &str = "momobako.netease.source";
const REPO: &str = "netease-cloud-music-10086";

fn manifest() -> PluginManifest {
    PluginManifest {
        plugin_id: PLUGIN.into(),
        package_format_version: None,
        package_hash: None,
        provenance: None,
        trust_level: None,
        deployment: None,
        target_triple: None,
        legacy_plugin_ids: Vec::new(),
        name: "Netease Cloud Music Source".into(),
        version: "1.0.0".into(),
        r#type: None,
        kind: "netease-cloud-music".into(),
        category: "source".into(),
        description: String::new(),
        capabilities: Vec::new(),
        enabled: true,
        sdk: "backend".into(),
        entry: Value::Null,
        contributes: json!({
            "source": {
                "authentication": {
                    "kind": "qr",
                    "createSessionMethod": "auth.createQrSession",
                    "pollSessionMethod": "auth.pollQrSession",
                    "statusMethod": "auth.getLoginStatus",
                    "clearMethod": "auth.clearLogin",
                    "repositoryProvisioning": {
                        "sourceUriScheme": "netease-cloud-music",
                        "repoIdPrefix": "netease-cloud-music",
                        "requiresLocalCache": true
                    }
                }
            }
        }),
        source: "builtin".into(),
        runtime: "native-dylib".into(),
        permissions: Vec::new(),
        requires: Vec::new(),
        optional: Vec::new(),
        hooks: Vec::new(),
        compat: Default::default(),
        status: "ready".into(),
        dependency_status: PluginDependencyStatus::default(),
        disable_reason: None,
        degraded: false,
        degradation_reason: None,
        archive_path: None,
    }
}

fn send(model: &mut ShellViewModel, message: AdminMessage) {
    model.reduce(ShellMessage::Admin(message));
}

fn finish(model: &mut ShellViewModel, step: SourceStep, result: Result<Value, String>) {
    send(model, AdminMessage::SourceStepFinished { step, result });
}

fn model() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.admin.plugins = vec![manifest()];
    model
}

/// 已登录的网易云仓库，缓存在 `D:/cache/old`。
fn with_repository(model: &mut ShellViewModel) {
    model.workspace.repositories.push(WorkspaceRepository {
        repo_id: REPO.into(),
        name: "桃子的网易云".into(),
        path: "netease-cloud-music://account/10086".into(),
        status: "ready".into(),
        backend_plugin_id: PLUGIN.into(),
        capabilities: Vec::new(),
        cache_required: true,
        cache_status: "ready".into(),
    });
    model.admin.note_backends(&[RepositorySummary {
        repo_id: REPO.into(),
        name: "桃子的网易云".into(),
        path: "netease-cloud-music://account/10086".into(),
        backend: RepositoryBackendSummary { plugin_id: PLUGIN.into(), kind: "netease-cloud-music".into(), name: "网易云".into(), capabilities: Vec::new() },
        status: "ready".into(),
        asset_count: 0,
        updated_at: String::new(),
        local_cache: Some(RepositoryLocalCacheStatus { required: true, path: Some("D:/cache/old".into()), status: "ready".into() }),
        authentication: Some(RepositoryAuthenticationStatus { required: true, logged_in: true, login_expired: false }),
    }]);
}

/// 取出唯一的插件调用：方法、仓库、载荷和流程步骤。
fn take_call(model: &mut ShellViewModel) -> (String, Option<String>, Value, SourceStep) {
    match model.admin.take_effects().as_slice() {
        [AdminEffect::CallPlugin { plugin_id, method, payload, repository_id, origin: PluginCallOrigin::SourceAuth(step) }] => {
            assert_eq!(plugin_id, PLUGIN);
            (method.clone(), repository_id.clone(), payload.clone(), step.clone())
        }
        other => panic!("unexpected {other:?}"),
    }
}

/// 建好扫码会话并选好缓存目录。
fn open_session(model: &mut ShellViewModel, repo_id: Option<&str>) {
    send(model, AdminMessage::BeginSourceAuth { plugin_id: PLUGIN.into(), repo_id: repo_id.map(str::to_string) });
    let (_, _, _, mut step) = take_call(model);
    if let SourceStep::BeginStatus { .. } = step {
        finish(model, step, Ok(json!({ "accountId": 10086 })));
        step = take_call(model).3;
    }
    assert_eq!(step, SourceStep::CreateSession);
    finish(model, step, Ok(json!({ "unikey": "key-1", "qrurl": "https://music.163.com/login?codekey=key-1" })));
    if model.admin.source_auth.cache_path.is_empty() {
        send(model, AdminMessage::SetSourceCachePath(" D:/cache/new ".into()));
    }
}

fn poll(model: &mut ShellViewModel, result: Value) {
    send(model, AdminMessage::PollSourceAuth { plugin_id: PLUGIN.into() });
    let (method, repository, payload, step) = take_call(model);
    assert_eq!((method.as_str(), repository, step.clone()), ("auth.pollQrSession", None, SourceStep::Poll));
    assert_eq!(payload, json!({ "key": "key-1", "sessionId": "key-1", "persistSession": true }));
    finish(model, step, Ok(result));
}

#[test]
fn new_account_waits_for_a_cache_directory_before_polling() {
    let mut model = model();
    send(&mut model, AdminMessage::BeginSourceAuth { plugin_id: PLUGIN.into(), repo_id: None });
    let (method, repository, payload, step) = take_call(&mut model);
    assert_eq!((method.as_str(), repository), ("auth.createQrSession", None));
    assert_eq!(payload, json!({ "qrImage": true, "qrimg": true }));
    assert!(model.admin.source_auth.busy);
    send(&mut model, AdminMessage::BeginSourceAuth { plugin_id: PLUGIN.into(), repo_id: None });
    assert!(model.admin.take_effects().is_empty());

    finish(&mut model, step, Ok(json!({ "unikey": "key-1", "qrurl": "https://music.163.com/login?codekey=key-1" })));
    assert_eq!(model.admin.source_auth.message, "请扫码并在手机端确认，然后检查登录结果。");
    assert!(!model.admin.source_auth.busy);
    assert!(!model.admin.source_auth.can_poll(true));
    send(&mut model, AdminMessage::PollSourceAuth { plugin_id: PLUGIN.into() });
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, AdminMessage::SetSourceCachePath("   ".into()));
    assert!(model.admin.source_auth.cache_path.is_empty());
    send(&mut model, AdminMessage::SetSourceCachePath(" D:/cache/new ".into()));
    assert!(model.admin.source_auth.can_poll(true));

    send(&mut model, AdminMessage::CancelSourceAuth);
    assert!(model.admin.source_auth.session.is_none());
    assert!(model.admin.source_auth.cache_path.is_empty());
    assert!(model.admin.source_auth.message.is_empty());
}

#[test]
fn unconfirmed_scan_only_updates_the_notice() {
    let mut model = model();
    open_session(&mut model, None);
    poll(&mut model, json!({ "code": 801, "message": "等待扫码" }));
    assert_eq!(model.admin.source_auth.message, "等待扫码");
    assert!(model.admin.take_effects().is_empty());
    poll(&mut model, json!({ "code": 802 }));
    assert_eq!(model.admin.source_auth.message, "尚未完成确认，请稍后重新检查。");
    assert!(model.admin.source_auth.session.is_some());
}

#[test]
fn new_account_creates_the_repository_without_secrets_then_syncs() {
    let mut model = model();
    open_session(&mut model, None);
    poll(&mut model, json!({
        "loggedIn": true,
        "credentialRef": "keyring:netease:10086",
        "backendConfig": {
            "accountId": "10086",
            "cookie": "c",
            "MUSIC_U_Cookie": "c2",
            "password": "p",
            "clientSecret": "s",
            "token": "t",
            "tokenType": "bearer",
            "region": "cn"
        },
        "profile": { "nickname": "桃子" }
    }));
    match model.admin.take_effects().as_slice() {
        [AdminEffect::CreateSourceRepository { repo_id, name, path, backend_plugin_id, backend_config }] => {
            assert_eq!((repo_id.as_str(), name.as_str(), path.as_str(), backend_plugin_id.as_str()), (REPO, "桃子", "D:/cache/new", PLUGIN));
            assert_eq!(backend_config["accountId"], "10086");
            assert_eq!(backend_config["credentialRef"], "keyring:netease:10086");
            assert_eq!(backend_config["sourceUri"], "netease-cloud-music://account/10086");
            assert_eq!(backend_config["localCachePath"], "D:/cache/new");
            assert_eq!((backend_config["tokenType"].as_str(), backend_config["region"].as_str()), (Some("bearer"), Some("cn")));
            assert!(backend_config["lastSyncAt"].as_str().is_some_and(|text| text.ends_with('Z')));
            for key in ["cookie", "MUSIC_U_Cookie", "password", "clientSecret", "token"] {
                assert!(backend_config.get(key).is_none(), "{key} 不能存进仓库配置");
            }
        }
        other => panic!("unexpected {other:?}"),
    }
    let _ = model.workspace.take_effects();
    finish(&mut model, SourceStep::ProvisionCreate { repo_id: REPO.into(), name: "桃子".into() }, Ok(json!({
        "repository": {
            "repoId": REPO,
            "name": "桃子",
            "path": "D:/cache/new",
            "status": "ready",
            "backend": { "pluginId": PLUGIN, "capabilities": ["read"] },
            "localCache": { "required": true, "status": "ready" }
        }
    })));
    assert_eq!(model.admin.source_auth.message, "已创建 桃子，正在同步。");
    assert!(model.admin.source_auth.session.is_none());
    assert!(!model.admin.source_auth.busy);
    assert!(model.workspace.repositories.iter().any(|item| item.repo_id == REPO && item.cache_required));
    assert!(model.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::RefreshRepositories { .. })));
    assert!(matches!(model.admin.take_effects().as_slice(), [AdminEffect::SyncRepository { repo_id }] if repo_id == REPO));

    finish(&mut model, SourceStep::Sync { repo_id: REPO.into() }, Err("网络断开".into()));
    assert_eq!(model.admin.source_auth.error, "后台同步失败：网络断开");
    finish(&mut model, SourceStep::Sync { repo_id: REPO.into() }, Ok(Value::Null));
    assert!(model.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::RefreshRepositories { .. })));
}

#[test]
fn existing_account_updates_the_config_then_the_cache_directory() {
    let mut model = model();
    with_repository(&mut model);
    send(&mut model, AdminMessage::BeginSourceAuth { plugin_id: PLUGIN.into(), repo_id: Some(REPO.into()) });
    assert_eq!(model.admin.source_auth.cache_path, "D:/cache/old");
    let (method, repository, _, step) = take_call(&mut model);
    assert_eq!((method.as_str(), repository.as_deref(), step.clone()), ("auth.getLoginStatus", Some(REPO), SourceStep::BeginStatus { repo_id: REPO.into() }));
    finish(&mut model, step, Ok(json!({ "account": { "id": 10086 } })));
    assert_eq!(model.admin.source_auth.expected_account_id.as_deref(), Some("10086"));
    let step = take_call(&mut model).3;
    finish(&mut model, step, Ok(json!({ "sessionId": "key-1" })));

    poll(&mut model, json!({ "accountId": "10086", "credentialRef": "keyring:netease:10086" }));
    let update = match model.admin.take_effects().as_slice() {
        [AdminEffect::UpdateBackendConfig { repo_id, backend_config, step }] => {
            assert_eq!(repo_id, REPO);
            assert_eq!(backend_config["localCachePath"], "D:/cache/old");
            step.clone()
        }
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(update, SourceStep::ProvisionUpdate { repo_id: REPO.into(), name: "桃子的网易云".into() });
    finish(&mut model, update, Ok(json!({})));
    let cache = match model.admin.take_effects().as_slice() {
        [AdminEffect::ConfigureSourceCache { repo_id, path, step }] => {
            assert_eq!((repo_id.as_str(), path.as_str()), (REPO, "D:/cache/old"));
            step.clone()
        }
        other => panic!("unexpected {other:?}"),
    };
    finish(&mut model, cache, Ok(json!({})));
    assert_eq!(model.admin.source_auth.message, "已更新 桃子的网易云 的登录状态，正在同步。");
    assert!(matches!(model.admin.take_effects().as_slice(), [AdminEffect::SyncRepository { repo_id }] if repo_id == REPO));
}

#[test]
fn another_account_or_a_missing_credential_reference_is_refused() {
    let mut model = model();
    with_repository(&mut model);
    open_session(&mut model, Some(REPO));
    poll(&mut model, json!({ "accountId": "20000", "credentialRef": "keyring:netease:20000" }));
    assert_eq!(model.admin.source_auth.error, "扫码账号与当前仓库不一致，请使用原账号重新登录。");
    assert!(model.admin.take_effects().is_empty());
    assert!(!model.admin.source_auth.busy);

    let mut model = super::super::super::ShellViewModel::default();
    model.admin.plugins = vec![manifest()];
    open_session(&mut model, None);
    poll(&mut model, json!({ "loggedIn": true, "accountId": 1 }));
    assert_eq!(model.admin.source_auth.error, "Source 未返回安全凭据引用，已拒绝保存登录配置。");
    assert!(model.admin.take_effects().is_empty());
}

#[test]
fn check_and_sign_out_keep_the_repository() {
    let mut model = model();
    with_repository(&mut model);
    assert_eq!(status_label(&model, REPO), "已登录");
    send(&mut model, AdminMessage::CheckSourceAuth { plugin_id: PLUGIN.into(), repo_id: REPO.into() });
    let (method, repository, payload, step) = take_call(&mut model);
    assert_eq!((method.as_str(), repository.as_deref(), payload), ("auth.getLoginStatus", Some(REPO), json!({})));
    finish(&mut model, step, Ok(json!({ "loggedIn": true, "loginExpired": true })));
    assert_eq!(status_label(&model, REPO), "登录失效");

    send(&mut model, AdminMessage::ClearSourceAuth { plugin_id: PLUGIN.into(), repo_id: REPO.into() });
    let (method, _, _, step) = take_call(&mut model);
    assert_eq!((method.as_str(), step.clone()), ("auth.clearLogin", SourceStep::Clear { repo_id: REPO.into() }));
    finish(&mut model, step, Ok(json!({})));
    assert_eq!(model.admin.source_auth.message, "已退出 桃子的网易云，仓库和缓存仍保留。");

    model.admin.source_auth.status_by_repo.clear();
    send(&mut model, AdminMessage::ClearSourceAuth { plugin_id: PLUGIN.into(), repo_id: REPO.into() });
    let step = take_call(&mut model).3;
    assert_eq!(step, SourceStep::ClearStatus { repo_id: REPO.into() });
    finish(&mut model, step, Ok(json!({ "accountId": "10086", "credentialRef": "keyring:netease:10086", "cookie": "c" })));
    let step = take_call(&mut model).3;
    finish(&mut model, step, Ok(json!({})));
    let update = match model.admin.take_effects().as_slice() {
        [AdminEffect::UpdateBackendConfig { backend_config, step, .. }] => {
            assert_eq!(backend_config["loginExpired"], true);
            assert!(backend_config.get("cookie").is_none());
            step.clone()
        }
        other => panic!("unexpected {other:?}"),
    };
    let _ = model.workspace.take_effects();
    finish(&mut model, update, Ok(json!({})));
    assert!(model.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::RefreshRepositories { .. })));
    assert_eq!(status_label(&model, REPO), "登录失效");
    assert!(!model.admin.source_auth.busy);
}

#[test]
fn failures_release_the_buttons_and_switching_plugins_resets_the_page() {
    let mut model = model();
    send(&mut model, AdminMessage::BeginSourceAuth { plugin_id: PLUGIN.into(), repo_id: None });
    let step = take_call(&mut model).3;
    send(&mut model, AdminMessage::CancelSourceAuth);
    assert!(model.admin.source_auth.busy);
    finish(&mut model, step, Err("插件未响应".into()));
    assert_eq!(model.admin.source_auth.error, "插件未响应");
    assert!(!model.admin.source_auth.busy);

    let mut other = manifest();
    other.plugin_id = "user.other".into();
    model.admin.plugins.push(other);
    send(&mut model, AdminMessage::ToggleSettings(PLUGIN.into()));
    model.admin.source_auth.message = "旧提示".into();
    send(&mut model, AdminMessage::ToggleSettings("user.other".into()));
    assert_eq!(model.admin.source_auth.plugin_id, "user.other");
    assert!(model.admin.source_auth.message.is_empty());
}
