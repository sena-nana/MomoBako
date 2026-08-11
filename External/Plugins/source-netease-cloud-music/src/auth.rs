//! 扫码认证与 Windows Credential Manager 会话管理。

use std::fs;

use base64::{engine::general_purpose, Engine as _};
use keyring::{Entry, Error as KeyringError};
use qrcode::{render::svg, QrCode};

use crate::{
    client::{self, QrCheckEnvelope, QrCreateEnvelope},
    models::{
        LegacyStoredSession, PluginPayload, RepoConfig, RuntimeContext, StoredSession,
        KEYRING_SERVICE,
    },
    util::{io_error, now_rfc3339},
};

const CREDENTIAL_PREFIX: &str = "keyring:momobako.netease.source:";

pub(crate) fn create_qr_session(
    runtime: &RuntimeContext,
    payload: PluginPayload,
) -> Result<serde_json::Value, String> {
    let key_response = client::ncm_call(runtime, None, |client, query| async move {
        client.login_qr_key(&query).await
    })?;
    let key = client::extract_qr_unikey(&key_response.body)
        .ok_or_else(|| "二维码 key 接口未返回 unikey".to_string())?;
    let query_key = key.clone();
    let create: QrCreateEnvelope = client::decode(client::ncm_call(
        runtime,
        None,
        |client, query| async move {
            client
                .login_qr_create(&query.param("key", &query_key))
                .await
        },
    )?)?;
    let create = create
        .data
        .ok_or_else(|| "二维码创建接口未返回 data".to_string())?;
    let qrimg = if payload.qrimg.unwrap_or(true) {
        Some(qr_svg_data_url(&create.qrurl)?)
    } else {
        create.qrimg
    };
    Ok(serde_json::json!({ "unikey": key, "qrurl": create.qrurl, "qrimg": qrimg }))
}

/// 扫码完成后先写入系统凭据，再持久化不含 Cookie 的公开会话引用。
pub(crate) fn poll_qr_session(
    runtime: &RuntimeContext,
    payload: PluginPayload,
) -> Result<serde_json::Value, String> {
    let key = payload.key.ok_or_else(|| "missing key".to_string())?;
    let query_key = key.clone();
    let api_response = client::ncm_call(runtime, None, |client, query| async move {
        client.login_qr_check(&query.param("key", &query_key)).await
    })?;
    let response_cookies = api_response.cookie.clone();
    let response: QrCheckEnvelope = client::decode(api_response)?;
    if response.code != 803 {
        return Ok(serde_json::json!({
            "unikey": key,
            "code": response.code,
            "message": response.message
        }));
    }
    let cookie = response
        .cookie
        .or_else(|| join_response_cookies(&response_cookies))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "扫码成功但未返回 Cookie".to_string())?;
    let login = client::fetch_login_status(runtime, &cookie)?;
    let fallback = (login.account.is_none() || login.profile.is_none())
        .then(|| client::fetch_user_account(runtime, &cookie).ok())
        .flatten();
    let account = login
        .account
        .or_else(|| fallback.as_ref().and_then(|value| value.account.clone()))
        .ok_or_else(|| "登录状态未返回账号信息".to_string())?;
    let profile = login
        .profile
        .or_else(|| fallback.and_then(|value| value.profile));
    let credential_ref = store_cookie(account.id, &cookie)?;
    let session = StoredSession {
        credential_ref: credential_ref.clone(),
        account_id: account.id,
        user_name: account.user_name.clone(),
        nickname: profile.as_ref().and_then(|value| value.nickname.clone()),
        avatar_url: profile.as_ref().and_then(|value| value.avatar_url.clone()),
        fetched_at: now_rfc3339()?,
    };
    save_session(runtime, &session)?;
    Ok(serde_json::json!({
        "code": response.code,
        "credentialRef": credential_ref,
        "account": account,
        "profile": profile,
        "backendConfig": {
            "apiBaseUrl": runtime.api_base_url,
            "credentialRef": session.credential_ref,
            "accountId": session.account_id.to_string(),
            "nickname": session.nickname,
            "userName": session.user_name,
            "defaultLevel": runtime.default_level
        }
    }))
}

pub(crate) fn get_login_status(runtime: &RuntimeContext) -> Result<serde_json::Value, String> {
    let Ok((config, cookie)) = resolve_repository_credential(runtime) else {
        return Ok(serde_json::json!({ "loggedIn": false, "loginExpired": true }));
    };
    match client::fetch_login_status(runtime, &cookie) {
        Ok(status) => Ok(serde_json::json!({
            "loggedIn": status.account.is_some(),
            "loginExpired": status.account.is_none(),
            "credentialRef": config.credential_ref,
            "accountId": config.account_id.to_string(),
            "account": status.account,
            "profile": status.profile
        })),
        Err(error) => Ok(serde_json::json!({
            "loggedIn": false,
            "loginExpired": true,
            "credentialRef": config.credential_ref,
            "accountId": config.account_id.to_string(),
            "error": error
        })),
    }
}

pub(crate) fn clear_login(runtime: &RuntimeContext) -> Result<serde_json::Value, String> {
    let credential_ref = runtime
        .repo_backend_config
        .credential_ref
        .clone()
        .or_else(|| load_session(runtime).ok().map(|value| value.credential_ref));
    if let Some(reference) = credential_ref {
        let account_id = account_id_from_ref(&reference)?;
        match Entry::new(KEYRING_SERVICE, &account_id.to_string())
            .and_then(|entry| entry.delete_credential())
        {
            Ok(()) | Err(KeyringError::NoEntry) => {}
            Err(error) => return Err(format!("删除系统凭据失败: {error}")),
        }
    }
    let path = session_file_path(runtime);
    if path.exists() {
        fs::remove_file(path).map_err(io_error)?
    }
    Ok(serde_json::json!({ "cleared": true }))
}

/// 将仓库旧配置中的 Cookie 无网络地迁入系统凭据库。
pub(crate) fn migrate_repository_credential(
    runtime: &RuntimeContext,
) -> Result<serde_json::Value, String> {
    if let (Some(cookie), Some(account_id)) = (
        runtime.repo_backend_config.legacy_cookie.as_deref(),
        runtime.repo_backend_config.account_id,
    ) {
        let credential_ref = store_cookie(account_id, cookie)?;
        let session = StoredSession {
            credential_ref: credential_ref.clone(),
            account_id,
            user_name: None,
            nickname: None,
            avatar_url: None,
            fetched_at: now_rfc3339()?,
        };
        save_session(runtime, &session)?;
        return Ok(serde_json::json!({
            "credentialRef": credential_ref,
            "accountId": account_id.to_string(),
            "migrated": true
        }));
    }
    if let (Some(credential_ref), Some(account_id)) = (
        runtime.repo_backend_config.credential_ref.as_deref(),
        runtime.repo_backend_config.account_id,
    ) {
        validate_credential_account(credential_ref, account_id)?;
        return Ok(serde_json::json!({
            "credentialRef": credential_ref,
            "accountId": account_id.to_string(),
            "migrated": false
        }));
    }
    let session = load_session(runtime)?;
    Ok(serde_json::json!({
        "credentialRef": session.credential_ref,
        "accountId": session.account_id.to_string(),
        "migrated": false
    }))
}

/// 解析仓库认证。旧配置中的 Cookie 会立即迁移到系统凭据库。
pub(crate) fn resolve_repository_credential(
    runtime: &RuntimeContext,
) -> Result<(RepoConfig, String), String> {
    if let (Some(cookie), Some(account_id)) = (
        runtime.repo_backend_config.legacy_cookie.as_deref(),
        runtime.repo_backend_config.account_id,
    ) {
        let credential_ref = store_cookie(account_id, cookie)?;
        let mut session = load_session(runtime).unwrap_or(StoredSession {
            credential_ref: credential_ref.clone(),
            account_id,
            user_name: None,
            nickname: None,
            avatar_url: None,
            fetched_at: now_rfc3339()?,
        });
        session.credential_ref = credential_ref.clone();
        session.account_id = account_id;
        save_session(runtime, &session)?;
        return Ok((
            RepoConfig {
                credential_ref,
                account_id,
                default_level: runtime.default_level.clone(),
            },
            cookie.to_string(),
        ));
    }

    let session = load_session(runtime).ok();
    let credential_ref = runtime
        .repo_backend_config
        .credential_ref
        .clone()
        .or_else(|| session.as_ref().map(|value| value.credential_ref.clone()))
        .ok_or_else(|| "网易云资源库缺少 credentialRef，请重新登录".to_string())?;
    let account_id = runtime
        .repo_backend_config
        .account_id
        .or_else(|| session.as_ref().map(|value| value.account_id))
        .or_else(|| account_id_from_ref(&credential_ref).ok())
        .ok_or_else(|| "网易云资源库缺少 accountId，请重新登录".to_string())?;
    validate_credential_account(&credential_ref, account_id)?;
    let cookie = load_cookie(&credential_ref)?;
    Ok((
        RepoConfig {
            credential_ref,
            account_id,
            default_level: runtime.default_level.clone(),
        },
        cookie,
    ))
}

pub(crate) fn current_login_expired(runtime: &RuntimeContext) -> bool {
    resolve_repository_credential(runtime)
        .ok()
        .and_then(|(_, cookie)| client::fetch_login_status(runtime, &cookie).ok())
        .is_none_or(|status| status.account.is_none())
}

fn credential_ref(account_id: i64) -> String {
    format!("{CREDENTIAL_PREFIX}{account_id}")
}

fn account_id_from_ref(reference: &str) -> Result<i64, String> {
    reference
        .strip_prefix(CREDENTIAL_PREFIX)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| "无效的网易云 credentialRef".to_string())
}

fn validate_credential_account(reference: &str, account_id: i64) -> Result<(), String> {
    if account_id_from_ref(reference)? != account_id {
        return Err("网易云 credentialRef 与 accountId 不匹配，请重新登录".to_string());
    }
    Ok(())
}

fn store_cookie(account_id: i64, cookie: &str) -> Result<String, String> {
    Entry::new(KEYRING_SERVICE, &account_id.to_string())
        .and_then(|entry| entry.set_password(cookie))
        .map_err(|error| format!("写入系统凭据失败: {error}"))?;
    Ok(credential_ref(account_id))
}

fn load_cookie(reference: &str) -> Result<String, String> {
    let account_id = account_id_from_ref(reference)?;
    Entry::new(KEYRING_SERVICE, &account_id.to_string())
        .and_then(|entry| entry.get_password())
        .map_err(|error| format!("读取系统凭据失败: {error}"))
}

fn session_file_path(runtime: &RuntimeContext) -> std::path::PathBuf {
    runtime.plugin_data_dir.join("last-session.json")
}

fn save_session(runtime: &RuntimeContext, session: &StoredSession) -> Result<(), String> {
    let path = session_file_path(runtime);
    let temp = path.with_extension("json.tmp");
    fs::write(
        &temp,
        serde_json::to_vec_pretty(session).map_err(|error| error.to_string())?,
    )
    .map_err(io_error)?;
    if path.exists() {
        fs::remove_file(&path).map_err(io_error)?
    }
    fs::rename(temp, path).map_err(io_error)
}

fn load_session(runtime: &RuntimeContext) -> Result<StoredSession, String> {
    let path = session_file_path(runtime);
    let raw = fs::read_to_string(&path).map_err(io_error)?;
    if let Ok(session) = serde_json::from_str::<StoredSession>(&raw) {
        return Ok(session);
    }
    let legacy: LegacyStoredSession =
        serde_json::from_str(&raw).map_err(|error| error.to_string())?;
    let credential_ref = store_cookie(legacy.account_id, &legacy.cookie)?;
    let session = StoredSession {
        credential_ref,
        account_id: legacy.account_id,
        user_name: legacy.user_name,
        nickname: legacy.nickname,
        avatar_url: legacy.avatar_url,
        fetched_at: legacy.fetched_at,
    };
    save_session(runtime, &session)?;
    Ok(session)
}

fn qr_svg_data_url(url: &str) -> Result<String, String> {
    let code = QrCode::new(url.as_bytes()).map_err(|error| error.to_string())?;
    let svg = code
        .render::<svg::Color<'_>>()
        .min_dimensions(180, 180)
        .dark_color(svg::Color("#111111"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(format!(
        "data:image/svg+xml;base64,{}",
        general_purpose::STANDARD.encode(svg.as_bytes())
    ))
}

fn join_response_cookies(values: &[String]) -> Option<String> {
    let cookies = values
        .iter()
        .filter_map(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    (!cookies.is_empty()).then(|| cookies.join("; "))
}
